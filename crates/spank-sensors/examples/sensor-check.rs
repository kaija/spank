//! Manual hardware check for `IoHidLid` / `IoHidAccel` (T17, T24). Needs a real MacBook.
//!
//!   cargo run --release -p spank-sensors --example sensor-check -- [--secs 3] [--slaps]
//!
//! Prints capabilities, then polls the lid and streams the accelerometer for `--secs` seconds.
//! With `--slaps`, runs the impact extractor live and prints each impact.

use spank_core::{ImpactExtractor, ImpactParams, SlapConfig};
use spank_sensors::{
    ACCEL_NOMINAL_HZ, AccelSource, IoHidAccel, IoHidLid, LidPollSchedule, capabilities,
    reset_accel_properties,
};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: f64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(3.0);
    let slaps = args.iter().any(|a| a == "--slaps");

    println!(
        "reset stale accel properties on {} driver(s)",
        reset_accel_properties()
    );
    let caps = capabilities(Duration::from_millis(500));
    println!("capabilities: {}", serde_json::to_string(&caps).unwrap());

    let epoch = Instant::now();
    let lid = IoHidLid::open(epoch).expect("open lid");
    let mut accel = IoHidAccel::start(epoch).expect("start accel");
    let mut sched = LidPollSchedule::new();
    let mut extractor = ImpactExtractor::new(ImpactParams::new(&SlapConfig::default(), ACCEL_NOMINAL_HZ));
    let (mut lid_n, mut lid_min, mut lid_max, mut lid_reads_us) = (0, u16::MAX, 0, 0u128);
    let mut batch = Vec::new();
    let mut total = 0usize;
    let mut first_ms = None;
    let mut last_ms = 0;
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut next_poll = Instant::now();
    while Instant::now() < end {
        if Instant::now() >= next_poll {
            let t = Instant::now();
            let s = lid.read().expect("read lid");
            lid_reads_us += t.elapsed().as_micros();
            lid_n += 1;
            lid_min = lid_min.min(s.angle_deg as u16);
            lid_max = lid_max.max(s.angle_deg as u16);
            next_poll = Instant::now() + Duration::from_millis(sched.on_sample(s.angle_deg, s.at_ms));
        }
        batch.clear();
        accel.drain(&mut batch);
        if let (Some(f), Some(l)) = (batch.first(), batch.last()) {
            first_ms.get_or_insert(f.at_ms);
            last_ms = l.at_ms;
        }
        total += batch.len();
        if slaps {
            for s in &batch {
                if let Some(c) = extractor.push(s) {
                    println!(
                        "impact at {:>6} ms: peak {:.3} g, {:.0} Hz, {} ms",
                        c.at_ms, c.peak_g, c.mean_freq_hz, c.duration_ms
                    );
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let span = last_ms.saturating_sub(first_ms.unwrap_or(0)).max(1) as f64 / 1000.0;
    println!(
        "lid: {lid_n} polls, {lid_min}–{lid_max}°, mean read {:.2} ms",
        lid_reads_us as f64 / lid_n.max(1) as f64 / 1000.0
    );
    println!(
        "accel: {total} samples, {:.0} Hz, dropped {}, noise floor {:.4} g",
        total as f64 / span,
        accel.dropped(),
        extractor.noise_floor()
    );
    accel.stop();
    println!("stopped; reporting restored to 0");
}
