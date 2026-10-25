//! Power check for the sensor pipeline (T42, ADR-0013). Needs a real MacBook.
//!
//!   cargo run --release -p spank-sensors --example power-check -- [--secs 20]
//!
//! Runs the app's core loop without UI or audio — lid polled per `LidPollSchedule`,
//! accelerometer drained every 50 ms into the `Orchestrator` — first with slaps off (lid only),
//! then on (lid + accelerometer + impact detection), and prints this process's CPU use.
//! Leave the laptop still: a moving lid switches polling to 30 Hz.

use spank_core::{
    AppConfig, Orchestrator, Output, PlayOutcome, PlayRequest, PlaybackStatus, Player, Profile,
    SensorCommand, TimeOfDay,
};
use spank_sensors::{
    ACCEL_NOMINAL_HZ, AccelSource, IoHidAccel, IoHidLid, LidPollSchedule, reset_accel_properties,
};
use std::time::{Duration, Instant};

struct NoAudio;
impl Player for NoAudio {
    fn play(&mut self, _: &PlayRequest) -> PlayOutcome {
        PlayOutcome::Unavailable
    }
    fn status(&self) -> PlaybackStatus {
        PlaybackStatus::default()
    }
}

/// User + system CPU time of this process.
fn cpu_time() -> Duration {
    // SAFETY: getrusage fills the struct we pass.
    let ru = unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        ru
    };
    let tv =
        |t: libc::timeval| Duration::from_secs(t.tv_sec as u64) + Duration::from_micros(t.tv_usec as u64);
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

struct Phase {
    cpu_pct: f64,
    lid_reads: u64,
    accel_samples: u64,
    wakeups: u64,
}

fn run(slaps: bool, secs: f64) -> Phase {
    let epoch = Instant::now();
    let mut cfg = AppConfig::default();
    cfg.slap.enabled = slaps;
    let mut orch = Orchestrator::new(
        cfg,
        Profile::new("p", "P"),
        ACCEL_NOMINAL_HZ,
        TimeOfDay::new(12, 0).expect("valid time"),
    );
    let (mut lid, mut accel) = (None, None);
    for o in orch.start() {
        match o {
            Output::Sensor(SensorCommand::StartLid) => lid = Some(IoHidLid::open(epoch).expect("open lid")),
            Output::Sensor(SensorCommand::StartAccel) => {
                accel = Some(IoHidAccel::start(epoch).expect("start accel"))
            }
            _ => {}
        }
    }
    let mut sched = LidPollSchedule::new();
    let mut next_lid = Instant::now();
    let mut batch = Vec::with_capacity(256);
    let mut player = NoAudio;
    let (mut lid_reads, mut accel_samples, mut wakeups) = (0, 0, 0);
    let cpu0 = cpu_time();
    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < secs {
        let mut wait = Duration::from_secs(1);
        if accel.is_some() {
            wait = wait.min(Duration::from_millis(50));
        }
        if lid.is_some() {
            wait = wait.min(next_lid.saturating_duration_since(Instant::now()));
        }
        std::thread::sleep(wait);
        wakeups += 1;
        if let Some(a) = accel.as_mut() {
            batch.clear();
            a.drain(&mut batch);
            accel_samples += batch.len() as u64;
            orch.on_accel(&batch, &mut player);
        }
        if let Some(l) = lid.as_ref()
            && Instant::now() >= next_lid
            && let Ok(s) = l.read()
        {
            lid_reads += 1;
            next_lid = Instant::now() + Duration::from_millis(sched.on_sample(s.angle_deg, s.at_ms));
            orch.on_lid(s, &mut player);
        }
    }
    let wall = t0.elapsed().as_secs_f64();
    let cpu = (cpu_time() - cpu0).as_secs_f64();
    if let Some(a) = accel {
        a.stop();
    }
    Phase {
        cpu_pct: cpu / wall * 100.0,
        lid_reads,
        accel_samples,
        wakeups,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: f64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(20.0);
    reset_accel_properties();
    for (name, slaps) in [
        ("lid only (slaps off)", false),
        ("lid + accelerometer (slaps on)", true),
    ] {
        let p = run(slaps, secs);
        println!(
            "{name:32} CPU {:5.2} %  wakeups {:5.1}/s  lid reads {:4.1}/s  accel {:5.0} samples/s",
            p.cpu_pct,
            p.wakeups as f64 / secs,
            p.lid_reads as f64 / secs,
            p.accel_samples as f64 / secs
        );
    }
}
