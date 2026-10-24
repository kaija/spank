//! T10: record labeled lid + accelerometer traces (format: fixtures/traces/README.md).
//!
//! Usage:
//!   trace-recorder --check [--secs N]                 sensor access check, prints a JSON summary (T09)
//!   trace-recorder --scenario <name|all> --out <dir>  guided recording; `--list` shows scenarios
//!
//! The accelerometer callback runs on the main thread's run loop; the lid is polled at 30 Hz on a
//! worker thread; prompts and label timing run on another thread. Everything is collected in memory
//! (a minute is ~50k samples) and written sorted by time at the end.

#[path = "../../common/iokit.rs"]
mod iokit;
#[path = "../../common/sensors.rs"]
mod sensors;

use core_foundation_sys::runloop::{CFRunLoopRunInMode, kCFRunLoopDefaultMode};
use sensors::{AccelSample, AccelStream, LidReader};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const ACCEL_INTERVAL_US: i64 = 1000;
const LID_POLL_HZ: f64 = 30.0;
const PLAYBACK_SOUND: &str = "/System/Library/Sounds/Funk.aiff";

struct Segment {
    label: &'static str,
    secs: u64,
    prompt: &'static str,
    /// Play a system sound in a loop during this segment (speaker vibration case).
    play: bool,
}

const fn seg(label: &'static str, secs: u64, prompt: &'static str) -> Segment {
    Segment {
        label,
        secs,
        prompt,
        play: false,
    }
}

const fn seg_play(label: &'static str, secs: u64, prompt: &'static str) -> Segment {
    Segment {
        label,
        secs,
        prompt,
        play: true,
    }
}

fn scenarios() -> Vec<(&'static str, &'static str, Vec<Segment>)> {
    vec![
        (
            "baseline",
            "Same as the spike recording, in trace format v1",
            vec![
                seg("idle", 5, "保持不動，手離開電腦"),
                seg("typing", 8, "正常速度打字"),
                seg("trackpad", 6, "點按觸控板幾下"),
                seg("slap_light", 8, "輕拍掌托旁邊 3 次（間隔約 2 秒）"),
                seg("slap_medium", 8, "中等力道拍 3 次"),
                seg("slap_hard", 8, "用力拍 3 次"),
                seg("slap_combo", 6, "快速連拍 5 下"),
                seg("lid_move", 12, "慢慢把螢幕闔到約 30°，再推開到最大"),
            ],
        ),
        (
            "desk",
            "Picking up / setting down / sliding the laptop",
            vec![
                seg("idle", 4, "保持不動"),
                seg("lift_set_down", 12, "把筆電拿起約 5 公分再放下，做 3 次"),
                seg("slide", 8, "在桌上前後推動筆電幾次"),
                seg("idle_end", 4, "保持不動"),
            ],
        ),
        (
            "carry",
            "Walking around holding the laptop",
            vec![
                seg("idle", 3, "保持不動"),
                seg("carry_walk", 15, "拿著筆電站起來走一圈"),
                seg("set_down", 6, "把筆電放回桌上"),
            ],
        ),
        (
            "playback",
            "Our own sound playing through the speakers",
            vec![
                seg_play("playback_idle", 8, "（會自動播放音效）不要碰電腦"),
                seg_play("playback_slap", 10, "音效持續播放中，中等力道拍 3 次"),
                seg("idle_end", 3, "保持不動"),
            ],
        ),
        (
            "lid",
            "Lid overbend and quick lid moves",
            vec![
                seg("lid_overbend", 12, "把螢幕往後推到最大角度，來回 3 次"),
                seg("lid_close_open", 12, "把螢幕闔到快關上（約 20°），再打開"),
                seg("lid_fast", 8, "快速小幅度開合螢幕幾次"),
            ],
        ),
        (
            "typing",
            "Heavy typing, trackpad, bumps near the laptop",
            vec![
                seg("typing_fast", 12, "用最快速度大力打字"),
                seg("typing_trackpad", 10, "打字夾雜用力點觸控板"),
                seg("table_bump", 8, "用手拍筆電旁邊的桌面 3 次（不是拍電腦）"),
            ],
        ),
    ]
}

#[derive(Default)]
struct Capture {
    accel: Vec<AccelSample>,
    lid: Vec<(f64, u16)>,
    accel_error: Option<String>,
    lid_error: Option<String>,
    accel_dropped: u64,
}

