//! Playback on the default output device (ADR-0008, spike S5).
//!
//! The cpal stream is built once at startup and kept **paused**: an always-running stream costs
//! coreaudiod ~12 % CPU, and the first open in a process can take ~1.9 s. A trigger resumes it
//! (`play()`, 20–40 ms) and the stream is paused again [`IDLE_PAUSE_MS`] after the sound ends.
//!
//! Threads: the caller's thread → (mpsc) → a control thread that owns the `cpal::Stream` →
//! (lock-free ring) → the audio callback that owns the [`Mixer`]. A stream error (e.g. the
//! default device changed) rebuilds the stream on the next request.

use crate::bank::SoundBank;
use crate::mixer::{Clip, MixOutcome, Mixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use spank_core::{PlayOutcome, PlayRequest, PlaybackStatus, Player, Priority};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Pause the stream this long after the last sound ended.
pub const IDLE_PAUSE_MS: u64 = 500;
const NONE: u64 = u64::MAX;

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("no audio output device")]
    NoDevice,
    #[error("audio output: {0}")]
    Stream(String),
}

/// What the audio callback shares with everyone else. Times are ms since the player's epoch.
#[derive(Debug)]
struct Status {
    epoch: Instant,
    playing: AtomicBool,
    /// Requests sent but not yet seen by the callback.
    pending: AtomicU32,
    priority: AtomicU32,
    ended_at_ms: AtomicU64,
    requested_at_us: AtomicU64,
    /// Delay from the last request to the callback that started it.
    last_start_delay_us: AtomicU64,
    stream_failed: AtomicBool,
}

impl Status {
    fn now_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }
}

enum Ctl {
    Play {
        clip: Arc<Clip>,
        priority: Priority,
        gain: f32,
    },
    Shutdown,
}

struct Cmd {
    clip: Arc<Clip>,
    priority: Priority,
    gain: f32,
}

/// Output format of the current stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    pub channels: u16,
    pub sample_rate: u32,
}

pub struct AudioPlayer {
    tx: mpsc::Sender<Ctl>,
    thread: Option<JoinHandle<()>>,
    status: Arc<Status>,
    format: OutputFormat,
    pub bank: SoundBank,
}

