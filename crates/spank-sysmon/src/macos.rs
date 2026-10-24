use crate::Dedup;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSObjectProtocol,
    NSOperationQueue, NSString,
};
use objc2_intents::{INFocusStatusAuthorizationStatus, INFocusStatusCenter};
use spank_core::SysSignal;
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Focus has no change notification, so it is polled this often.
pub const FOCUS_POLL: Duration = Duration::from_secs(5);

// ---- CoreAudio / CoreMediaIO FFI ------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct Addr {
    selector: u32,
    scope: u32,
    element: u32,
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const SYSTEM_OBJECT: u32 = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");
const ELEMENT_MAIN: u32 = 0;
const HW_PROCESS_LIST: u32 = fourcc(b"prs#");
const HW_DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
const HW_DEVICES: u32 = fourcc(b"dev#");
const PROCESS_PID: u32 = fourcc(b"ppid");
const PROCESS_RUNNING_INPUT: u32 = fourcc(b"piri");
const DEVICE_MUTE: u32 = fourcc(b"mute");
const DEVICE_VOLUME: u32 = fourcc(b"volm");
const DEVICE_RUNNING_SOMEWHERE: u32 = fourcc(b"gone");

type Listener = unsafe extern "C" fn(u32, u32, *const Addr, *mut c_void) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectHasProperty(id: u32, addr: *const Addr) -> u8;
    fn AudioObjectGetPropertyDataSize(
        id: u32,
        addr: *const Addr,
        qs: u32,
        q: *const c_void,
        out: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        id: u32,
        addr: *const Addr,
        qs: u32,
        q: *const c_void,
        io_size: *mut u32,
        out: *mut c_void,
    ) -> i32;
    fn AudioObjectAddPropertyListener(id: u32, addr: *const Addr, l: Listener, data: *mut c_void) -> i32;
    fn AudioObjectRemovePropertyListener(id: u32, addr: *const Addr, l: Listener, data: *mut c_void) -> i32;
}

#[link(name = "CoreMediaIO", kind = "framework")]
unsafe extern "C" {
    fn CMIOObjectGetPropertyDataSize(
        id: u32,
        addr: *const Addr,
        qs: u32,
        q: *const c_void,
        out: *mut u32,
    ) -> i32;
    fn CMIOObjectGetPropertyData(
        id: u32,
        addr: *const Addr,
        qs: u32,
        q: *const c_void,
        size: u32,
        used: *mut u32,
        out: *mut c_void,
    ) -> i32;
    fn CMIOObjectAddPropertyListener(id: u32, addr: *const Addr, l: Listener, data: *mut c_void) -> i32;
    fn CMIOObjectRemovePropertyListener(id: u32, addr: *const Addr, l: Listener, data: *mut c_void) -> i32;
}

fn addr(selector: u32, scope: u32, element: u32) -> Addr {
    Addr {
        selector,
        scope,
        element,
    }
}

/// Read a fixed-size CoreAudio property.
fn ca_get<T: Copy + Default>(id: u32, a: Addr) -> Option<T> {
    let mut v = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    // SAFETY: `v` is a valid buffer of `size` bytes.
    let r = unsafe {
        if AudioObjectHasProperty(id, &a) == 0 {
            return None;
        }
        AudioObjectGetPropertyData(
            id,
            &a,
            0,
            std::ptr::null(),
            &mut size,
            &mut v as *mut T as *mut c_void,
        )
    };
    (r == 0 && size as usize == std::mem::size_of::<T>()).then_some(v)
}

/// Read a CoreAudio property that is an array of object ids.
fn ca_ids(id: u32, a: Addr) -> Vec<u32> {
    let mut size = 0u32;
    // SAFETY: size query, then a buffer of exactly that size.
    unsafe {
        if AudioObjectGetPropertyDataSize(id, &a, 0, std::ptr::null(), &mut size) != 0 {
            return Vec::new();
        }
        let mut v = vec![0u32; size as usize / 4];
        let mut size = (v.len() * 4) as u32;
        if AudioObjectGetPropertyData(
            id,
            &a,
            0,
            std::ptr::null(),
            &mut size,
            v.as_mut_ptr() as *mut c_void,
        ) != 0
        {
            return Vec::new();
        }
        v.truncate(size as usize / 4);
        v
    }
}

fn cmio_get_u32(id: u32, a: Addr) -> Option<u32> {
    let mut v = 0u32;
    let mut used = 0u32;
    // SAFETY: 4-byte buffer.
    let r = unsafe {
        CMIOObjectGetPropertyData(
            id,
            &a,
            0,
            std::ptr::null(),
            4,
            &mut used,
            &mut v as *mut u32 as *mut c_void,
        )
    };
    (r == 0 && used == 4).then_some(v)
}