/// Run sensors on this thread's run loop until `done` is set.
fn capture(start: Instant, done: Arc<AtomicBool>) -> Capture {
    let mut cap = Capture::default();

    let lid_done = done.clone();
    let lid_thread = std::thread::spawn(move || -> Result<Vec<(f64, u16)>, String> {
        let lid = LidReader::open()?;
        let mut out = Vec::new();
        let period = Duration::from_secs_f64(1.0 / LID_POLL_HZ);
        while !lid_done.load(Ordering::Relaxed) {
            let t = start.elapsed().as_secs_f64();
            out.push((t, lid.read()?));
            std::thread::sleep(period);
        }
        Ok(out)
    });

    let (tx, rx) = mpsc::sync_channel::<AccelSample>(200_000);
    let stream = match AccelStream::start(start, ACCEL_INTERVAL_US, tx) {
        Ok(s) => Some(s),
        Err(e) => {
            cap.accel_error = Some(e);
            None
        }
    };
    while !done.load(Ordering::Relaxed) {
        // SAFETY: pumping the current thread's run loop delivers the HID callbacks.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0) };
        cap.accel.extend(rx.try_iter());
    }
    if let Some(s) = &stream {
        cap.accel_dropped = s.dropped();
    }
    drop(stream);
    cap.accel.extend(rx.try_iter());

    match lid_thread.join() {
        Ok(Ok(v)) => cap.lid = v,
        Ok(Err(e)) => cap.lid_error = Some(e),
        Err(_) => cap.lid_error = Some("lid thread panicked".into()),
    }
    cap
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn opt_json(v: &Option<String>) -> String {
    v.as_deref().map_or("null".into(), json_str)
}

fn check(secs: f64) -> bool {
    let start = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let d = done.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(secs));
        d.store(true, Ordering::Relaxed);
    });
    let cap = capture(start, done);
    let span = cap.accel.last().map_or(0.0, |l| l.t) - cap.accel.first().map_or(0.0, |f| f.t);
    let hz = if span > 0.0 {
        (cap.accel.len().saturating_sub(1)) as f64 / span
    } else {
        0.0
    };
    let (lmin, lmax) = cap
        .lid
        .iter()
        .fold((u16::MAX, 0), |(a, b), &(_, d)| (a.min(d), b.max(d)));
    let ok =
        cap.accel_error.is_none() && cap.lid_error.is_none() && !cap.accel.is_empty() && !cap.lid.is_empty();
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    println!(
        "{{\"ok\":{ok},\"uid\":{},\"exe\":{},\"accel\":{{\"samples\":{},\"hz\":{hz:.1},\"dropped\":{},\"error\":{}}},\"lid\":{{\"samples\":{},\"min\":{},\"max\":{},\"error\":{}}}}}",
        unsafe { getuid() },
        json_str(&exe),
        cap.accel.len(),
        cap.accel_dropped,
        opt_json(&cap.accel_error),
        cap.lid.len(),
        if cap.lid.is_empty() { 0 } else { lmin },
        lmax,
        opt_json(&cap.lid_error),
    );
    ok
}

fn wait_enter(msg: &str) {
    print!("{msg}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
}

/// Prompt through the segments; returns (label, start, end) in session seconds.
fn run_prompts(start: Instant, segments: &[Segment]) -> Vec<(String, f64, f64)> {
    let mut labels = Vec::new();
    for s in segments {
        println!("\n▶ [{}] {}（{}s）", s.label, s.prompt, s.secs);
        let t0 = start.elapsed().as_secs_f64();
        let deadline = Instant::now() + Duration::from_secs(s.secs);
        let stop_sound = Arc::new(AtomicBool::new(false));
        let player = s.play.then(|| {
            let stop = stop_sound.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = std::process::Command::new("afplay")
                        .args(["-v", "1.0", PLAYBACK_SOUND])
                        .status();
                }
            })
        });
        while Instant::now() < deadline {
            let left = deadline.saturating_duration_since(Instant::now()).as_secs() + 1;
            print!("  {left:2}\r");
            let _ = std::io::stdout().flush();
            std::thread::sleep(
                Duration::from_millis(200).min(deadline.saturating_duration_since(Instant::now())),
            );
        }
        stop_sound.store(true, Ordering::Relaxed);
        if let Some(p) = player {
            let _ = p.join();
        }
        labels.push((s.label.to_string(), t0, start.elapsed().as_secs_f64()));
    }
    labels
}