impl AudioPlayer {
    /// Open the default output device and keep a paused stream ready. `epoch` must be the same
    /// clock the sensors use, so `ended_at_ms` lines up with sample timestamps.
    pub fn new(epoch: Instant) -> Result<Self, PlayerError> {
        let status = Arc::new(Status {
            epoch,
            playing: AtomicBool::new(false),
            pending: AtomicU32::new(0),
            priority: AtomicU32::new(0),
            ended_at_ms: AtomicU64::new(NONE),
            requested_at_us: AtomicU64::new(0),
            last_start_delay_us: AtomicU64::new(NONE),
            stream_failed: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let st = status.clone();
        let thread = std::thread::Builder::new()
            .name("spank-audio".into())
            .spawn(move || control_thread(rx, st, ready_tx))
            .map_err(|e| PlayerError::Stream(e.to_string()))?;
        let format = ready_rx
            .recv()
            .map_err(|_| PlayerError::Stream("audio thread exited".into()))??;
        Ok(Self {
            tx,
            thread: Some(thread),
            status,
            format,
            bank: SoundBank::default(),
        })
    }

    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Play a clip directly (previews, tests).
    pub fn play_clip(&self, clip: Arc<Clip>, priority: Priority, gain: f32) -> PlayOutcome {
        let st = &self.status;
        if st.playing.load(Ordering::Acquire) || st.pending.load(Ordering::Acquire) > 0 {
            let cur = Priority(st.priority.load(Ordering::Acquire) as u8);
            if !priority.preempts(cur) {
                return PlayOutcome::Busy;
            }
        }
        let preempt = st.playing.load(Ordering::Acquire);
        st.pending.fetch_add(1, Ordering::AcqRel);
        st.priority.store(priority.0 as u32, Ordering::Release);
        st.requested_at_us.store(st.now_us(), Ordering::Release);
        if self.tx.send(Ctl::Play { clip, priority, gain }).is_err() {
            st.pending.fetch_sub(1, Ordering::AcqRel);
            return PlayOutcome::Unavailable;
        }
        if preempt {
            PlayOutcome::Preempted
        } else {
            PlayOutcome::Started
        }
    }

    /// Delay between the last request and the audio callback that started it.
    pub fn last_start_delay(&self) -> Option<Duration> {
        match self.status.last_start_delay_us.load(Ordering::Acquire) {
            NONE => None,
            us => Some(Duration::from_micros(us)),
        }
    }

    pub fn is_playing(&self) -> bool {
        self.status.playing.load(Ordering::Acquire) || self.status.pending.load(Ordering::Acquire) > 0
    }
}

impl Player for AudioPlayer {
    fn play(&mut self, req: &PlayRequest) -> PlayOutcome {
        match self.bank.get(&req.sound) {
            Some(clip) => self.play_clip(clip, req.priority, req.gain),
            None => PlayOutcome::Unavailable,
        }
    }

    fn status(&self) -> PlaybackStatus {
        PlaybackStatus {
            active: self.is_playing(),
            ended_at_ms: match self.status.ended_at_ms.load(Ordering::Acquire) {
                NONE => None,
                ms => Some(ms),
            },
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.tx.send(Ctl::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Output {
    stream: cpal::Stream,
    cmds: rtrb::Producer<Cmd>,
    format: OutputFormat,
}

fn build(status: &Arc<Status>) -> Result<Output, PlayerError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(PlayerError::NoDevice)?;
    let config: cpal::StreamConfig = device
        .default_output_config()
        .map_err(|e| PlayerError::Stream(e.to_string()))?
        .into();
    let format = OutputFormat {
        channels: config.channels,
        sample_rate: config.sample_rate,
    };
    let (cmds, mut rx) = rtrb::RingBuffer::<Cmd>::new(16);
    let mut mixer = Mixer::new(format.channels, format.sample_rate);
    let st = status.clone();
    let st_err = status.clone();
    let stream = device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| {
                // Real-time: no locks, no allocation (dropping a finished clip's Arc may free).
                while let Ok(c) = rx.pop() {
                    if mixer.request(c.clip, c.priority, c.gain) != MixOutcome::Busy {
                        let req = st.requested_at_us.load(Ordering::Acquire);
                        st.last_start_delay_us
                            .store(st.now_us().saturating_sub(req), Ordering::Release);
                    }
                    st.playing.store(true, Ordering::Release);
                    st.pending.fetch_sub(1, Ordering::AcqRel);
                }
                mixer.render(data);
                let active = mixer.is_active();
                if st.playing.load(Ordering::Acquire) && !active {
                    st.ended_at_ms
                        .store(st.epoch.elapsed().as_millis() as u64, Ordering::Release);
                }
                st.playing.store(active, Ordering::Release);
            },
            move |_e| st_err.stream_failed.store(true, Ordering::Release),
            None,
        )
        .map_err(|e| PlayerError::Stream(e.to_string()))?;
    // Some backends start immediately; we want it paused until the first sound.
    let _ = stream.pause();
    Ok(Output { stream, cmds, format })
}

fn control_thread(
    rx: mpsc::Receiver<Ctl>,
    status: Arc<Status>,
    ready: mpsc::SyncSender<Result<OutputFormat, PlayerError>>,
) {
    let mut out = match build(&status) {
        Ok(o) => {
            let _ = ready.send(Ok(o.format));
            Some(o)
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mut running = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ctl::Play { clip, priority, gain }) => {
                if status.stream_failed.swap(false, Ordering::AcqRel) || out.is_none() {
                    drop(out.take());
                    running = false;
                    out = build(&status).ok();
                }
                let Some(o) = out.as_mut() else {
                    status.pending.fetch_sub(1, Ordering::AcqRel);
                    continue;
                };
                if o.cmds.push(Cmd { clip, priority, gain }).is_err() {
                    status.pending.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                if !running && o.stream.play().is_ok() {
                    running = true;
                }
            }
            Ok(Ctl::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if running && !status.playing.load(Ordering::Acquire) && status.pending.load(Ordering::Acquire) == 0 {
            let ended = status.ended_at_ms.load(Ordering::Acquire);
            let now = status.epoch.elapsed().as_millis() as u64;
            if ended != NONE && now.saturating_sub(ended) >= IDLE_PAUSE_MS {
                if let Some(o) = &out {
                    let _ = o.stream.pause();
                }
                running = false;
            }
        }
    }
}
