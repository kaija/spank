//! Keyboard / trackpad activity for precise typing mode (T39, ADR-0006).
//!
//! An `IOHIDManager` on its own CFRunLoop thread watches keyboards, mice and trackpads. The value
//! callback only records **when** a key or button changed — never which one — as milliseconds
//! since the shared epoch. Reading keyboards needs the Input Monitoring permission
//! (`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`); without it the slap pipeline simply keeps
//! its signal-based typing filter (T23).

use super::ffi::check;
use crate::SensorError;
use core_foundation::array::CFArray;
use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFRelease;
use core_foundation_sys::runloop::{
    CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRunInMode, CFRunLoopStop, kCFRunLoopDefaultMode,
};
use io_kit_sys::hid::base::IOHIDValueRef;
use io_kit_sys::hid::element::IOHIDElementGetUsagePage;
use io_kit_sys::hid::manager::*;
use io_kit_sys::hid::value::IOHIDValueGetElement;
use spank_core::InputAccess;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// `kIOHIDRequestTypeListenEvent` (IOKit/hidsystem/IOHIDLib.h).
const REQUEST_LISTEN_EVENT: u32 = 1;
const ACCESS_GRANTED: u32 = 0;
const ACCESS_DENIED: u32 = 1;

const PAGE_GENERIC_DESKTOP: i64 = 0x01;
const USAGE_MOUSE: i64 = 0x02;
const USAGE_KEYBOARD: i64 = 0x06;
const PAGE_DIGITIZER: i64 = 0x0D;
const USAGE_TOUCH_PAD: i64 = 0x05;
/// Values on these pages count as activity: keys and buttons (clicks), not pointer motion.
const PAGE_KEYBOARD: u32 = 0x07;
const PAGE_BUTTON: u32 = 0x09;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}

/// Current Input Monitoring permission (no prompt).
pub fn input_access() -> InputAccess {
    // SAFETY: plain query without side effects.
    match unsafe { IOHIDCheckAccess(REQUEST_LISTEN_EVENT) } {
        ACCESS_GRANTED => InputAccess::Granted,
        ACCESS_DENIED => InputAccess::Denied,
        _ => InputAccess::Unknown,
    }
}

/// Ask for Input Monitoring. Shows the system prompt the first time; afterwards the user has to
/// change it in System Settings. Returns whether access is granted now.
pub fn request_input_access() -> bool {
    // SAFETY: may show a system prompt; no other side effects.
    unsafe { IOHIDRequestAccess(REQUEST_LISTEN_EVENT) }
}

struct Ctx {
    epoch: Instant,
    /// Last activity in ms since `epoch`, plus one (0 = none yet).
    last: Arc<AtomicU64>,
}

unsafe extern "C" fn on_value(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    value: IOHIDValueRef,
) {
    // Called from C on the input thread: must never panic.
    if context.is_null() || value.is_null() {
        return;
    }
    // SAFETY: `value` is valid for the duration of the callback.
    let element = unsafe { IOHIDValueGetElement(value) };
    if element.is_null() {
        return;
    }
    // SAFETY: valid element from the value above.
    let page = unsafe { IOHIDElementGetUsagePage(element) };
    if page != PAGE_KEYBOARD && page != PAGE_BUTTON {
        return;
    }
    // SAFETY: `context` is the Ctx owned by the input thread, which outlives the registration.
    let ctx = unsafe { &*(context as *const Ctx) };
    let ms = ctx.epoch.elapsed().as_millis() as u64;
    ctx.last.store(ms.saturating_add(1), Ordering::Relaxed);
}

/// Watches keyboard and trackpad activity until stopped (or dropped).
pub struct InputMonitor {
    last: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    runloop: usize,
}

impl InputMonitor {
    /// Start watching. Timestamps are ms since `epoch` (share it with the accelerometer).
    pub fn start(epoch: Instant) -> Result<Self, SensorError> {
        let last = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<usize, SensorError>>(1);
        let (l, s) = (last.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("spank-input".into())
            .spawn(move || input_thread(epoch, l, s, ready_tx))
            .map_err(|e| SensorError::Thread(e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(runloop)) => Ok(Self {
                last,
                stop,
                thread: Some(thread),
                runloop,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                Err(SensorError::Thread("input thread did not start".into()))
            }
        }
    }

    /// Time of the latest key or button change, in ms since the epoch.
    pub fn last_input_ms(&self) -> Option<u64> {
        match self.last.load(Ordering::Relaxed) {
            0 => None,
            v => Some(v - 1),
        }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        let Some(t) = self.thread.take() else {
            return;
        };
        if self.runloop != 0 {
            // SAFETY: the run loop is alive until its thread sees `stop` and exits; we join right
            // after and never call this again (see IoHidAccel::shutdown).
            unsafe { CFRunLoopStop(self.runloop as CFRunLoopRef) };
            self.runloop = 0;
        }
        let _ = t.join();
    }
}

impl Drop for InputMonitor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn matching(page: i64, usage: i64) -> CFDictionary<CFString, CFNumber> {
    CFDictionary::from_CFType_pairs(&[
        (CFString::new("DeviceUsagePage"), CFNumber::from(page)),
        (CFString::new("DeviceUsage"), CFNumber::from(usage)),
    ])
}

fn input_thread(
    epoch: Instant,
    last: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    ready: mpsc::SyncSender<Result<usize, SensorError>>,
) {
    let devices = CFArray::from_CFTypes(&[
        matching(PAGE_GENERIC_DESKTOP, USAGE_KEYBOARD),
        matching(PAGE_GENERIC_DESKTOP, USAGE_MOUSE),
        matching(PAGE_DIGITIZER, USAGE_TOUCH_PAD),
    ]);
    let ctx = Box::new(Ctx { epoch, last });
    let ctx_ptr = &*ctx as *const Ctx as *mut c_void;

    // SAFETY: standard IOHIDManager lifecycle on this thread's run loop; `ctx` outlives the
    // registration (we unregister and close before it drops).
    unsafe {
        let mgr = IOHIDManagerCreate(kCFAllocatorDefault, kIOHIDManagerOptionNone);
        if mgr.is_null() {
            let _ = ready.send(Err(SensorError::NotFound(
                "IOHIDManagerCreate returned NULL".into(),
            )));
            return;
        }
        IOHIDManagerSetDeviceMatchingMultiple(mgr, devices.as_concrete_TypeRef());
        IOHIDManagerRegisterInputValueCallback(mgr, Some(on_value), ctx_ptr);
        let rl = CFRunLoopGetCurrent();
        IOHIDManagerScheduleWithRunLoop(mgr, rl, kCFRunLoopDefaultMode);
        if let Err(e) = check("IOHIDManagerOpen", IOHIDManagerOpen(mgr, kIOHIDManagerOptionNone)) {
            IOHIDManagerUnscheduleFromRunLoop(mgr, rl, kCFRunLoopDefaultMode);
            IOHIDManagerRegisterInputValueCallback(mgr, None, std::ptr::null_mut());
            CFRelease(mgr as *const c_void);
            let _ = ready.send(Err(e));
            return;
        }
        let _ = ready.send(Ok(rl as usize));

        while !stop.load(Ordering::Acquire) {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0);
        }

        IOHIDManagerUnscheduleFromRunLoop(mgr, rl, kCFRunLoopDefaultMode);
        IOHIDManagerRegisterInputValueCallback(mgr, None, std::ptr::null_mut());
        IOHIDManagerClose(mgr, kIOHIDManagerOptionNone);
        CFRelease(mgr as *const c_void);
    }
    drop(ctx);
}
