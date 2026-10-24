//! Lid angle via feature-report polling (spike S1: pushed input reports come at only ~1 Hz).

use super::ffi::*;
use crate::{LidSource, SensorError, parse};
use core_foundation::base::{TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFIndex, CFRelease};
use core_foundation_sys::set::{CFSetGetCount, CFSetGetValues};
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::{IOHIDDeviceGetReport, IOHIDDeviceOpen};
use io_kit_sys::hid::keys::kIOHIDReportTypeFeature;
use io_kit_sys::hid::manager::IOHIDManagerRef;
use io_kit_sys::hid::manager::{
    IOHIDManagerClose, IOHIDManagerCopyDevices, IOHIDManagerCreate, IOHIDManagerOpen,
    IOHIDManagerSetDeviceMatching,
};
use spank_core::LidSample;
use std::ffi::c_void;
use std::time::Instant;

/// The lid angle sensor. Not `Send`: create and use it on one thread.
pub struct IoHidLid {
    mgr: IOHIDManagerRef,
    dev: IOHIDDeviceRef,
    epoch: Instant,
}

impl IoHidLid {
    /// Open the sensor. Timestamps are milliseconds since `epoch`.
    pub fn open(epoch: Instant) -> Result<Self, SensorError> {
        // SAFETY: plain IOHIDManager calls; every object is released in Drop or on error.
        unsafe {
            let mgr = IOHIDManagerCreate(kCFAllocatorDefault, 0);
            if mgr.is_null() {
                return Err(SensorError::NotFound("IOHIDManagerCreate returned NULL".into()));
            }
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
            let close = |e| {
                IOHIDManagerClose(mgr, 0);
                CFRelease(mgr as *const c_void);
                e
            };
            check("IOHIDManagerOpen", IOHIDManagerOpen(mgr, 0)).map_err(close)?;
            let set = IOHIDManagerCopyDevices(mgr);
            if set.is_null() {
                return Err(close(SensorError::NotFound("lid angle sensor".into())));
            }
            let n = CFSetGetCount(set);
            if n == 0 {
                CFRelease(set as *const c_void);
                return Err(close(SensorError::NotFound("lid angle sensor".into())));
            }
            let mut vals: Vec<*const c_void> = vec![std::ptr::null(); n as usize];
            CFSetGetValues(set, vals.as_mut_ptr());
            // The manager keeps the device alive; the set only borrowed it.
            let dev = vals[0] as IOHIDDeviceRef;
            CFRelease(set as *const c_void);
            check("IOHIDDeviceOpen(lid)", IOHIDDeviceOpen(dev, 0)).map_err(close)?;
            Ok(Self { mgr, dev, epoch })
        }
    }

    /// Current angle in whole degrees (~0.7 ms per call).
    pub fn read_angle(&self) -> Result<u16, SensorError> {
        let mut buf = [0u8; 8];
        let mut len: CFIndex = buf.len() as CFIndex;
        // SAFETY: `dev` is open; buffer and length are valid for the call.
        let r =
            unsafe { IOHIDDeviceGetReport(self.dev, kIOHIDReportTypeFeature, 1, buf.as_mut_ptr(), &mut len) };
        check("GetReport(lid)", r)?;
        let len = (len.max(0) as usize).min(buf.len());
        parse::lid_angle(&buf[..len])
            .ok_or_else(|| SensorError::BadReport(format!("lid report {:?}", &buf[..len])))
    }

    pub fn read(&self) -> Result<LidSample, SensorError> {
        let angle = self.read_angle()?;
        Ok(LidSample {
            at_ms: self.epoch.elapsed().as_millis() as u64,
            angle_deg: angle as f32,
        })
    }
}

impl LidSource for IoHidLid {
    /// Poll once now. `None` if the read failed.
    fn next(&mut self) -> Option<LidSample> {
        self.read().ok()
    }
}

impl Drop for IoHidLid {
    fn drop(&mut self) {
        // SAFETY: closing the manager closes its devices; then drop our reference.
        unsafe {
            IOHIDManagerClose(self.mgr, 0);
            CFRelease(self.mgr as *const c_void);
        }
    }
}
