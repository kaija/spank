//! Sensor sources (ADR-0010, design §5–6).
//!
//! - [`LidSource`] / [`AccelSource`]: what the detectors read from.
//! - [`trace`]: the NDJSON trace format v1 (`fixtures/traces/README.md`) and [`TraceReplay`].
//! - [`parse`]: pure decoders for the HID reports.
//! - [`poll`]: adaptive lid polling rate (5 Hz idle, 30 Hz while moving).
//! - `macos` (macOS only): [`IoHidLid`] and [`IoHidAccel`], the real sensors on `05AC:8104`.
//!
//! The FFI layer is kept thin: report decoding lives in [`parse`] so it can be unit-tested, and
//! the accelerometer callback only pushes into a lock-free ring buffer (a panic inside an
//! `extern "C"` callback aborts the process).

pub mod parse;
pub mod poll;
pub mod replay;
pub mod trace;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{IoHidAccel, IoHidLid, accel_driver_present, capabilities, reset_accel_properties};
pub use poll::LidPollSchedule;
pub use replay::{SensorEvent, TraceReplay};
pub use spank_core::{AccelSample, LidSample};
pub use trace::{Label, Trace, TraceError, TraceHeader, TraceWriter};

/// Nominal accelerometer rate with `ReportInterval = 1000 µs` (measured 796–799 Hz, spike S2/S3).
pub const ACCEL_NOMINAL_HZ: f32 = 800.0;
/// `ReportInterval` we request from the driver, in µs.
pub const ACCEL_INTERVAL_US: u32 = 1000;

/// Lid angle samples. Real sources poll when asked; replay sources return recorded samples.
pub trait LidSource {
    fn next(&mut self) -> Option<LidSample>;
}

/// Accelerometer samples, oldest first. `None` means "nothing buffered right now" (or the end
/// of a replay).
pub trait AccelSource {
    fn next(&mut self) -> Option<AccelSample>;

    /// Move everything currently available into `out`.
    fn drain(&mut self, out: &mut Vec<AccelSample>) {
        while let Some(s) = self.next() {
            out.push(s);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SensorError {
    #[error("sensor not found: {0}")]
    NotFound(String),
    #[error("{call} failed: {code}")]
    Io { call: &'static str, code: String },
    #[error("bad report: {0}")]
    BadReport(String),
    #[error("sensor thread: {0}")]
    Thread(String),
}
