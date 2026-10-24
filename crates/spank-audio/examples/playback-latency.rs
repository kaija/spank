//! Manual check of the playback engine on the real output device (T19).
//!
//!   cargo run --release -p spank-audio --example playback-latency -- [--runs 10] [--gap-ms 2000] [--audible]
//!
//! Plays a 100 ms tone (silent unless `--audible`) and reports the delay from the request to the
//! audio callback that started it. The stream is paused between runs, as in the app. Target < 50 ms.

use spank_audio::{AudioPlayer, Clip};
use spank_core::Priority;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn arg(args: &[String], flag: &str, default: u64) -> u64 {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let runs = arg(&args, "--runs", 10);
    let gap = arg(&args, "--gap-ms", 2000);
    let gain = if args.iter().any(|a| a == "--audible") {
        0.2
    } else {
        0.0
    };

    let t = Instant::now();
    let player = AudioPlayer::new(t).expect("audio output");
    println!("stream ready in {:?}, format {:?}", t.elapsed(), player.format());
    let tone: Vec<f32> = (0..4800)
        .map(|i| (2.0 * std::f32::consts::PI * 660.0 * i as f32 / 48_000.0).sin() * 0.5)
        .collect();
    let clip = Arc::new(Clip::new(tone, 1, 48_000));

    let mut delays = Vec::new();
    for i in 0..runs {
        std::thread::sleep(Duration::from_millis(gap));
        let before = player.last_start_delay();
        player.play_clip(clip.clone(), Priority(1), gain);
        let t = Instant::now();
        while player.last_start_delay() == before && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_micros(200));
        }
        let d = player.last_start_delay().unwrap_or(Duration::MAX);
        println!("run {i}: {:.1} ms", d.as_secs_f64() * 1000.0);
        delays.push(d);
    }
    delays.sort();
    println!(
        "min {:.1} ms, median {:.1} ms, max {:.1} ms",
        delays[0].as_secs_f64() * 1000.0,
        delays[delays.len() / 2].as_secs_f64() * 1000.0,
        delays[delays.len() - 1].as_secs_f64() * 1000.0
    );
}
