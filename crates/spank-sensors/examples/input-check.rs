//! Manual check for precise typing mode (T39): Input Monitoring permission and activity timestamps.
//!
//!   cargo run -p spank-sensors --example input-check -- [--request] [--secs 10]
//!
//! Prints the permission state (`--request` shows the system prompt if never asked), then for
//! `--secs` seconds prints how long ago the last key / click happened. Only timing is read; key
//! identities are never looked at. Run from a terminal that has Input Monitoring, or as a .app
//! via `tools/run-as-app.sh`.

use spank_sensors::{InputMonitor, input_access, request_input_access};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    println!("input monitoring: {:?}", input_access());
    if args.iter().any(|a| a == "--request") {
        println!("request → granted: {}", request_input_access());
    }
    let epoch = Instant::now();
    let m = match InputMonitor::start(epoch) {
        Ok(m) => m,
        Err(e) => {
            println!("could not start: {e}");
            return;
        }
    };
    println!("type or click for {secs} s…");
    let mut seen = 0u64;
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last = None;
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(250));
        let now = epoch.elapsed().as_millis() as u64;
        if let Some(t) = m.last_input_ms()
            && last != Some(t)
        {
            seen += 1;
            last = Some(t);
            println!("activity {} ms ago", now.saturating_sub(t));
        }
    }
    m.stop();
    println!("{seen} polls saw new activity (0 with keys pressed means no permission)");
}
