//! Real sensors on the SPU HID device `05AC:8104` (design §6).

mod accel;
mod ffi;
mod input;
mod lid;

pub use accel::{IoHidAccel, accel_driver_present, reset_accel_properties};
pub use input::{InputMonitor, input_access, request_input_access};
pub use lid::IoHidLid;

use spank_core::{Capability, SensorCaps};
use std::time::{Duration, Instant};

/// Check both sensors. The lid is opened and read once; the accelerometer is switched on until
/// the first report arrives (or `accel_timeout` passes), then off again.
pub fn capabilities(accel_timeout: Duration) -> SensorCaps {
    let epoch = Instant::now();
    let lid = match IoHidLid::open(epoch).and_then(|l| l.read_angle()) {
        Ok(_) => Capability::Available,
        Err(e) => Capability::Unsupported {
            reason: e.to_string(),
        },
    };
    let accel = match IoHidAccel::probe(epoch, accel_timeout) {
        Ok(a) => {
            a.stop();
            Capability::Available
        }
        Err(e) => Capability::Unsupported {
            reason: e.to_string(),
        },
    };
    SensorCaps { lid, accel }
}
