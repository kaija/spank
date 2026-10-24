//! Spike S4: can a Rust app read macOS Focus (Do Not Disturb) status via INFocusStatusCenter?
//!
//! Must run inside an .app bundle with NSFocusStatusUsageDescription; see `bundle.sh`.
//! Usage: focus-probe [--watch-secs N]

use block2::RcBlock;
use core_foundation_sys::runloop::{CFRunLoopRunInMode, kCFRunLoopDefaultMode};
use objc2_intents::{INFocusStatusAuthorizationStatus, INFocusStatusCenter};
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

fn status_name(s: INFocusStatusAuthorizationStatus) -> &'static str {
    match s {
        INFocusStatusAuthorizationStatus::NotDetermined => "NotDetermined",
        INFocusStatusAuthorizationStatus::Restricted => "Restricted",
        INFocusStatusAuthorizationStatus::Denied => "Denied",
        INFocusStatusAuthorizationStatus::Authorized => "Authorized",
        _ => "Unknown",
    }
}

fn pump(secs: f64) {
    // SAFETY: running the current thread's run loop is always allowed.
    unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, secs, 0) };
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let watch: u64 = args
        .iter()
        .position(|a| a == "--watch-secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    // SAFETY: plain Objective-C calls on the main thread.
    let center = unsafe { INFocusStatusCenter::defaultCenter() };
    let before = unsafe { center.authorizationStatus() };
    println!("authorization before request: {}", status_name(before));

    let result = Arc::new(AtomicIsize::new(-1));
    let r = result.clone();
    let block = RcBlock::new(move |s: INFocusStatusAuthorizationStatus| r.store(s.0, Ordering::SeqCst));
    unsafe { center.requestAuthorizationWithCompletionHandler(Some(&block)) };

    let t0 = Instant::now();
    while result.load(Ordering::SeqCst) < 0 && t0.elapsed() < Duration::from_secs(60) {
        pump(0.25);
    }
    let after = INFocusStatusAuthorizationStatus(result.load(Ordering::SeqCst));
    println!(
        "authorization after request: {} (callback after {:.1}s)",
        if after.0 < 0 {
            "no callback"
        } else {
            status_name(after)
        },
        t0.elapsed().as_secs_f64()
    );

    println!("watching isFocused for {watch}s — toggle Focus / Do Not Disturb in Control Center now");
    let mut last = None;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(watch) {
        let focused = unsafe { center.focusStatus().isFocused() }.map(|n| n.boolValue());
        if Some(focused) != last {
            println!("[{:5.1}s] isFocused = {focused:?}", t0.elapsed().as_secs_f64());
            last = Some(focused);
        }
        pump(0.5);
    }
}
