//! Lid angle + accelerometer readers shared by dev tools (findings: docs/spike-report.md S1/S2).
//!
//! These are dev-tool versions of what `spank-sensors` (T17/T24) will provide in the app.
//! Rules learned in the spike: poll the lid feature report (pushed reports are ~1 Hz), restore the
//! accelerometer driver properties on stop, and never panic inside a HID callback.
#![allow(dead_code)]

use super::iokit::*;
use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFIndex;
use core_foundation_sys::runloop::{CFRunLoopGetCurrent, kCFRunLoopDefaultMode};
use core_foundation_sys::set::{CFSetGetCount, CFSetGetValues};
use io_kit_sys::IORegistryEntrySetCFProperty;
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::*;
use io_kit_sys::hid::keys::{IOHIDReportType, kIOHIDReportTypeFeature};
use io_kit_sys::hid::manager::*;
use io_kit_sys::types::io_service_t;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Instant;

const PAGE_SENSOR: i64 = 0x20;
const USAGE_ORIENTATION: i64 = 0x8A;
const PAGE_VENDOR: i64 = 0xFF00;
const USAGE_ACCEL: i64 = 3;
const ACCEL_PROPS: [&str; 3] = [
    "SensorPropertyReportingState",
    "SensorPropertyPowerState",
    "ReportInterval",
];

/// Lid angle via feature-report polling. Create and use it on one thread.
pub struct LidReader {
    dev: IOHIDDeviceRef,
}

