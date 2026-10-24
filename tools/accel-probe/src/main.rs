//! Spike S2: does the Apple Silicon SPU accelerometer need root, and what do its reports look like?
//!
//! Usage: accel-probe [--secs N] [--enable] [--interval-us N] [--ndjson] [--dump N]
//!   --enable       set SensorPropertyReportingState/PowerState/ReportInterval on AppleSPUHIDDriver
//!                  (expected to need root) and restore the previous values afterwards
//!   --dump N       hex-dump the first N reports (default 5)

#[path = "../../common/iokit.rs"]
mod iokit;

use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFIndex;
use core_foundation_sys::runloop::{CFRunLoopGetCurrent, CFRunLoopRunInMode, kCFRunLoopDefaultMode};
use io_kit_sys::IORegistryEntrySetCFProperty;
use io_kit_sys::hid::device::*;
use io_kit_sys::hid::keys::IOHIDReportType;
use io_kit_sys::types::io_service_t;
use iokit::*;
use std::ffi::c_void;
use std::time::Instant;

const PAGE_VENDOR: i64 = 0xFF00;
const USAGE_ACCEL: i64 = 3;
const PROPS: [&str; 3] = ["SensorPropertyReportingState", "SensorPropertyPowerState", "ReportInterval"];

struct Ctx {
    start: Instant,
    count: u64,
    lens: std::collections::BTreeMap<usize, u64>,
    dump_left: u32,
    ndjson: bool,
    mags: Vec<f64>,
    first_t: Option<f64>,
    last_t: f64,
}

/// Assumed layout (from public reverse-engineering): x/y/z as i32 LE at offsets 6/10/14, 1 g = 65536.
fn parse_xyz(r: &[u8]) -> Option<(f64, f64, f64)> {
    if r.len() < 18 {
        return None;
    }
    let rd = |o: usize| i32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]) as f64 / 65536.0;
    Some((rd(6), rd(10), rd(14)))
}

