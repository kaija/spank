//! Thin IOKit / CoreFoundation helpers (started from `tools/common/iokit.rs`).

use crate::SensorError;
use core_foundation::base::{CFType, TCFType, kCFAllocatorDefault};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::CFTypeRef;
use io_kit_sys::types::{io_iterator_t, io_object_t, io_service_t};
use io_kit_sys::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IORegistryEntrySetCFProperty,
    IOServiceGetMatchingServices, IOServiceMatching,
};
use std::ffi::CString;

pub const APPLE_VID: i64 = 0x05AC;
pub const SPU_PID: i64 = 0x8104;
pub const PAGE_SENSOR: i64 = 0x20;
pub const USAGE_ORIENTATION: i64 = 0x8A;
pub const PAGE_VENDOR: i64 = 0xFF00;
pub const USAGE_ACCEL: i64 = 3;

/// IOReturn as hex plus a name when known.
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

pub fn check(call: &'static str, r: i32) -> Result<(), SensorError> {
    if r == 0 {
        Ok(())
    } else {
        Err(SensorError::Io {
            call,
            code: ioreturn_name(r),
        })
    }
}

/// An owned io_object_t, released on drop.
pub struct Service(pub io_service_t);

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: we own one reference to this object.
        unsafe { IOObjectRelease(self.0) };
    }
}

impl Service {
    pub fn prop_i64(&self, key: &str) -> Option<i64> {
        registry_prop_i64(self.0, key)
    }

    pub fn is_usage(&self, page: i64, usage: i64) -> bool {
        self.prop_i64("PrimaryUsagePage") == Some(page) && self.prop_i64("PrimaryUsage") == Some(usage)
    }

    pub fn set_prop_i64(&self, key: &str, val: i64) -> i32 {
        let k = CFString::new(key);
        let v = CFNumber::from(val);
        // SAFETY: valid service handle and CF objects that outlive the call.
        unsafe { IORegistryEntrySetCFProperty(self.0, k.as_concrete_TypeRef(), v.as_CFTypeRef()) }
    }
}

fn cf_to_i64(ptr: CFTypeRef) -> Option<i64> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `ptr` comes from a Create call, so we own it.
    let v = unsafe { CFType::wrap_under_create_rule(ptr) };
    v.downcast::<CFNumber>().and_then(|n| n.to_i64())
}

pub fn registry_prop_i64(entry: io_object_t, key: &str) -> Option<i64> {
    let k = CFString::new(key);
    // SAFETY: valid entry and key; the result is owned (create rule).
    let p =
        unsafe { IORegistryEntryCreateCFProperty(entry, k.as_concrete_TypeRef(), kCFAllocatorDefault, 0) };
    cf_to_i64(p)
}

/// All registry services of an IOKit class.
pub fn services_of_class(class: &str) -> Vec<Service> {
    let Ok(name) = CString::new(class) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // SAFETY: standard IOKit iteration; `matching` is consumed by IOServiceGetMatchingServices.
    unsafe {
        let matching = IOServiceMatching(name.as_ptr());
        if matching.is_null() {
            return out;
        }
        let mut iter: io_iterator_t = 0;
        if IOServiceGetMatchingServices(0, matching as _, &mut iter) != 0 {
            return out;
        }
        loop {
            let s = IOIteratorNext(iter);
            if s == 0 {
                break;
            }
            out.push(Service(s));
        }
        IOObjectRelease(iter);
    }
    out
}
