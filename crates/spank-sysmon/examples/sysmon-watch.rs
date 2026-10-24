//! Manual check of every system signal (T20). Needs a real Mac.
//!
//!   cargo run -p spank-sysmon --example sysmon-watch -- [--secs 60]
//!
//! Prints each change. Try: join a call or open Photo Booth (mic / camera), mute or change the
//! volume, lock the screen (Ctrl-Cmd-Q) and unlock, close and reopen the lid (sleep / wake).

use core_foundation_sys::runloop::{CFRunLoopRunInMode, kCFRunLoopDefaultMode};
use spank_sysmon::{SysMonitor, focus_authorization};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    println!("focus authorization: {:?}", focus_authorization());
    let start = Instant::now();
    let _mon = SysMonitor::start(move |s| {
        println!(
            "[{:6.1}s] {}",
            start.elapsed().as_secs_f64(),
            serde_json::to_string(&s).unwrap()
        );
    });
    // Notifications arrive on the main queue: keep the main run loop running.
    while start.elapsed() < Duration::from_secs(secs) {
        // SAFETY: running the current (main) thread's run loop.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, 0) };
    }
}
