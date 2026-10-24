//! Accelerometer (spike S2, ADR-0014).
//!
//! Reporting is off until three properties are set on `AppleSPUHIDDriver`; afterwards the device
//! pushes 22-byte input reports at ~800 Hz. [`IoHidAccel::start`] runs its own sensor thread with
//! a CFRunLoop. The HID callback only decodes the report and pushes it into an SPSC ring buffer —
//! no allocation, no locks, no panics. Stopping (or dropping) restores the properties to 0.

use super::ffi::*;
use crate::{ACCEL_INTERVAL_US, AccelSource, SensorError, parse};
use core_foundation::base::kCFAllocatorDefault;
use core_foundation_sys::base::{CFIndex, CFRelease};
use core_foundation_sys::runloop::{
    CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRunInMode, CFRunLoopStop, kCFRunLoopDefaultMode,
};
use io_kit_sys::hid::base::IOHIDDeviceRef;
use io_kit_sys::hid::device::*;
use io_kit_sys::hid::keys::IOHIDReportType;
use spank_core::AccelSample;
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PROPS: [&str; 3] = [
    "SensorPropertyReportingState",
    "SensorPropertyPowerState",
    "ReportInterval",
];
/// ~5 s at 800 Hz; the core thread drains every ~50 ms.
const RING_CAPACITY: usize = 4096;
const REPORT_BUF: usize = 64;

/// Is the accelerometer driver present? (Does not turn it on.)
pub fn accel_driver_present() -> bool {
    services_of_class("AppleSPUHIDDriver")
        .iter()
        .any(|d| d.is_usage(PAGE_VENDOR, USAGE_ACCEL))
}

fn accel_drivers() -> Vec<Service> {
    services_of_class("AppleSPUHIDDriver")
        .into_iter()
        .filter(|d| d.is_usage(PAGE_VENDOR, USAGE_ACCEL))
        .collect()
}

/// Turn accelerometer reporting off. Call at startup to undo a crashed run (ADR-0014).
/// Returns how many driver instances were reset.
pub fn reset_accel_properties() -> usize {
    let drivers = accel_drivers();
    for d in &drivers {
        for p in PROPS {
            d.set_prop_i64(p, 0);
        }
    }
    drivers.len()
}

