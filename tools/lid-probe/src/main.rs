//! Spike S1: can we read the MacBook lid angle sensor without root, and how?
//!
//! Usage: lid-probe [--secs N] [--interval-ms N] [--mode poll|input|both] [--ndjson]

#[path = "../../common/iokit.rs"]
mod iokit;

use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFIndex;
use core_foundation_sys::runloop::{CFRunLoopGetCurrent, CFRunLoopRunInMode, kCFRunLoopDefaultMode};
use core_foundation_sys::set::{CFSetGetCount, CFSetGetValues};
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::*;
use io_kit_sys::hid::keys::{IOHIDReportType, kIOHIDReportTypeFeature, kIOHIDReportTypeInput};
use io_kit_sys::hid::manager::*;
use iokit::*;
use std::ffi::c_void;
use std::time::{Duration, Instant};

const USAGE_PAGE_SENSOR: i64 = 0x20;
const USAGE_ORIENTATION: i64 = 0x8A;

fn parse_angle(report: &[u8]) -> Option<u16> {
    // Assumed layout: [report_id, angle_lo, angle_hi, ...]
    (report.len() >= 3).then(|| u16::from_le_bytes([report[1], report[2]]))
}

struct InputCtx {
    start: Instant,
    count: u64,
    ndjson: bool,
    last: Option<u16>,
}

unsafe extern "C" fn on_input(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    type_: IOHIDReportType,
    report_id: u32,
    report: *mut u8,
    len: CFIndex,
) {
    let ctx = unsafe { &mut *(context as *mut InputCtx) };
    let bytes = unsafe { std::slice::from_raw_parts(report, len as usize) };
    ctx.count += 1;
    let angle = parse_angle(bytes);
    let t = ctx.start.elapsed().as_secs_f64();
    if ctx.ndjson {
        println!(
            r#"{{"src":"input","t":{t:.4},"report_id":{report_id},"angle":{}}}"#,
            angle.map_or("null".into(), |a| a.to_string())
        );
    } else if angle != ctx.last || ctx.count <= 3 {
        println!(
            "[input t={t:7.3}s] type={type_} id={report_id} len={len} raw=[{}] angle={angle:?}",
            hex(bytes)
        );
    }
    ctx.last = angle;
}

fn find_lid_devices() -> Vec<IOHIDDeviceRef> {
    unsafe {
        let mgr = IOHIDManagerCreate(kCFAllocatorDefault, 0);
        let pairs = [
            ("VendorID", APPLE_VID),
            ("ProductID", SPU_PID),
            ("PrimaryUsagePage", USAGE_PAGE_SENSOR),
            ("PrimaryUsage", USAGE_ORIENTATION),
        ];
        let kv: Vec<_> = pairs
            .iter()
            .map(|(k, v)| (CFString::new(k).as_CFType(), CFNumber::from(*v).as_CFType()))
            .collect();
        let dict = CFDictionary::from_CFType_pairs(&kv);
        IOHIDManagerSetDeviceMatching(mgr, dict.as_concrete_TypeRef());
        let r = IOHIDManagerOpen(mgr, 0);
        println!("IOHIDManagerOpen -> {}", ioreturn_name(r));
        let set = IOHIDManagerCopyDevices(mgr);
        if set.is_null() {
            return vec![];
        }
        let n = CFSetGetCount(set);
        let mut vals: Vec<*const c_void> = vec![std::ptr::null(); n as usize];
        CFSetGetValues(set, vals.as_mut_ptr());
        // Keep `mgr` and `set` alive for the lifetime of the probe (intentional leak).
        vals.into_iter().map(|p| p as IOHIDDeviceRef).collect()
    }
}

fn main() {
    let args = Args::new();
    let secs: f64 = args.get("--secs", 10.0);
    let interval_ms: u64 = args.get("--interval-ms", 100);
    let mode: String = args.get("--mode", "both".to_string());
    let ndjson = args.has("--ndjson");

    println!("uid={} (0 = root)", unsafe { libc_getuid() });
    if ndjson {
        println!(r#"{{"header":true,"t0_unix_ms":{:.1}}}"#, now_unix_ms());
    }
    let devs = find_lid_devices();
    println!("found {} lid-angle device(s)", devs.len());
    if devs.is_empty() {
        std::process::exit(2);
    }

    for (i, &dev) in devs.iter().enumerate() {
        println!(
            "dev[{i}] product={:?} transport={:?} maxInput={:?} maxFeature={:?} reportInterval={:?}",
            device_prop_string(dev, "Product"),
            device_prop_string(dev, "Transport"),
            device_prop_i64(dev, "MaxInputReportSize"),
            device_prop_i64(dev, "MaxFeatureReportSize"),
            device_prop_i64(dev, "ReportInterval"),
        );
    }
    let dev = devs[0];
    let r = unsafe { IOHIDDeviceOpen(dev, 0) };
    println!("IOHIDDeviceOpen -> {}", ioreturn_name(r));

    if mode == "input" || mode == "both" {
        println!("\n== input-report listening for {secs}s (move the lid!) ==");
        let ctx = Box::into_raw(Box::new(InputCtx {
            start: Instant::now(),
            count: 0,
            ndjson,
            last: None,
        }));
        let buf: &'static mut [u8] = Box::leak(vec![0u8; 64].into_boxed_slice());
        unsafe {
            IOHIDDeviceRegisterInputReportCallback(
                dev,
                buf.as_mut_ptr(),
                buf.len() as CFIndex,
                Some(on_input),
                ctx as *mut c_void,
            );
            IOHIDDeviceScheduleWithRunLoop(dev, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, secs, 0);
            let c = &*ctx;
            println!(
                "input reports received: {} ({:.1} Hz)",
                c.count,
                c.count as f64 / secs
            );
        }
    }

    if mode == "poll" || mode == "both" {
        println!("\n== feature-report polling every {interval_ms}ms for {secs}s (move the lid!) ==");
        let start = Instant::now();
        let mut last: Option<u16> = None;
        let (mut ok, mut fail, mut changes) = (0u64, 0u64, 0u64);
        let mut call_us: Vec<u128> = Vec::new();
        while start.elapsed() < Duration::from_secs_f64(secs) {
            let mut buf = [0u8; 64];
            let mut len: CFIndex = buf.len() as CFIndex;
            let t0 = Instant::now();
            let r =
                unsafe { IOHIDDeviceGetReport(dev, kIOHIDReportTypeFeature, 1, buf.as_mut_ptr(), &mut len) };
            call_us.push(t0.elapsed().as_micros());
            let t = start.elapsed().as_secs_f64();
            if r != 0 {
                fail += 1;
                if fail <= 3 {
                    println!("[poll t={t:7.3}s] GetReport(feature,1) -> {}", ioreturn_name(r));
                }
            } else {
                ok += 1;
                let bytes = &buf[..len as usize];
                let angle = parse_angle(bytes);
                if ndjson {
                    println!(
                        r#"{{"src":"poll","t":{t:.4},"angle":{}}}"#,
                        angle.map_or("null".into(), |a| a.to_string())
                    );
                } else if angle != last || ok <= 2 {
                    println!("[poll t={t:7.3}s] len={len} raw=[{}] angle={angle:?}", hex(bytes));
                }
                if angle != last {
                    changes += 1;
                }
                last = angle;
            }
            std::thread::sleep(Duration::from_millis(interval_ms));
        }
        call_us.sort();
        let med = call_us.get(call_us.len() / 2).copied().unwrap_or(0);
        println!("poll ok={ok} fail={fail} value-changes={changes} median GetReport latency={med}µs");
    }
    let _ = kIOHIDReportTypeInput;
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}