fn cmio_ids(id: u32, a: Addr) -> Vec<u32> {
    let mut size = 0u32;
    // SAFETY: size query, then a buffer of that size.
    unsafe {
        if CMIOObjectGetPropertyDataSize(id, &a, 0, std::ptr::null(), &mut size) != 0 {
            return Vec::new();
        }
        let mut v = vec![0u32; size as usize / 4];
        let mut used = 0u32;
        if CMIOObjectGetPropertyData(
            id,
            &a,
            0,
            std::ptr::null(),
            size,
            &mut used,
            v.as_mut_ptr() as *mut c_void,
        ) != 0
        {
            return Vec::new();
        }
        v.truncate(used as usize / 4);
        v
    }
}

// ---- one-shot reads -------------------------------------------------------------------------

/// Is any other process recording from an input device?
pub fn mic_in_use() -> bool {
    let me = std::process::id() as i32;
    ca_ids(SYSTEM_OBJECT, addr(HW_PROCESS_LIST, SCOPE_GLOBAL, ELEMENT_MAIN))
        .into_iter()
        .filter(|&p| ca_get::<i32>(p, addr(PROCESS_PID, SCOPE_GLOBAL, ELEMENT_MAIN)) != Some(me))
        .any(|p| ca_get::<u32>(p, addr(PROCESS_RUNNING_INPUT, SCOPE_GLOBAL, ELEMENT_MAIN)).unwrap_or(0) != 0)
}

/// Is any camera in use by any process?
pub fn camera_in_use() -> bool {
    cmio_ids(SYSTEM_OBJECT, addr(HW_DEVICES, SCOPE_GLOBAL, ELEMENT_MAIN))
        .into_iter()
        .any(|d| {
            cmio_get_u32(d, addr(DEVICE_RUNNING_SOMEWHERE, SCOPE_GLOBAL, ELEMENT_MAIN)).unwrap_or(0) != 0
        })
}

fn default_output() -> Option<u32> {
    ca_get::<u32>(SYSTEM_OBJECT, addr(HW_DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN)).filter(|&d| d != 0)
}