fn record(name: &str, description: &str, segments: &[Segment], out_dir: &Path) -> std::io::Result<PathBuf> {
    let total: u64 = segments.iter().map(|s| s.secs).sum();
    wait_enter(&format!(
        "\n=== {name}: {description}（約 {}s）— 準備好按 Enter 開始 ",
        total + 1
    ));
    let t0_unix_ms = iokit::now_unix_ms();
    let start = Instant::now();
    let done = Arc::new(AtomicBool::new(false));
    let d = done.clone();
    let segs: Vec<Segment> = segments
        .iter()
        .map(|s| Segment {
            label: s.label,
            secs: s.secs,
            prompt: s.prompt,
            play: s.play,
        })
        .collect();
    let prompts = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let labels = run_prompts(start, &segs);
        std::thread::sleep(Duration::from_millis(500));
        d.store(true, Ordering::Relaxed);
        labels
    });
    let cap = capture(start, done);
    let labels = prompts.join().unwrap_or_default();

    if let Some(e) = cap.accel_error.as_ref().or(cap.lid_error.as_ref()) {
        eprintln!("sensor error: {e}");
    }
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(format!("{name}.ndjson"));
    let mut w = BufWriter::new(std::fs::File::create(&path)?);
    writeln!(
        w,
        "{{\"format\":\"spank-trace\",\"version\":1,\"scenario\":{},\"model\":{},\"os\":{},\"t0_unix_ms\":{t0_unix_ms:.1},\"accel_interval_us\":{ACCEL_INTERVAL_US},\"lid_poll_hz\":{LID_POLL_HZ},\"accel_dropped\":{}}}",
        json_str(name),
        json_str(&sensors::hw_model()),
        json_str(&sensors::os_version()),
        cap.accel_dropped,
    )?;
    // Merge both streams by time.
    let (mut i, mut j) = (0, 0);
    while i < cap.accel.len() || j < cap.lid.len() {
        let take_accel = j >= cap.lid.len() || (i < cap.accel.len() && cap.accel[i].t <= cap.lid[j].0);
        if take_accel {
            let a = cap.accel[i];
            writeln!(w, "{{\"t\":{:.5},\"a\":[{:.5},{:.5},{:.5}]}}", a.t, a.x, a.y, a.z)?;
            i += 1;
        } else {
            let (t, deg) = cap.lid[j];
            writeln!(w, "{{\"t\":{t:.5},\"lid\":{deg}}}")?;
            j += 1;
        }
    }
    w.flush()?;

    let label_path = out_dir.join(format!("{name}.labels.json"));
    let items: Vec<String> = labels
        .iter()
        .map(|(l, s, e)| {
            format!(
                "    {{\"label\":{},\"start\":{s:.3},\"end\":{e:.3}}}",
                json_str(l)
            )
        })
        .collect();
    std::fs::write(
        &label_path,
        format!("{{\n  \"labels\": [\n{}\n  ]\n}}\n", items.join(",\n")),
    )?;
    println!(
        "\n✓ {} — accel {} samples, lid {} samples, {} labels",
        path.display(),
        cap.accel.len(),
        cap.lid.len(),
        labels.len()
    );
    Ok(path)
}

fn main() {
    let args = iokit::Args::new();
    if args.has("--check") {
        let ok = check(args.get("--secs", 3.0));
        std::process::exit(if ok { 0 } else { 1 });
    }
    let all = scenarios();
    if args.has("--list") || !args.has("--scenario") {
        eprintln!("usage: trace-recorder --check | --scenario <name|all> --out <dir>\nscenarios:");
        for (n, d, s) in &all {
            eprintln!("  {n:<9} {d} ({}s)", s.iter().map(|x| x.secs).sum::<u64>());
        }
        std::process::exit(2);
    }
    let which: String = args.get("--scenario", String::new());
    let out: PathBuf = args.get("--out", String::from("fixtures/traces/recorded")).into();
    let chosen: Vec<_> = all
        .iter()
        .filter(|(n, _, _)| which == "all" || *n == which)
        .collect();
    if chosen.is_empty() {
        eprintln!("unknown scenario {which:?} (try --list)");
        std::process::exit(2);
    }
    for (name, desc, segs) in chosen {
        if let Err(e) = record(name, desc, segs, &out) {
            eprintln!("failed to write {name}: {e}");
            std::process::exit(1);
        }
    }
}

unsafe extern "C" {
    fn getuid() -> u32;
}
