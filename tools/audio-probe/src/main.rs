//! Spike S5: cost of keeping a cpal output stream open vs opening it on demand.
//!
//! Usage:
//!   audio-probe cpu <none|idle|paused> [--secs N]   CPU used by this process and coreaudiod
//!   audio-probe latency [--runs N] [--gap-ms N]      time until the first audio callback

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[repr(C)]
#[derive(Default)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i32,
}

#[repr(C)]
#[derive(Default)]
struct Rusage {
    ru_utime: Timeval,
    ru_stime: Timeval,
    rest: [i64; 14],
}

unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut Rusage) -> i32;
}

fn self_cpu_secs() -> f64 {
    let mut r = Rusage::default();
    // SAFETY: getrusage writes into the provided struct; RUSAGE_SELF = 0.
    unsafe { getrusage(0, &mut r) };
    let tv = |t: &Timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    tv(&r.ru_utime) + tv(&r.ru_stime)
}

/// Cumulative CPU seconds of coreaudiod (from `ps -o time=`, format [H:]M:SS.ss).
fn coreaudiod_cpu_secs() -> Option<f64> {
    let pid = String::from_utf8(
        Command::new("pgrep")
            .arg("-x")
            .arg("coreaudiod")
            .output()
            .ok()?
            .stdout,
    )
    .ok()?;
    let out = Command::new("ps")
        .args(["-o", "time=", "-p", pid.trim()])
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let mut secs = 0.0;
    for part in s.trim().split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(secs)
}

fn build(device: &cpal::Device, first: Arc<AtomicBool>, calls: Arc<AtomicU64>) -> cpal::Stream {
    let config: cpal::StreamConfig = device.default_output_config().expect("no output config").into();
    device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| {
                data.fill(0.0);
                first.store(true, Ordering::Release);
                calls.fetch_add(1, Ordering::Relaxed);
            },
            |e| eprintln!("stream error: {e}"),
            None,
        )
        .expect("build_output_stream")
}

fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn wait_first(flag: &AtomicBool, t0: Instant) -> Duration {
    while !flag.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_micros(100));
        if t0.elapsed() > Duration::from_secs(5) {
            break;
        }
    }
    t0.elapsed()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let cfg = device.default_output_config().expect("no output config");
    println!(
        "output: {:?} {} Hz, {} ch, {:?}",
        device.description().ok(),
        cfg.sample_rate(),
        cfg.channels(),
        cfg.buffer_size()
    );

    match args.first().map(String::as_str) {
        Some("cpu") => {
            let mode = args.get(1).map(String::as_str).unwrap_or("idle");
            let secs: f64 = arg(&args, "--secs", 30.0);
            let calls = Arc::new(AtomicU64::new(0));
            let stream = (mode != "none").then(|| {
                let s = build(&device, Arc::new(AtomicBool::new(false)), calls.clone());
                s.play().unwrap();
                if mode == "paused" {
                    std::thread::sleep(Duration::from_millis(200));
                    s.pause().unwrap();
                }
                s
            });
            let (c0, a0, t0) = (self_cpu_secs(), coreaudiod_cpu_secs(), Instant::now());
            let calls0 = calls.load(Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs_f64(secs));
            let wall = t0.elapsed().as_secs_f64();
            let me = (self_cpu_secs() - c0) / wall * 100.0;
            let ca = a0.zip(coreaudiod_cpu_secs()).map(|(a, b)| (b - a) / wall * 100.0);
            let cps = (calls.load(Ordering::Relaxed) - calls0) as f64 / wall;
            println!(
                "mode={mode} wall={wall:.1}s self_cpu={me:.3}% coreaudiod_cpu={} callbacks/s={cps:.1}",
                ca.map_or("n/a".into(), |v| format!("{v:.3}%"))
            );
            drop(stream);
        }
        Some("latency") => {
            let runs: usize = arg(&args, "--runs", 10);
            let gap = Duration::from_millis(arg(&args, "--gap-ms", 200));
            let mut cold = Vec::new();
            for _ in 0..runs {
                let first = Arc::new(AtomicBool::new(false));
                let t0 = Instant::now();
                let s = build(&device, first.clone(), Arc::new(AtomicU64::new(0)));
                s.play().unwrap();
                cold.push(wait_first(&first, t0));
                drop(s);
                std::thread::sleep(Duration::from_millis(200));
            }
            // Resume from paused (keeps the stream object alive between sounds).
            let mut warm = Vec::new();
            let first = Arc::new(AtomicBool::new(false));
            let s = build(&device, first.clone(), Arc::new(AtomicU64::new(0)));
            s.play().unwrap();
            for _ in 0..runs {
                s.pause().unwrap();
                std::thread::sleep(gap);
                first.store(false, Ordering::Release);
                let t0 = Instant::now();
                s.play().unwrap();
                warm.push(wait_first(&first, t0));
            }
            let stats = |v: &mut Vec<Duration>| {
                v.sort();
                format!(
                    "min={:?} median={:?} max={:?}",
                    v[0],
                    v[v.len() / 2],
                    v[v.len() - 1]
                )
            };
            println!("cold open (build+play → first callback): {}", stats(&mut cold));
            println!("resume   (pause → play → first callback): {}", stats(&mut warm));
        }
        _ => eprintln!("usage: audio-probe cpu <none|idle|paused> [--secs N] | latency [--runs N]"),
    }
}