/// (muted, volume 0–1) of the default output device. Devices without a volume control report 1.0.
pub fn output_state() -> (bool, f32) {
    let Some(dev) = default_output() else {
        return (false, 1.0);
    };
    let muted = [ELEMENT_MAIN, 1]
        .iter()
        .find_map(|&e| ca_get::<u32>(dev, addr(DEVICE_MUTE, SCOPE_OUTPUT, e)))
        .unwrap_or(0)
        != 0;
    let volume = ca_get::<f32>(dev, addr(DEVICE_VOLUME, SCOPE_OUTPUT, ELEMENT_MAIN)).unwrap_or_else(|| {
        let ch: Vec<f32> = [1, 2]
            .iter()
            .filter_map(|&e| ca_get::<f32>(dev, addr(DEVICE_VOLUME, SCOPE_OUTPUT, e)))
            .collect();
        if ch.is_empty() {
            1.0
        } else {
            ch.iter().sum::<f32>() / ch.len() as f32
        }
    });
    (muted, volume.clamp(0.0, 1.0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusAuthorization {
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}

fn auth_from(s: INFocusStatusAuthorizationStatus) -> FocusAuthorization {
    match s {
        INFocusStatusAuthorizationStatus::Authorized => FocusAuthorization::Authorized,
        INFocusStatusAuthorizationStatus::Denied => FocusAuthorization::Denied,
        INFocusStatusAuthorizationStatus::Restricted => FocusAuthorization::Restricted,
        _ => FocusAuthorization::NotDetermined,
    }
}

pub fn focus_authorization() -> FocusAuthorization {
    // SAFETY: plain property read.
    auth_from(unsafe { INFocusStatusCenter::defaultCenter().authorizationStatus() })
}

/// Focus on/off, or `None` when not authorized or not readable (ADR-0007: needs the signed build
/// with the Communication Notifications profile; otherwise it always reads false).
pub fn focus_status() -> Option<bool> {
    if focus_authorization() != FocusAuthorization::Authorized {
        return None;
    }
    // SAFETY: plain property reads on the shared center.
    unsafe { INFocusStatusCenter::defaultCenter().focusStatus().isFocused() }.map(|n| n.boolValue())
}

/// Ask for Focus permission (shows the system prompt once). Needs `NSFocusStatusUsageDescription`
/// in Info.plist. `done` runs on an arbitrary thread.
pub fn request_focus_authorization(done: impl Fn(FocusAuthorization) + 'static) {
    let block = RcBlock::new(move |s: INFocusStatusAuthorizationStatus| done(auth_from(s)));
    // SAFETY: the block is retained by the framework until it is called.
    unsafe { INFocusStatusCenter::defaultCenter().requestAuthorizationWithCompletionHandler(Some(&block)) };
}

// ---- monitor --------------------------------------------------------------------------------

enum Ping {
    Audio,
    Camera,
    Signal(SysSignal),
    Stop,
}

/// Leaked per-monitor context given to C listeners; freed after they are removed.
struct Hook {
    tx: mpsc::Sender<Ping>,
    camera: bool,
}

unsafe extern "C" fn on_property(_id: u32, _n: u32, _a: *const Addr, data: *mut c_void) -> i32 {
    if data.is_null() {
        return 0;
    }
    // SAFETY: `data` is a Hook that lives until every listener using it is removed.
    let hook = unsafe { &*(data as *const Hook) };
    let _ = hook.tx.send(if hook.camera { Ping::Camera } else { Ping::Audio });
    0
}

struct Registrations {
    audio: *mut c_void,
    camera: *mut c_void,
    ca: Vec<(u32, Addr)>,
    cmio: Vec<(u32, Addr)>,
    processes: BTreeSet<u32>,
    cameras: BTreeSet<u32>,
    output: Option<u32>,
}

impl Registrations {
    fn add_ca(&mut self, id: u32, a: Addr) {
        // SAFETY: `audio` is a live Hook.
        if unsafe { AudioObjectAddPropertyListener(id, &a, on_property, self.audio) } == 0 {
            self.ca.push((id, a));
        }
    }

    fn remove_ca_object(&mut self, id: u32) {
        let audio = self.audio;
        self.ca.retain(|(o, a)| {
            if *o == id {
                // SAFETY: removing a listener we added.
                unsafe { AudioObjectRemovePropertyListener(*o, a, on_property, audio) };
                false
            } else {
                true
            }
        });
    }

    fn add_cmio(&mut self, id: u32, a: Addr) {
        // SAFETY: `camera` is a live Hook.
        if unsafe { CMIOObjectAddPropertyListener(id, &a, on_property, self.camera) } == 0 {
            self.cmio.push((id, a));
        }
    }

    /// Track process objects and the default output device as they come and go.
    fn sync_audio(&mut self) {
        let now: BTreeSet<u32> = ca_ids(SYSTEM_OBJECT, addr(HW_PROCESS_LIST, SCOPE_GLOBAL, ELEMENT_MAIN))
            .into_iter()
            .collect();
        for &p in now.difference(&self.processes.clone()) {
            self.add_ca(p, addr(PROCESS_RUNNING_INPUT, SCOPE_GLOBAL, ELEMENT_MAIN));
        }
        for &p in self.processes.clone().difference(&now) {
            self.remove_ca_object(p);
        }
        self.processes = now;

        let out = default_output();
        if out != self.output {
            if let Some(old) = self.output {
                self.remove_ca_object(old);
            }
            if let Some(dev) = out {
                for e in [ELEMENT_MAIN, 1, 2] {
                    self.add_ca(dev, addr(DEVICE_MUTE, SCOPE_OUTPUT, e));
                    self.add_ca(dev, addr(DEVICE_VOLUME, SCOPE_OUTPUT, e));
                }
            }
            self.output = out;
        }
    }

    fn sync_cameras(&mut self) {
        for d in cmio_ids(SYSTEM_OBJECT, addr(HW_DEVICES, SCOPE_GLOBAL, ELEMENT_MAIN)) {
            if self.cameras.insert(d) {
                self.add_cmio(d, addr(DEVICE_RUNNING_SOMEWHERE, SCOPE_GLOBAL, ELEMENT_MAIN));
            }
        }
    }

    fn remove_all(&mut self) {
        for (o, a) in self.ca.drain(..) {
            // SAFETY: removing listeners we added.
            unsafe { AudioObjectRemovePropertyListener(o, &a, on_property, self.audio) };
        }
        for (o, a) in self.cmio.drain(..) {
            // SAFETY: as above.
            unsafe { CMIOObjectRemovePropertyListener(o, &a, on_property, self.camera) };
        }
    }
}

/// Notification observer tokens. `removeObserver:` is thread-safe, so they may move threads.
type Observer = (
    Retained<NSNotificationCenter>,
    Retained<ProtocolObject<dyn NSObjectProtocol>>,
);

struct Observers(Vec<Observer>);
// SAFETY: only used to call `removeObserver:` (thread-safe) on drop.
unsafe impl Send for Observers {}

impl Drop for Observers {
    fn drop(&mut self) {
        for (center, token) in self.0.drain(..) {
            // SAFETY: removing an observer we added.
            unsafe { center.removeObserver(token.as_ref()) };
        }
    }
}

/// Running system monitor. Dropping it removes every listener and joins the monitor thread.
pub struct SysMonitor {
    tx: mpsc::Sender<Ping>,
    thread: Option<JoinHandle<()>>,
    _observers: Observers,
}

impl SysMonitor {
    /// Start listening. The current mic / camera / output / Focus values are emitted first.
    pub fn start(sink: impl FnMut(SysSignal) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let observers = Observers(observe_notifications(&tx));
        let hook_tx = tx.clone();
        let thread = std::thread::Builder::new()
            .name("spank-sysmon".into())
            .spawn(move || monitor_thread(rx, hook_tx, sink))
            .ok();
        Self {
            tx,
            thread,
            _observers: observers,
        }
    }
}

impl Drop for SysMonitor {
    fn drop(&mut self) {
        let _ = self.tx.send(Ping::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn observe_notifications(tx: &mpsc::Sender<Ping>) -> Vec<Observer> {
    let mut out = Vec::new();
    let queue = NSOperationQueue::mainQueue();
    let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
    let distributed: Retained<NSNotificationCenter> =
        Retained::into_super(NSDistributedNotificationCenter::defaultCenter());
    // SAFETY: the statics are valid NSString constants from AppKit.
    let (sleep, wake) = unsafe { (NSWorkspaceWillSleepNotification, NSWorkspaceDidWakeNotification) };
    let locked = NSString::from_str("com.apple.screenIsLocked");
    let unlocked = NSString::from_str("com.apple.screenIsUnlocked");
    let items: [(&Retained<NSNotificationCenter>, &NSString, SysSignal); 4] = [
        (&workspace, sleep, SysSignal::WillSleep),
        (&workspace, wake, SysSignal::DidWake),
        (&distributed, &locked, SysSignal::ScreenLocked),
        (&distributed, &unlocked, SysSignal::ScreenUnlocked),
    ];
    for (center, name, signal) in items {
        let tx = tx.clone();
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
            let _ = tx.send(Ping::Signal(signal));
        });
        // SAFETY: the center copies the block; the returned token is kept until removal.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&queue), &block)
        };
        out.push((center.clone(), token));
    }
    out
}

fn monitor_thread(rx: mpsc::Receiver<Ping>, tx: mpsc::Sender<Ping>, mut sink: impl FnMut(SysSignal)) {
    let audio = Box::into_raw(Box::new(Hook {
        tx: tx.clone(),
        camera: false,
    })) as *mut c_void;
    let camera = Box::into_raw(Box::new(Hook { tx, camera: true })) as *mut c_void;
    let mut reg = Registrations {
        audio,
        camera,
        ca: Vec::new(),
        cmio: Vec::new(),
        processes: BTreeSet::new(),
        cameras: BTreeSet::new(),
        output: None,
    };
    reg.add_ca(SYSTEM_OBJECT, addr(HW_PROCESS_LIST, SCOPE_GLOBAL, ELEMENT_MAIN));
    reg.add_ca(SYSTEM_OBJECT, addr(HW_DEFAULT_OUTPUT, SCOPE_GLOBAL, ELEMENT_MAIN));
    reg.add_cmio(SYSTEM_OBJECT, addr(HW_DEVICES, SCOPE_GLOBAL, ELEMENT_MAIN));
    reg.sync_audio();
    reg.sync_cameras();

    let mut dedup = Dedup::default();
    let mut emit = |s: SysSignal| {
        if let Some(s) = dedup.filter(s) {
            sink(s);
        }
    };
    let audio_signals = |emit: &mut dyn FnMut(SysSignal)| {
        emit(SysSignal::MicInUse { in_use: mic_in_use() });
        let (muted, volume) = output_state();
        emit(SysSignal::Output { muted, volume });
    };
    audio_signals(&mut emit);
    emit(SysSignal::CameraInUse {
        in_use: camera_in_use(),
    });
    emit(SysSignal::Focus {
        focused: focus_status(),
    });

    loop {
        let first = match rx.recv_timeout(FOCUS_POLL) {
            Ok(p) => p,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                emit(SysSignal::Focus {
                    focused: focus_status(),
                });
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        // Coalesce bursts (a process list change fires many listeners at once).
        let (mut a, mut c, mut stop) = (false, false, false);
        for p in std::iter::once(first).chain(rx.try_iter()) {
            match p {
                Ping::Audio => a = true,
                Ping::Camera => c = true,
                Ping::Signal(s) => emit(s),
                Ping::Stop => stop = true,
            }
        }
        if stop {
            break;
        }
        if a {
            reg.sync_audio();
            audio_signals(&mut emit);
        }
        if c {
            reg.sync_cameras();
            emit(SysSignal::CameraInUse {
                in_use: camera_in_use(),
            });
        }
    }
    reg.remove_all();
    // SAFETY: no listener references the hooks any more.
    unsafe {
        drop(Box::from_raw(audio as *mut Hook));
        drop(Box::from_raw(camera as *mut Hook));
    }
}