unsafe extern "C" fn on_input(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    _type: IOHIDReportType,
    report_id: u32,
    report: *mut u8,
    len: CFIndex,
) {
    let c = unsafe { &mut *(context as *mut Ctx) };
    let bytes = unsafe { std::slice::from_raw_parts(report, len as usize) };
    let t = c.start.elapsed().as_secs_f64();
    c.count += 1;
    c.first_t.get_or_insert(t);
    c.last_t = t;
    *c.lens.entry(bytes.len()).or_default() += 1;
    let xyz = parse_xyz(bytes);
    if c.dump_left > 0 {
        c.dump_left -= 1;
        println!("[t={t:7.4}s] id={report_id} len={len} raw=[{}] xyz={xyz:.4?}", hex(bytes));
    }
    if let Some((x, y, z)) = xyz {
        let m = (x * x + y * y + z * z).sqrt();
        c.mags.push(m);
        if c.ndjson {
            println!(r#"{{"t":{t:.5},"x":{x:.5},"y":{y:.5},"z":{z:.5}}}"#);
        }
    }
}

fn set_prop(svc: io_service_t, key: &str, val: i64) -> i32 {
    let k = CFString::new(key);
    let v = CFNumber::from(val);
    unsafe { IORegistryEntrySetCFProperty(svc, k.as_concrete_TypeRef(), v.as_CFTypeRef()) }
}

fn main() {
    let args = Args::new();
    let secs: f64 = args.get("--secs", 5.0);
    let interval_us: i64 = args.get("--interval-us", 1000);
    let enable = args.has("--enable");
    let ndjson = args.has("--ndjson");
    let dump: u32 = args.get("--dump", 5);

    println!("uid={} (0 = root)", unsafe { getuid() });
    if ndjson {
        println!(r#"{{"header":true,"t0_unix_ms":{:.1}}}"#, now_unix_ms());
    }

    // 1. Driver services: inspect (and optionally set) reporting properties.
    let drivers = services_of_class("AppleSPUHIDDriver");
    let mut restore: Vec<(io_service_t, &str, i64)> = Vec::new();
    for &d in &drivers {
        let page = registry_prop_i64(d, "PrimaryUsagePage");
        let usage = registry_prop_i64(d, "PrimaryUsage");
        let cur: Vec<_> = PROPS.iter().map(|p| registry_prop_i64(d, p)).collect();
        println!("driver page={page:?} usage={usage:?} reporting/power/interval={cur:?}");
        let is_accel = page == Some(PAGE_VENDOR) && usage == Some(USAGE_ACCEL);
        if enable && is_accel {
            for (i, (p, want)) in PROPS.iter().zip([1, 1, interval_us]).enumerate() {
                let r = set_prop(d, p, want);
                println!("  set {p}={want} -> {}", ioreturn_name(r));
                if r == 0 {
                    // Properties that were absent are treated as "off" (0) when restoring.
                    restore.push((d, p, cur[i].unwrap_or(0)));
                }
            }
        }
    }

    // 2. Device services: open the accelerometer and listen for input reports.
    let devices = services_of_class("AppleSPUHIDDevice");
    let mut opened = 0;
    for &s in &devices {
        if registry_prop_i64(s, "PrimaryUsagePage") != Some(PAGE_VENDOR)
            || registry_prop_i64(s, "PrimaryUsage") != Some(USAGE_ACCEL)
        {
            continue;
        }
        unsafe {
            let dev = IOHIDDeviceCreate(kCFAllocatorDefault, s);
            if dev.is_null() {
                println!("IOHIDDeviceCreate -> NULL");
                continue;
            }
            println!(
                "accel device maxInput={:?} reportInterval={:?}",
                device_prop_i64(dev, "MaxInputReportSize"),
                device_prop_i64(dev, "ReportInterval")
            );
            let r = IOHIDDeviceOpen(dev, 0);
            println!("IOHIDDeviceOpen -> {}", ioreturn_name(r));
            if r != 0 {
                continue;
            }
            let ctx = Box::into_raw(Box::new(Ctx {
                start: Instant::now(),
                count: 0,
                lens: Default::default(),
                dump_left: dump,
                ndjson,
                mags: Vec::new(),
                first_t: None,
                last_t: 0.0,
            }));
            let buf: &'static mut [u8] = Box::leak(vec![0u8; 4096].into_boxed_slice());
            IOHIDDeviceRegisterInputReportCallback(dev, buf.as_mut_ptr(), buf.len() as CFIndex, Some(on_input), ctx as *mut c_void);
            IOHIDDeviceScheduleWithRunLoop(dev, CFRunLoopGetCurrent(), kCFRunLoopDefaultMode);
            opened += 1;
            println!("== listening {secs}s (slap / tap the laptop!) ==");
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, secs, 0);

            let c = &*ctx;
            let span = c.last_t - c.first_t.unwrap_or(0.0);
            let hz = if span > 0.0 { (c.count.saturating_sub(1)) as f64 / span } else { 0.0 };
            println!("reports={} rate≈{hz:.1} Hz lengths={:?}", c.count, c.lens);
            if !c.mags.is_empty() {
                let n = c.mags.len() as f64;
                let mean = c.mags.iter().sum::<f64>() / n;
                let sd = (c.mags.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / n).sqrt();
                let max = c.mags.iter().cloned().fold(f64::MIN, f64::max);
                let min = c.mags.iter().cloned().fold(f64::MAX, f64::min);
                println!("|a| g: mean={mean:.4} sd={sd:.5} min={min:.4} max={max:.4}");
            }
        }
        break;
    }
    if opened == 0 {
        println!("no accelerometer device opened");
    }

    for (d, p, old) in restore {
        println!("restore {p}={old} -> {}", ioreturn_name(set_prop(d, p, old)));
    }
    for d in drivers.into_iter().chain(devices) {
        release(d);
    }
}

unsafe extern "C" {
    fn getuid() -> u32;
}
