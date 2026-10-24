//! Small IOKit / CoreFoundation helpers shared by the spike probes.
#![allow(dead_code)]

use core_foundation::base::{CFType, TCFType, kCFAllocatorDefault};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFTypeRef;
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::IOHIDDeviceGetProperty;
use io_kit_sys::types::{io_iterator_t, io_object_t, io_service_t};
use io_kit_sys::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IOServiceGetMatchingServices,
    IOServiceMatching,
};
use std::ffi::CString;
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLE_VID: i64 = 0x05AC;
pub const SPU_PID: i64 = 0x8104;

/// Format an IOReturn code as hex with a known name when we have one.
pub fn ioreturn_name(r: i32) -> String {
    let name = match r as u32 {
        0 => "kIOReturnSuccess",
        0xe00002bc => "kIOReturnError",
        0xe00002c1 => "kIOReturnNotPrivileged",
        0xe00002c2 => "kIOReturnBadArgument",
        0xe00002c5 => "kIOReturnExclusiveAccess",
        0xe00002c7 => "kIOReturnUnsupported",
        0xe00002cd => "kIOReturnNotOpen",
        0xe00002d6 => "kIOReturnTimeout",
        0xe00002e2 => "kIOReturnNotPermitted",
        0xe00002f0 => "kIOReturnNotFound",
        _ => "?",
    };
    format!("0x{:08x} ({name})", r as u32)
}

pub fn now_unix_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

fn cf_to_i64(ptr: CFTypeRef, owned: bool) -> Option<i64> {
    if ptr.is_null() {
        return None;
    }
    let v = unsafe {
        if owned {
            CFType::wrap_under_create_rule(ptr)
        } else {
            CFType::wrap_under_get_rule(ptr)
        }
    };
    v.downcast::<CFNumber>().and_then(|n| n.to_i64())
}

pub fn device_prop_i64(dev: IOHIDDeviceRef, key: &str) -> Option<i64> {
    let k = CFString::new(key);
    cf_to_i64(unsafe { IOHIDDeviceGetProperty(dev, k.as_concrete_TypeRef()) }, false)
}

pub fn device_prop_string(dev: IOHIDDeviceRef, key: &str) -> Option<String> {
    let k = CFString::new(key);
    let p = unsafe { IOHIDDeviceGetProperty(dev, k.as_concrete_TypeRef()) };
    if p.is_null() {
        return None;
    }
    let v = unsafe { CFType::wrap_under_get_rule(p) };
    v.downcast::<CFString>().map(|s| s.to_string())
}

pub fn registry_prop_i64(entry: io_object_t, key: &str) -> Option<i64> {
    let k = CFString::new(key);
    let p = unsafe {
        IORegistryEntryCreateCFProperty(entry, k.as_concrete_TypeRef(), kCFAllocatorDefault, 0)
    };
    cf_to_i64(p, true)
}

/// All registry services of the given IOKit class. Caller releases them.
pub fn services_of_class(class: &str) -> Vec<io_service_t> {
    let name = CString::new(class).unwrap();
    let mut out = Vec::new();
    unsafe {
        let matching = IOServiceMatching(name.as_ptr());
        let mut iter: io_iterator_t = 0;
        // IOServiceGetMatchingServices consumes `matching`.
        let kr = IOServiceGetMatchingServices(0, matching as _, &mut iter);
        if kr != 0 {
            eprintln!("IOServiceGetMatchingServices({class}) failed: {}", ioreturn_name(kr));
            return out;
        }
        loop {
            let s = IOIteratorNext(iter);
            if s == 0 {
                break;
            }
            out.push(s);
        }
        IOObjectRelease(iter);
    }
    out
}

pub fn release(obj: io_object_t) {
    unsafe {
        IOObjectRelease(obj);
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// Minimal `--flag value` parser.
pub struct Args(Vec<String>);
impl Args {
    pub fn new() -> Self {
        Args(std::env::args().skip(1).collect())
    }
    pub fn has(&self, flag: &str) -> bool {
        self.0.iter().any(|a| a == flag)
    }
    pub fn get<T: std::str::FromStr>(&self, flag: &str, default: T) -> T {
        self.0
            .iter()
            .position(|a| a == flag)
            .and_then(|i| self.0.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }
}
