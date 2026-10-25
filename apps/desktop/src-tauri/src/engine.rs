//! The app runtime (T30, design §2.1): one core thread owns the orchestrator, the sensors and the
//! audio player.
//!
//! - Lid: polled on this thread following `LidPollSchedule` (5 Hz idle, 30 Hz while moving).
//! - Accelerometer: `IoHidAccel` runs its own CFRunLoop thread; we drain its ring buffer every
//!   ~50 ms (ADR-0013) and feed the batch to the orchestrator.
//! - Sensors start and stop only when the orchestrator says so (sleep, lock, pause, slaps off).
//! - Precise typing (T39): while the accelerometer runs and the setting is on, an `InputMonitor`
//!   reports keyboard / click times to the orchestrator's typing gate. It needs Input Monitoring;
//!   without it nothing starts and detection keeps its signal-based typing filter.
//! - Calibration (T36) and the live waveform: the orchestrator reports `Calibration` progress and
//!   per-batch `AccelPeak`s, which leave as events.
//! - System signals, settings and previews arrive as [`EngineCmd`]s; state changes and fired events
//!   leave through the sink as [`EngineEvent`]s.
//!
//! The sensors and the player share one `Instant` epoch, so playback end times line up with sample
//! timestamps for the self-playback gate.

use spank_audio::{AudioPlayer, SoundBank};
use spank_core::{
    AppConfig, CalStep, Calibration, CalibrationProgress, Capability, EventKind, FireResult, FiredEvent,
    FocusAuthorization, InputAccess, Orchestrator, Output, PlayOutcome, PlayRequest, PlaybackStatus, Player,
    Priority, Profile, RuntimeState, SensorCommand, SlapConfig, SoundId, SysSignal, TimeOfDay,
};
use spank_sensors::{ACCEL_NOMINAL_HZ, AccelSource, InputMonitor, IoHidAccel, IoHidLid, LidPollSchedule};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How often the accelerometer ring buffer is drained.
const ACCEL_BATCH: Duration = Duration::from_millis(50);
/// Quiet hours are re-checked this often.
const MUTE_TICK: Duration = Duration::from_secs(20);
/// Previews beat every event (combo is 7).
const PREVIEW_PRIORITY: Priority = Priority(u8::MAX);
/// While precise typing waits for Input Monitoring, re-check the permission this often.
const INPUT_ACCESS_RECHECK: Duration = Duration::from_secs(5);

pub type CalibrationReply = mpsc::Sender<Result<(Calibration, SlapConfig), String>>;

/// Where a sound's audio comes from.
pub enum SoundData {
    /// Built-in sounds, kept in memory from the bundled packs.
    Bytes(Arc<[u8]>),
    /// User sounds in the store.
    File(PathBuf),
}

pub type Resolver = Arc<dyn Fn(&SoundId) -> Option<SoundData> + Send + Sync>;

pub enum EngineCmd {
    Sys(SysSignal),
    Config(AppConfig),
    Profile(Profile),
    PreviewEvent(EventKind),
    PreviewSound(SoundId),
    StopPreview,
    FocusAuthorization(FocusAuthorization),
    StartCalibration,
    CalibrationStep {
        step: CalStep,
        reset: bool,
    },
    /// Ends the wizard; replies with the result (or why there is none).
    FinishCalibration(CalibrationReply),
    CancelCalibration,
    /// Live waveform for the settings window.
    Monitor(bool),
    /// Input Monitoring may have changed (e.g. right after asking).
    CheckInputAccess,
    Shutdown,
}

pub enum EngineEvent {
    State(RuntimeState),
    Fired(FiredEvent),
    Calibration(CalibrationProgress),
    /// Largest |a| (g, gravity removed) of the last ~50 ms.
    AccelLevel(f32),
}

pub struct Engine {
    tx: mpsc::Sender<EngineCmd>,
    state: Arc<Mutex<RuntimeState>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Engine {
    pub fn start(
        config: AppConfig,
        profile: Profile,
        resolver: Resolver,
        sink: impl Fn(EngineEvent) + Send + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(RuntimeState {
            paused: config.paused,
            active_profile: profile.id.clone(),
            ..Default::default()
        }));
        let st = state.clone();
        let thread = std::thread::Builder::new()
            .name("spank-core".into())
            .spawn(move || Core::new(config, profile, resolver, st, Box::new(sink)).run(rx))
            .ok();
        Self {
            tx,
            state,
            thread: Mutex::new(thread),
        }
    }

    pub fn send(&self, cmd: EngineCmd) {
        let _ = self.tx.send(cmd);
    }

    /// A sender for other threads (e.g. the system monitor).
    pub fn sender(&self) -> mpsc::Sender<EngineCmd> {
        self.tx.clone()
    }