impl LidReader {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let mgr = IOHIDManagerCreate(kCFAllocatorDefault, 0);
            let pairs = [
                ("VendorID", APPLE_VID),
                ("ProductID", SPU_PID),
                ("PrimaryUsagePage", PAGE_SENSOR),
                ("PrimaryUsage", USAGE_ORIENTATION),
            ];
            let kv: Vec<_> = pairs
                .iter()
                .map(|(k, v)| (CFString::new(k).as_CFType(), CFNumber::from(*v).as_CFType()))
                .collect();
            let dict = CFDictionary::from_CFType_pairs(&kv);
            IOHIDManagerSetDeviceMatching(mgr, dict.as_concrete_TypeRef());
            let r = IOHIDManagerOpen(mgr, 0);
            if r != 0 {
                return Err(format!("IOHIDManagerOpen: {}", ioreturn_name(r)));
            }
            let set = IOHIDManagerCopyDevices(mgr);
            if set.is_null() || CFSetGetCount(set) == 0 {
                return Err("no lid angle sensor".into());
            }
            let mut vals: Vec<*const c_void> = vec![std::ptr::null(); CFSetGetCount(set) as usize];
            CFSetGetValues(set, vals.as_mut_ptr());
            let dev = vals[0] as IOHIDDeviceRef;
            let r = IOHIDDeviceOpen(dev, 0);
            if r != 0 {
                return Err(format!("IOHIDDeviceOpen(lid): {}", ioreturn_name(r)));
            }
            // `mgr` and `set` intentionally stay alive for the process lifetime.
            Ok(Self { dev })
        }
    }

    /// Current angle in degrees.
    pub fn read(&self) -> Result<u16, String> {
        let mut buf = [0u8; 8];
        let mut len: CFIndex = buf.len() as CFIndex;
        let r =
            unsafe { IOHIDDeviceGetReport(self.dev, kIOHIDReportTypeFeature, 1, buf.as_mut_ptr(), &mut len) };
        if r != 0 {
            return Err(format!("GetReport(lid): {}", ioreturn_name(r)));
        }
        if len < 3 {
            return Err(format!("short lid report ({len} bytes)"));
        }
        Ok(u16::from_le_bytes([buf[1], buf[2]]))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AccelSample {
    /// Seconds since the shared session start.
    pub t: f64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

struct AccelCtx {
    start: Instant,
    tx: SyncSender<AccelSample>,
    dropped: AtomicU64,
}

unsafe extern "C" fn on_accel(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    _type: IOHIDReportType,
    _id: u32,
    report: *mut u8,
    len: CFIndex,
) {
    // Must never panic: this is called from C.
    if context.is_null() || report.is_null() || len < 18 {
        return;
    }
    let c = unsafe { &*(context as *const AccelCtx) };
    let r = unsafe { std::slice::from_raw_parts(report, len as usize) };
    let rd = |o: usize| i32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]) as f32 / 65536.0;
    let s = AccelSample {
        t: c.start.elapsed().as_secs_f64(),
        x: rd(6),
        y: rd(10),
        z: rd(14),
    };
    if c.tx.try_send(s).is_err() {
        c.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

/// Accelerometer input reports delivered on the run loop of the thread that called `start`.
/// Dropping it turns sensor reporting back off.
pub struct AccelStream {
    restore: Vec<(io_service_t, &'static str, i64)>,
    ctx: *mut AccelCtx,
}

fn set_prop(svc: io_service_t, key: &str, val: i64) -> i32 {
    let k = CFString::new(key);
    let v = CFNumber::from(val);
    unsafe { IORegistryEntrySetCFProperty(svc, k.as_concrete_TypeRef(), v.as_CFTypeRef()) }
}

impl AccelStream {
    pub fn start(start: Instant, interval_us: i64, tx: SyncSender<AccelSample>) -> Result<Self, String> {
        let mut restore = Vec::new();
        for d in services_of_class("AppleSPUHIDDriver") {
            if registry_prop_i64(d, "PrimaryUsagePage") == Some(PAGE_VENDOR)
                && registry_prop_i64(d, "PrimaryUsage") == Some(USAGE_ACCEL)
            {
                for (p, want) in ACCEL_PROPS.iter().zip([1, 1, interval_us]) {
                    let old = registry_prop_i64(d, p).unwrap_or(0);
                    let r = set_prop(d, p, want);
                    if r != 0 {
                        return Err(format!("set {p}: {}", ioreturn_name(r)));
                    }
                    restore.push((d, *p, old));
                }
            }
        }
        if restore.is_empty() {
            return Err("no accelerometer driver".into());
        }
        let ctx = Box::into_raw(Box::new(AccelCtx {
            start,
            tx,
            dropped: AtomicU64::new(0),
        }));
        let mut stream = Self { restore, ctx };
        for s in services_of_class("AppleSPUHIDDevice") {
            if registry_prop_i64(s, "PrimaryUsagePage") != Some(PAGE_VENDOR)
                || registry_prop_i64(s, "PrimaryUsage") != Some(USAGE_ACCEL)
            {
                continue;
            }
            unsafe {
                let dev = IOHIDDeviceCreate(kCFAllocatorDefault, s);
                if dev.is_null() {
                    return Err("IOHIDDeviceCreate(accel) returned NULL".into());
                }
                let r = IOHIDDeviceOpen(dev, 0);
                if r != 0 {
                    return Err(format!("IOHIDDeviceOpen(accel): {}", ioreturn_name(r)));
                }
                let buf: &'static mut [u8] = Box::leak(vec![0u8; 4096].into_boxed_slice());
                IOHIDDeviceRegisterInputReportCallback(
                    dev,
                    buf.as_mut_ptr(),
                    buf.len() as CFIndex,
                    Some(on_accel),
                    stream.ctx as *mut c_void,
                );
                IOHIDDeviceScheduleWithRunLoop(dev, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            }
            return Ok(stream);
        }
        stream.stop();
        Err("no accelerometer device".into())
    }

    pub fn dropped(&self) -> u64 {
        unsafe { (*self.ctx).dropped.load(Ordering::Relaxed) }
    }

    fn stop(&mut self) {
        for (d, p, old) in self.restore.drain(..) {
            set_prop(d, p, old);
        }
    }
}

impl Drop for AccelStream {
    fn drop(&mut self) {
        self.stop();
        // `ctx` is leaked on purpose: the device (also leaked) may still reference it.
    }
}

/// `hw.model`, e.g. "Mac16,13".
pub fn hw_model() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "hw.model"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

/// e.g. "26.6.2 (25G83)".
pub fn os_version() -> String {
    let get = |arg: &str| {
        std::process::Command::new("sw_vers")
            .arg(arg)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    format!("{} ({})", get("-productVersion"), get("-buildVersion"))
}