struct Ctx {
    producer: UnsafeCell<rtrb::Producer<AccelSample>>,
    epoch: Instant,
    received: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

unsafe extern "C" fn on_report(
    context: *mut c_void,
    _result: i32,
    _sender: *mut c_void,
    _type: IOHIDReportType,
    _id: u32,
    report: *mut u8,
    len: CFIndex,
) {
    // Called from C on the sensor thread: must never panic.
    if context.is_null() || report.is_null() || len <= 0 {
        return;
    }
    // SAFETY: `context` is the Ctx owned by the sensor thread, which outlives the registration;
    // only this callback touches the producer, and it runs on that one thread.
    let ctx = unsafe { &*(context as *const Ctx) };
    // SAFETY: IOKit hands us `len` valid bytes.
    let bytes = unsafe { std::slice::from_raw_parts(report, len as usize) };
    let Some([x, y, z]) = parse::accel_xyz(bytes) else {
        return;
    };
    let at_ms = ctx.epoch.elapsed().as_millis() as u64;
    // SAFETY: see above — exclusive access from this thread.
    let producer = unsafe { &mut *ctx.producer.get() };
    if producer.push(AccelSample { at_ms, x, y, z }).is_err() {
        ctx.dropped.fetch_add(1, Ordering::Relaxed);
    } else {
        ctx.received.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone)]
struct Shared {
    stop: Arc<AtomicBool>,
    received: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

/// A running accelerometer. Samples are timestamped in ms since the `epoch` given to `start`.
pub struct IoHidAccel {
    consumer: rtrb::Consumer<AccelSample>,
    thread: Option<JoinHandle<()>>,
    runloop: usize,
    shared: Shared,
}

impl IoHidAccel {
    /// Enable reporting and start the sensor thread.
    pub fn start(epoch: Instant) -> Result<Self, SensorError> {
        let (producer, consumer) = rtrb::RingBuffer::new(RING_CAPACITY);
        let shared = Shared {
            stop: Arc::new(AtomicBool::new(false)),
            received: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<usize, SensorError>>(1);
        let s = shared.clone();
        let thread = std::thread::Builder::new()
            .name("spank-accel".into())
            .spawn(move || sensor_thread(producer, epoch, s, ready_tx))
            .map_err(|e| SensorError::Thread(e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(runloop)) => Ok(Self {
                consumer,
                thread: Some(thread),
                runloop,
                shared,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                shared.stop.store(true, Ordering::Release);
                Err(SensorError::Thread("sensor thread did not start".into()))
            }
        }
    }

    /// Start, then wait up to `timeout` for the first report (capability check).
    pub fn probe(epoch: Instant, timeout: Duration) -> Result<Self, SensorError> {
        let s = Self::start(epoch)?;
        let t = Instant::now();
        while s.received() == 0 {
            if t.elapsed() > timeout {
                return Err(SensorError::NotFound(format!(
                    "no accelerometer reports within {} ms",
                    timeout.as_millis()
                )));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(s)
    }

    /// Reports received so far (including ones not yet drained).
    pub fn received(&self) -> u64 {
        self.shared.received.load(Ordering::Relaxed)
    }

    /// Reports lost because the ring buffer was full.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped.load(Ordering::Relaxed)
    }

    /// Stop reporting and join the sensor thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    /// Idempotent: `stop()` runs it, then `Drop` runs it again.
    fn shutdown(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        let Some(t) = self.thread.take() else {
            return;
        };
        if self.runloop != 0 {
            // SAFETY: CFRunLoopStop is thread-safe, and the run loop is alive: its thread only
            // exits after seeing `stop`, and we join it right after. Never called again after
            // the join (the thread handle is gone), so we never touch a freed run loop.
            unsafe { CFRunLoopStop(self.runloop as CFRunLoopRef) };
            self.runloop = 0;
        }
        let _ = t.join();
    }
}

impl AccelSource for IoHidAccel {
    fn next(&mut self) -> Option<AccelSample> {
        self.consumer.pop().ok()
    }
}

impl Drop for IoHidAccel {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn sensor_thread(
    producer: rtrb::Producer<AccelSample>,
    epoch: Instant,
    shared: Shared,
    ready: mpsc::SyncSender<Result<usize, SensorError>>,
) {
    let drivers = accel_drivers();
    let restore = |drivers: &[Service]| {
        for d in drivers {
            for p in PROPS {
                d.set_prop_i64(p, 0);
            }
        }
    };
    if drivers.is_empty() {
        let _ = ready.send(Err(SensorError::NotFound("accelerometer driver".into())));
        return;
    }
    for d in &drivers {
        for (p, v) in PROPS.iter().zip([1, 1, ACCEL_INTERVAL_US as i64]) {
            if let Err(e) = check("IORegistryEntrySetCFProperty", d.set_prop_i64(p, v)) {
                restore(&drivers);
                let _ = ready.send(Err(e));
                return;
            }
        }
    }
    let Some(svc) = services_of_class("AppleSPUHIDDevice")
        .into_iter()
        .find(|s| s.is_usage(PAGE_VENDOR, USAGE_ACCEL))
    else {
        restore(&drivers);
        let _ = ready.send(Err(SensorError::NotFound("accelerometer device".into())));
        return;
    };

    let ctx = Box::new(Ctx {
        producer: UnsafeCell::new(producer),
        epoch,
        received: shared.received.clone(),
        dropped: shared.dropped.clone(),
    });
    let ctx_ptr = &*ctx as *const Ctx as *mut c_void;
    let mut buf = [0u8; REPORT_BUF];

    // SAFETY: standard IOHIDDevice lifecycle on this thread's run loop. `ctx` and `buf` outlive
    // the registration: we unregister and close before they drop.
    unsafe {
        let dev: IOHIDDeviceRef = IOHIDDeviceCreate(kCFAllocatorDefault, svc.0);
        if dev.is_null() {
            restore(&drivers);
            let _ = ready.send(Err(SensorError::NotFound(
                "IOHIDDeviceCreate(accel) returned NULL".into(),
            )));
            return;
        }
        if let Err(e) = check("IOHIDDeviceOpen(accel)", IOHIDDeviceOpen(dev, 0)) {
            CFRelease(dev as *const c_void);
            restore(&drivers);
            let _ = ready.send(Err(e));
            return;
        }
        let rl = CFRunLoopGetCurrent();
        IOHIDDeviceRegisterInputReportCallback(
            dev,
            buf.as_mut_ptr(),
            buf.len() as CFIndex,
            Some(on_report),
            ctx_ptr,
        );
        IOHIDDeviceScheduleWithRunLoop(dev, rl, kCFRunLoopDefaultMode);
        let _ = ready.send(Ok(rl as usize));

        while !shared.stop.load(Ordering::Acquire) {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0);
        }

        IOHIDDeviceUnscheduleFromRunLoop(dev, rl, kCFRunLoopDefaultMode);
        IOHIDDeviceRegisterInputReportCallback(
            dev,
            buf.as_mut_ptr(),
            buf.len() as CFIndex,
            None,
            std::ptr::null_mut(),
        );
        IOHIDDeviceClose(dev, 0);
        CFRelease(dev as *const c_void);
    }
    restore(&drivers);
    drop(ctx);
}