    pub fn state(&self) -> RuntimeState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Stop sensors (restoring accelerometer properties) and join the core thread.
    pub fn shutdown(&self) {
        let _ = self.tx.send(EngineCmd::Shutdown);
        if let Some(t) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = t.join();
        }
    }
}

/// The audio output, or nothing when no device could be opened.
enum Audio {
    Real(AudioPlayer),
    None,
}

impl Player for Audio {
    fn play(&mut self, req: &PlayRequest) -> PlayOutcome {
        match self {
            Audio::Real(p) => p.play(req),
            Audio::None => PlayOutcome::Unavailable,
        }
    }

    fn status(&self) -> PlaybackStatus {
        match self {
            Audio::Real(p) => p.status(),
            Audio::None => PlaybackStatus::default(),
        }
    }

    fn sound_ms(&self, sound: &SoundId) -> Option<u64> {
        match self {
            Audio::Real(p) => p.sound_ms(sound),
            Audio::None => None,
        }
    }
}

impl Audio {
    fn bank(&mut self) -> Option<&mut SoundBank> {
        match self {
            Audio::Real(p) => Some(&mut p.bank),
            Audio::None => None,
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Local wall-clock time of day (quiet hours are in local time).
pub fn local_time_of_day() -> TimeOfDay {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    // SAFETY: localtime_r writes into the provided struct and is thread-safe.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        tm
    };
    TimeOfDay::new(tm.tm_hour.clamp(0, 23) as u8, tm.tm_min.clamp(0, 59) as u8)
        .unwrap_or(TimeOfDay { hour: 0, minute: 0 })
}

struct Core {
    epoch: Instant,
    orch: Orchestrator,
    audio: Audio,
    resolver: Resolver,
    lid: Option<IoHidLid>,
    lid_sched: LidPollSchedule,
    next_lid_poll: Instant,
    accel: Option<IoHidAccel>,
    input: Option<InputMonitor>,
    next_access_check: Instant,
    batch: Vec<spank_core::AccelSample>,
    last_mute_tick: Instant,
    state: RuntimeState,
    shared: Arc<Mutex<RuntimeState>>,
    published: Option<RuntimeState>,
    sink: Box<dyn Fn(EngineEvent) + Send>,
}

impl Core {
    fn new(
        config: AppConfig,
        profile: Profile,
        resolver: Resolver,
        shared: Arc<Mutex<RuntimeState>>,
        sink: Box<dyn Fn(EngineEvent) + Send>,
    ) -> Self {
        let epoch = Instant::now();
        // A crashed run may have left reporting on (ADR-0014). The sandbox (App Store edition)
        // forbids the property writes, and that edition never turns the accelerometer on.
        if !crate::edition::APP_STORE {
            spank_sensors::reset_accel_properties();
        }
        let audio = match AudioPlayer::new(epoch) {
            Ok(p) => Audio::Real(p),
            Err(e) => {
                eprintln!("spank: audio output unavailable: {e}");
                Audio::None
            }
        };
        let state = RuntimeState {
            paused: config.paused,
            active_profile: profile.id.clone(),
            audio_ok: matches!(audio, Audio::Real(_)),
            input_access: spank_sensors::input_access(),
            ..shared.lock().map(|s| s.clone()).unwrap_or_default()
        };
        let orch = Orchestrator::new(config, profile, ACCEL_NOMINAL_HZ, local_time_of_day());
        Self {
            epoch,
            orch,
            audio,
            resolver,
            lid: None,
            lid_sched: LidPollSchedule::new(),
            next_lid_poll: Instant::now(),
            accel: None,
            input: None,
            next_access_check: Instant::now() + INPUT_ACCESS_RECHECK,
            batch: Vec::with_capacity(256),
            last_mute_tick: Instant::now(),
            state,
            shared,
            published: None,
            sink,
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn run(mut self, rx: mpsc::Receiver<EngineCmd>) {
        self.probe_accel();
        self.sync_bank();
        let out = self.orch.start();
        self.handle(out);
        self.publish();

        loop {
            let timeout = self.next_wakeup();
            match rx.recv_timeout(timeout) {
                Ok(EngineCmd::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.command(cmd),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            self.poll_sensors();
            if Instant::now() >= self.next_access_check {
                self.next_access_check = Instant::now() + INPUT_ACCESS_RECHECK;
                if self.orch.config().slap.precise_typing && self.state.input_access != InputAccess::Granted {
                    self.state.input_access = spank_sensors::input_access();
                    self.sync_input();
                }
            }
            if self.last_mute_tick.elapsed() >= MUTE_TICK {
                self.last_mute_tick = Instant::now();
                let out = self.orch.on_tick(local_time_of_day());
                self.handle(out);
            }
            self.publish();
        }
        if let Some(i) = self.input.take() {
            i.stop();
        }
        if let Some(a) = self.accel.take() {
            a.stop();
        }
        self.lid = None;
    }

    fn next_wakeup(&self) -> Duration {
        let mut t = Duration::from_secs(1);
        if self.accel.is_some() {
            t = t.min(ACCEL_BATCH);
        }
        if self.lid.is_some() {
            t = t.min(self.next_lid_poll.saturating_duration_since(Instant::now()));
        }
        t
    }

    /// One-time capability check: switch the accelerometer on until the first report.
    fn probe_accel(&mut self) {
        self.state.caps.accel = if crate::edition::APP_STORE {
            Capability::Unsupported {
                reason: crate::edition::unavailable(),
            }
        } else if !spank_sensors::accel_driver_present() {
            Capability::Unsupported {
                reason: "no accelerometer driver".into(),
            }
        } else {
            match IoHidAccel::probe(self.epoch, Duration::from_millis(500)) {
                Ok(a) => {
                    a.stop();
                    Capability::Available
                }
                Err(e) => Capability::Unsupported {
                    reason: e.to_string(),
                },
            }
        };
    }

    fn command(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::Sys(s) => {
                let out = self.orch.on_sys(s, local_time_of_day());
                self.handle(out);
            }
            EngineCmd::Config(cfg) => {
                let out = self.orch.set_config(cfg, local_time_of_day());
                self.handle(out);
            }
            EngineCmd::Profile(p) => {
                let out = self.orch.set_profile(p);
                self.handle(out);
                self.sync_bank();
            }
            EngineCmd::PreviewEvent(kind) => {
                let at = self.now_ms();
                let result = self.orch.preview(kind, at, &mut self.audio);
                (self.sink)(EngineEvent::Fired(FiredEvent {
                    event: spank_core::TriggerEvent {
                        kind,
                        at_ms: at,
                        meta: spank_core::EventMeta::None,
                    },
                    result,
                }));
            }
            EngineCmd::PreviewSound(id) => self.preview_sound(&id),
            EngineCmd::StopPreview => {
                if let Audio::Real(p) = &self.audio {
                    p.stop();
                }
            }
            EngineCmd::FocusAuthorization(a) => self.state.focus_authorization = a,
            EngineCmd::StartCalibration => {
                let out = self.orch.start_calibration(self.now_ms());
                self.handle(out);
            }
            EngineCmd::CalibrationStep { step, reset } => {
                let out = self.orch.calibration_step(step, reset, self.now_ms());
                self.handle(out);
            }
            EngineCmd::FinishCalibration(reply) => {
                let r = match self.orch.finish_calibration(unix_ms()) {
                    Ok((cal, slap, out)) => {
                        self.handle(out);
                        Ok((cal, slap))
                    }
                    Err(e) => Err(e.to_string()),
                };
                let _ = reply.send(r);
            }
            EngineCmd::CancelCalibration => {
                let out = self.orch.cancel_calibration();
                self.handle(out);
            }
            EngineCmd::Monitor(on) => self.orch.set_monitor(on),
            EngineCmd::CheckInputAccess => {
                self.state.input_access = spank_sensors::input_access();
                self.sync_input();
            }
            EngineCmd::Shutdown => {}
        }
    }

    fn load(&mut self, id: &SoundId) -> bool {
        let data = (self.resolver)(id);
        let Some(bank) = self.audio.bank() else {
            return false;
        };
        if bank.contains(id) {
            return true;
        }
        let r = match data {
            Some(SoundData::Bytes(b)) => bank.load_wav_bytes(id.clone(), &b),
            Some(SoundData::File(p)) => bank.load_wav(id.clone(), &p),
            None => return false,
        };
        if let Err(e) = &r {
            eprintln!("spank: could not load sound: {e}");
        }
        r.is_ok()
    }

    /// Preload the active profile's sounds and drop everything else.
    fn sync_bank(&mut self) {
        let wanted: Vec<SoundId> = self
            .orch
            .profile()
            .referenced_sounds()
            .into_iter()
            .cloned()
            .collect();
        if let Some(bank) = self.audio.bank() {
            bank.retain(&wanted.iter().collect::<Vec<_>>());
        }
        for id in &wanted {
            self.load(id);
        }
        self.state.active_profile = self.orch.profile().id.clone();
    }

    fn preview_sound(&mut self, id: &SoundId) {
        if !self.load(id) {
            return;
        }
        let gain = self.orch.config().app_volume;
        if let Audio::Real(p) = &self.audio
            && let Some(clip) = p.bank.get(id)
        {
            p.stop();
            p.play_clip(clip, PREVIEW_PRIORITY, gain);
        }
    }

    /// Watch keyboard / trackpad activity only while it can matter: precise typing on, slaps
    /// being detected, and Input Monitoring granted.
    fn sync_input(&mut self) {
        let want = !crate::edition::APP_STORE
            && self.orch.config().slap.precise_typing
            && self.accel.is_some()
            && !self.orch.calibrating()
            && self.state.input_access == InputAccess::Granted;
        if want && self.input.is_none() {
            match InputMonitor::start(self.epoch) {
                Ok(m) => self.input = Some(m),
                Err(e) => {
                    eprintln!("spank: input monitor failed to start: {e}");
                    self.state.input_access = spank_sensors::input_access();
                }
            }
        } else if !want && let Some(m) = self.input.take() {
            m.stop();
        }
        self.state.input_running = self.input.is_some();
    }

    fn poll_sensors(&mut self) {
        if let Some(t) = self.input.as_ref().and_then(|m| m.last_input_ms()) {
            self.orch.on_input_activity(t);
        }
        if let Some(a) = self.accel.as_mut() {
            self.batch.clear();
            a.drain(&mut self.batch);
            if !self.batch.is_empty() {
                let batch = std::mem::take(&mut self.batch);
                let out = self.orch.on_accel(&batch, &mut self.audio);
                self.batch = batch;
                self.handle(out);
            }
        }
        if self.lid.is_some() && Instant::now() >= self.next_lid_poll {
            let read = self.lid.as_ref().map(|l| l.read());
            match read {
                Some(Ok(s)) => {
                    let wait = self.lid_sched.on_sample(s.angle_deg, s.at_ms);
                    self.next_lid_poll = Instant::now() + Duration::from_millis(wait);
                    self.state.lid_angle = Some(s.angle_deg);
                    let out = self.orch.on_lid(s, &mut self.audio);
                    self.handle(out);
                }
                Some(Err(e)) => {
                    eprintln!("spank: lid read failed: {e}");
                    self.next_lid_poll = Instant::now() + Duration::from_secs(1);
                }
                None => {}
            }
        }
    }

    fn handle(&mut self, outputs: Vec<Output>) {
        for o in outputs {
            match o {
                Output::Sensor(c) => self.sensor(c),
                Output::Fired { event, result } => {
                    if let FireResult::Unavailable = result {
                        eprintln!("spank: {:?} fired but its sound is unavailable", event.kind);
                    }
                    (self.sink)(EngineEvent::Fired(FiredEvent { event, result }));
                }
                Output::MuteChanged(m) => self.state.mute = m,
                Output::StopSound(priority) => {
                    if let Audio::Real(p) = &self.audio {
                        p.stop_priority(priority);
                    }
                }
                Output::SlapSuppressed { .. } => {}
                Output::Calibration(p) => (self.sink)(EngineEvent::Calibration(p)),
                Output::AccelPeak { peak_g, .. } => (self.sink)(EngineEvent::AccelLevel(peak_g)),
            }
        }
        self.state.paused = self.orch.paused();
        self.state.lid_running = self.lid.is_some();
        self.state.accel_running = self.accel.is_some();
        self.state.calibrating = self.orch.calibrating();
        self.sync_input();
    }

    fn sensor(&mut self, c: SensorCommand) {
        match c {
            SensorCommand::StartLid => match IoHidLid::open(self.epoch) {
                Ok(l) => {
                    self.lid = Some(l);
                    self.lid_sched.reset();
                    self.next_lid_poll = Instant::now();
                    self.state.caps.lid = Capability::Available;
                }
                Err(e) => {
                    self.state.caps.lid = Capability::Unsupported {
                        reason: e.to_string(),
                    };
                }
            },
            SensorCommand::StopLid => {
                self.lid = None;
                self.state.lid_angle = None;
            }
            // Never asked for in the App Store edition (slaps and calibration are off there).
            SensorCommand::StartAccel if crate::edition::APP_STORE => {}
            SensorCommand::StartAccel => match IoHidAccel::start(self.epoch) {
                Ok(a) => self.accel = Some(a),
                Err(e) => {
                    eprintln!("spank: accelerometer failed to start: {e}");
                    self.state.caps.accel = Capability::Unsupported {
                        reason: e.to_string(),
                    };
                }
            },
            SensorCommand::StopAccel => {
                if let Some(a) = self.accel.take() {
                    a.stop();
                }
            }
        }
    }

    fn publish(&mut self) {
        if self.published.as_ref() == Some(&self.state) {
            return;
        }
        if let Ok(mut s) = self.shared.lock() {
            *s = self.state.clone();
        }
        self.published = Some(self.state.clone());
        (self.sink)(EngineEvent::State(self.state.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_is_valid() {
        let t = local_time_of_day();
        assert!(t.hour < 24 && t.minute < 60);
    }
}
