//! Orchestrator (design §4.3): the single place where sensor data, system signals and settings meet.
//!
//! ```text
//! lid sample ─► LidDetector ─┐
//! accel batch ─► ImpactExtractor ─► SlapClassifier (gates) ─┤
//!                                                           ▼
//!          event ─► paused? ─► muted? (AutoMute) ─► binding? ─► pick sound ─► Player::play
//! ```
//!
//! It is pure logic: the runtime feeds it inputs and executes the returned [`Output`]s (sensor
//! start/stop, UI notifications). Playback goes through the [`Player`] trait so tests can use a fake.
//!
//! Sensors run only while awake, unlocked and not paused (ADR-0013); the accelerometer also needs
//! slaps enabled. After a wake the lid is read once even if the screen is locked, so `LidOpened`
//! plays when the lid opens, not when the user unlocks.

use crate::config::{AppConfig, TimeOfDay};
use crate::event::{EventKind, EventMeta, Priority, TriggerEvent};
use crate::impact::{ImpactExtractor, ImpactParams};
use crate::lid::LidDetector;
use crate::mute::{AutoMute, MuteReason, MuteStatus};
use crate::profile::{EventBinding, Profile, SoundId, SoundPicker};
use crate::sample::{AccelSample, LidSample};
use crate::slap::{GateSignals, SlapClassifier, SlapOutcome, SuppressReason};
use crate::sys::SysSignal;
use serde::{Deserialize, Serialize};

/// What the orchestrator asks the player to do.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayRequest {
    pub kind: EventKind,
    pub sound: SoundId,
    pub priority: Priority,
    /// Profile volume × app volume (the OS applies the system volume).
    pub gain: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayOutcome {
    Started,
    /// Interrupted a lower-priority sound (50 ms fade-out).
    Preempted,
    /// Something of equal or higher priority is playing.
    Busy,
    /// Sound not loaded or no output device.
    Unavailable,
}

/// Playback state for the self-playback gate. Times share the sensor epoch.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlaybackStatus {
    pub active: bool,
    pub ended_at_ms: Option<u64>,
}

pub trait Player {
    fn play(&mut self, req: &PlayRequest) -> PlayOutcome;
    fn status(&self) -> PlaybackStatus;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SensorCommand {
    StartLid,
    StopLid,
    StartAccel,
    StopAccel,
}

/// What happened to a detected event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FireResult {
    Played {
        sound: SoundId,
    },
    Muted {
        reasons: Vec<MuteReason>,
    },
    Paused,
    /// The active profile has no enabled sound for this event.
    NoSound,
    /// A sound of equal or higher priority is playing (ADR-0008).
    Busy,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    Sensor(SensorCommand),
    Fired { event: TriggerEvent, result: FireResult },
    SlapSuppressed { at_ms: u64, reason: SuppressReason },
    MuteChanged(MuteStatus),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SensorState {
    lid: bool,
    accel: bool,
}

pub struct Orchestrator {
    cfg: AppConfig,
    profile: Profile,
    accel_rate_hz: f32,
    lid: LidDetector,
    impacts: ImpactExtractor,
    slaps: SlapClassifier,
    mute: AutoMute,
    picker: SoundPicker,
    sensors: SensorState,
    awaiting_wake: bool,
    last_input_ms: Option<u64>,
}

impl Orchestrator {
    /// `accel_rate_hz` is the accelerometer's nominal rate. Call [`Orchestrator::start`] next.
    pub fn new(cfg: AppConfig, profile: Profile, accel_rate_hz: f32, now: TimeOfDay) -> Self {
        Self {
            lid: LidDetector::new(cfg.lid.clone()),
            impacts: ImpactExtractor::new(ImpactParams::new(&cfg.slap, accel_rate_hz)),
            slaps: SlapClassifier::new(cfg.slap.clone()),
            mute: AutoMute::new(cfg.mute.clone(), now),
            picker: SoundPicker::new(),
            sensors: SensorState::default(),
            awaiting_wake: false,
            last_input_ms: None,
            accel_rate_hz,
            cfg,
            profile,
        }
    }

    /// Use a deterministic sound picker (tests).
    pub fn with_picker(mut self, picker: SoundPicker) -> Self {
        self.picker = picker;
        self
    }

    /// Initial sensor commands and mute status.
    pub fn start(&mut self) -> Vec<Output> {
        let mut out = vec![Output::MuteChanged(self.mute.status().clone())];
        self.sync_sensors(&mut out);
        out
    }

    pub fn config(&self) -> &AppConfig {
        &self.cfg
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    pub fn mute_status(&self) -> &MuteStatus {
        self.mute.status()
    }

    pub fn paused(&self) -> bool {
        self.cfg.paused
    }

    pub fn lid_angle(&self) -> Option<f32> {
        self.lid.angle()
    }

    pub fn lid_running(&self) -> bool {
        self.sensors.lid
    }

    pub fn accel_running(&self) -> bool {
        self.sensors.accel
    }

    // ---- inputs -------------------------------------------------------------------------

    pub fn on_lid(&mut self, s: LidSample, player: &mut impl Player) -> Vec<Output> {
        let mut out = Vec::new();
        if !self.sensors.lid {
            return out;
        }
        let event = if self.awaiting_wake {
            self.awaiting_wake = false;
            let e = self.lid.on_wake(s.angle_deg, s.at_ms);
            self.sync_sensors(&mut out);
            e
        } else {
            self.lid.on_sample(s.angle_deg, s.at_ms)
        };
        if let Some(e) = event {
            self.fire(e, player, &mut out);
        }
        out
    }

    pub fn on_accel(&mut self, batch: &[AccelSample], player: &mut impl Player) -> Vec<Output> {
        let mut out = Vec::new();
        if !self.sensors.accel {
            return out;
        }
        let mut candidates = Vec::new();
        self.impacts.extend(batch, &mut candidates);
        for c in candidates {
            let pb = player.status();
            let gates = GateSignals {
                lid_last_motion_ms: self.lid.last_motion_ms(),
                playback_active: pb.active,
                playback_ended_ms: pb.ended_at_ms,
                last_input_ms: self.last_input_ms,
            };
            match self.slaps.process(&c, &gates) {
                SlapOutcome::Fire(e) => self.fire(e, player, &mut out),
                SlapOutcome::Suppressed(reason) => out.push(Output::SlapSuppressed {
                    at_ms: c.at_ms,
                    reason,
                }),
            }
        }
        out
    }

    pub fn on_sys(&mut self, s: SysSignal, now: TimeOfDay) -> Vec<Output> {
        let mut out = Vec::new();
        match s {
            SysSignal::WillSleep => self.awaiting_wake = false,
            // A wake is reported once per sleep; ignore duplicates while awake.
            SysSignal::DidWake if self.mute.sys().asleep => self.awaiting_wake = !self.cfg.paused,
            _ => {}
        }
        if let Some(status) = self.mute.on_signal(s, now) {
            out.push(Output::MuteChanged(status));
        }
        self.sync_sensors(&mut out);
        out
    }

    /// Re-evaluate time-based mute rules (call at least every minute, or at the boundary from
    /// [`crate::mute::minutes_to_next_boundary`]).
    pub fn on_tick(&mut self, now: TimeOfDay) -> Vec<Output> {
        self.mute
            .refresh(now)
            .map(Output::MuteChanged)
            .into_iter()
            .collect()
    }

    /// Keyboard / trackpad activity (precise typing mode, T39).
    pub fn on_input_activity(&mut self, at_ms: u64) {
        self.last_input_ms = Some(at_ms);
    }

    pub fn set_paused(&mut self, paused: bool) -> Vec<Output> {
        let mut out = Vec::new();
        self.cfg.paused = paused;
        self.awaiting_wake = false;
        self.sync_sensors(&mut out);
        out
    }

    /// Apply new settings. Detectors whose settings changed start fresh.
    pub fn set_config(&mut self, cfg: AppConfig, now: TimeOfDay) -> Vec<Output> {
        let mut out = Vec::new();
        if cfg.lid != self.cfg.lid {
            self.lid.set_config(cfg.lid.clone());
        }
        if cfg.slap != self.cfg.slap {
            self.slaps.set_config(cfg.slap.clone());
            let params = ImpactParams::new(&cfg.slap, self.accel_rate_hz);
            if &params != self.impacts.params() {
                self.impacts = ImpactExtractor::new(params);
            }
        }
        if cfg.mute != self.cfg.mute
            && let Some(status) = self.mute.set_rules(cfg.mute.clone(), now)
        {
            out.push(Output::MuteChanged(status));
        }
        if cfg.paused != self.cfg.paused {
            self.awaiting_wake = false;
        }
        self.cfg = cfg;
        self.sync_sensors(&mut out);
        out
    }

    pub fn set_profile(&mut self, profile: Profile) {
        if profile.id != self.profile.id {
            self.picker.reset();
        }
        self.profile = profile;
    }

    /// Play the active profile's sound for `kind` right away (tray "preview"). Ignores pause and
    /// mute, because the user asked for it.
    pub fn preview(&mut self, kind: EventKind, at_ms: u64, player: &mut impl Player) -> FireResult {
        let event = TriggerEvent {
            kind,
            at_ms,
            meta: EventMeta::None,
        };
        self.play(&event, player)
    }

    // ---- internals ----------------------------------------------------------------------

    fn desired(&self) -> SensorState {
        let sys = self.mute.sys();
        let running = !self.cfg.paused && !sys.asleep;
        SensorState {
            lid: running && (!sys.locked || self.awaiting_wake),
            accel: running && !sys.locked && self.cfg.slap.enabled,
        }
    }

    fn sync_sensors(&mut self, out: &mut Vec<Output>) {
        let want = self.desired();
        if want.lid != self.sensors.lid {
            out.push(Output::Sensor(if want.lid {
                SensorCommand::StartLid
            } else {
                // The next sample after a restart only re-learns the zone.
                self.lid.on_sleep();
                SensorCommand::StopLid
            }));
        }
        if want.accel != self.sensors.accel {
            out.push(Output::Sensor(if want.accel {
                SensorCommand::StartAccel
            } else {
                self.impacts.reset();
                SensorCommand::StopAccel
            }));
        }
        self.sensors = want;
    }

    fn fire(&mut self, event: TriggerEvent, player: &mut impl Player, out: &mut Vec<Output>) {
        let result = if self.cfg.paused {
            FireResult::Paused
        } else if self.mute.status().muted() {
            FireResult::Muted {
                reasons: self.mute.status().reasons.clone(),
            }
        } else {
            self.play(&event, player)
        };
        out.push(Output::Fired { event, result });
    }

    /// The binding to use. A combo falls back to the slap's strength level when the profile has
    /// no combo sound.
    fn binding_for(&self, event: &TriggerEvent) -> Option<(EventKind, &EventBinding)> {
        if let Some(b) = self.profile.playable(event.kind) {
            return Some((event.kind, b));
        }
        if let (EventKind::SlapCombo, EventMeta::Combo { peak_g, .. }) = (event.kind, event.meta) {
            let level = self.slaps.level(peak_g);
            return self.profile.playable(level).map(|b| (level, b));
        }
        None
    }

    fn play(&mut self, event: &TriggerEvent, player: &mut impl Player) -> FireResult {
        let Some((kind, binding)) = self.binding_for(event) else {
            return FireResult::NoSound;
        };
        let binding = binding.clone();
        let Some(sound) = self.picker.pick(kind, &binding) else {
            return FireResult::NoSound;
        };
        let req = PlayRequest {
            kind: event.kind,
            sound: sound.clone(),
            // Priority follows the detected event, even when a combo borrows a level's sound.
            priority: event.kind.priority(),
            gain: (self.profile.volume * self.cfg.app_volume).clamp(0.0, 1.0),
        };
        match player.play(&req) {
            PlayOutcome::Started | PlayOutcome::Preempted => FireResult::Played { sound },
            PlayOutcome::Busy => FireResult::Busy,
            PlayOutcome::Unavailable => FireResult::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MuteRules, QuietHours};

    #[derive(Default)]
    struct FakePlayer {
        requests: Vec<PlayRequest>,
        status: PlaybackStatus,
        outcome: Option<PlayOutcome>,
    }

    impl Player for FakePlayer {
        fn play(&mut self, req: &PlayRequest) -> PlayOutcome {
            self.requests.push(req.clone());
            self.outcome.unwrap_or(PlayOutcome::Started)
        }
        fn status(&self) -> PlaybackStatus {
            self.status
        }
    }

    fn noon() -> TimeOfDay {
        "12:00".parse().unwrap()
    }

    fn profile() -> Profile {
        let mut p = Profile::new("p", "P");
        p.volume = 0.5;
        for (k, s) in [
            (EventKind::LidOpened, "open"),
            (EventKind::LidClosing, "close"),
            (EventKind::LidOverbent, "bend"),
            (EventKind::SlapLight, "light"),
            (EventKind::SlapHard, "hard"),
        ] {
            p.mapping.get_mut(&k).unwrap().sounds = vec![s.into()];
        }
        p
    }

    fn orch(cfg: AppConfig) -> (Orchestrator, Vec<Output>) {
        let mut o = Orchestrator::new(cfg, profile(), 800.0, noon()).with_picker(SoundPicker::with_seed(1));
        let out = o.start();
        (o, out)
    }

    fn sensors(out: &[Output]) -> Vec<SensorCommand> {
        out.iter()
            .filter_map(|o| match o {
                Output::Sensor(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    fn fired(out: &[Output]) -> Vec<(EventKind, FireResult)> {
        out.iter()
            .filter_map(|o| match o {
                Output::Fired { event, result } => Some((event.kind, result.clone())),
                _ => None,
            })
            .collect()
    }

    fn lid(o: &mut Orchestrator, p: &mut FakePlayer, samples: &[(f32, u64)]) -> Vec<Output> {
        samples
            .iter()
            .flat_map(|&(a, t)| {
                o.on_lid(
                    LidSample {
                        at_ms: t,
                        angle_deg: a,
                    },
                    p,
                )
            })
            .collect()
    }

    fn slap_cfg() -> AppConfig {
        let mut c = AppConfig::default();
        c.slap.enabled = true;
        c
    }

    /// Quiet accel samples from `t0` for `ms`, with a 30 Hz slap at `slap_at` (absolute ms).
    fn accel(t0: u64, ms: u64, slaps: &[(u64, f32)]) -> Vec<AccelSample> {
        (0..ms * 8 / 10)
            .map(|i| {
                let t = t0 as f64 + i as f64 * 1.25;
                let z: f64 = slaps
                    .iter()
                    .map(|&(at, amp)| {
                        let d = (t - at as f64) / 1000.0;
                        if d < 0.0 {
                            0.0
                        } else {
                            amp as f64 * (-d / 0.04).exp() * (2.0 * std::f64::consts::PI * 30.0 * d).sin()
                        }
                    })
                    .sum();
                AccelSample {
                    at_ms: t as u64,
                    x: 0.0,
                    y: 0.0,
                    z: z as f32 - 1.0,
                }
            })
            .collect()
    }

    #[test]
    fn start_runs_lid_only_until_slaps_enabled() {
        let (o, out) = orch(AppConfig::default());
        assert_eq!(sensors(&out), vec![SensorCommand::StartLid]);
        assert!(matches!(out[0], Output::MuteChanged(_)));
        assert!(o.lid_running() && !o.accel_running());

        let (o, out) = orch(slap_cfg());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StartLid, SensorCommand::StartAccel]
        );
        assert!(o.accel_running());
    }

    #[test]
    fn lid_closing_plays_with_gain_and_priority() {
        let (mut o, _) = orch(AppConfig::default());
        let mut p = FakePlayer::default();
        let out = lid(&mut o, &mut p, &[(100.0, 0), (50.0, 200), (15.0, 400)]);
        assert_eq!(
            fired(&out),
            vec![(
                EventKind::LidClosing,
                FireResult::Played {
                    sound: "close".into()
                }
            )]
        );
        let r = &p.requests[0];
        assert_eq!(r.priority, EventKind::LidClosing.priority());
        assert!((r.gain - 0.5 * 0.8).abs() < 1e-6);
        assert_eq!(o.lid_angle(), Some(15.0));
    }

    #[test]
    fn sleep_stops_sensors_and_wake_fires_opened_once() {
        let (mut o, _) = orch(slap_cfg());
        let mut p = FakePlayer::default();
        lid(&mut o, &mut p, &[(100.0, 0)]);
        let out = o.on_sys(SysSignal::WillSleep, noon());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StopLid, SensorCommand::StopAccel]
        );
        // Samples while stopped are ignored.
        assert!(lid(&mut o, &mut p, &[(5.0, 100)]).is_empty());

        let out = o.on_sys(SysSignal::DidWake, noon());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StartLid, SensorCommand::StartAccel]
        );
        let out = lid(&mut o, &mut p, &[(105.0, 10_000), (105.0, 10_200)]);
        assert_eq!(
            fired(&out),
            vec![(EventKind::LidOpened, FireResult::Played { sound: "open".into() })]
        );
        // Duplicate wake notifications don't re-arm.
        o.on_sys(SysSignal::DidWake, noon());
        assert!(fired(&lid(&mut o, &mut p, &[(105.0, 20_000)])).is_empty());
    }

    #[test]
    fn wake_while_locked_reads_lid_once() {
        let (mut o, _) = orch(slap_cfg());
        let mut p = FakePlayer::default();
        o.on_sys(SysSignal::ScreenLocked, noon());
        o.on_sys(SysSignal::WillSleep, noon());
        let out = o.on_sys(SysSignal::DidWake, noon());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StartLid],
            "accel stays off while locked"
        );
        let out = lid(&mut o, &mut p, &[(100.0, 5000)]);
        assert_eq!(fired(&out).len(), 1);
        assert_eq!(sensors(&out), vec![SensorCommand::StopLid]);
        let out = o.on_sys(SysSignal::ScreenUnlocked, noon());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StartLid, SensorCommand::StartAccel]
        );
    }

    #[test]
    fn lock_and_pause_stop_sensors() {
        let (mut o, _) = orch(slap_cfg());
        let out = o.on_sys(SysSignal::ScreenLocked, noon());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StopLid, SensorCommand::StopAccel]
        );
        o.on_sys(SysSignal::ScreenUnlocked, noon());

        let out = o.set_paused(true);
        assert!(o.paused());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StopLid, SensorCommand::StopAccel]
        );
        // A wake while paused does not queue a LidOpened for later.
        o.on_sys(SysSignal::WillSleep, noon());
        o.on_sys(SysSignal::DidWake, noon());
        let out = o.set_paused(false);
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StartLid, SensorCommand::StartAccel]
        );
        let mut p = FakePlayer::default();
        assert!(fired(&lid(&mut o, &mut p, &[(100.0, 1000)])).is_empty());
    }

    #[test]
    fn muted_events_are_reported_not_played() {
        let (mut o, _) = orch(AppConfig::default());
        let mut p = FakePlayer::default();
        let out = o.on_sys(SysSignal::MicInUse { in_use: true }, noon());
        assert!(matches!(&out[..], [Output::MuteChanged(s)] if s.muted()));
        let out = lid(&mut o, &mut p, &[(100.0, 0), (15.0, 400)]);
        assert_eq!(
            fired(&out),
            vec![(
                EventKind::LidClosing,
                FireResult::Muted {
                    reasons: vec![MuteReason::Meeting]
                }
            )]
        );
        assert!(p.requests.is_empty());
        assert!(o.mute_status().muted());
    }

    #[test]
    fn quiet_hours_follow_the_clock() {
        let mut cfg = AppConfig::default();
        cfg.mute.quiet_hours = vec![QuietHours {
            start: "13:00".parse().unwrap(),
            end: "14:00".parse().unwrap(),
        }];
        let (mut o, _) = orch(cfg);
        assert!(o.on_tick("12:59".parse().unwrap()).is_empty());
        assert!(matches!(&o.on_tick("13:00".parse().unwrap())[..], [Output::MuteChanged(s)] if s.muted()));
        assert!(matches!(&o.on_tick("14:00".parse().unwrap())[..], [Output::MuteChanged(s)] if !s.muted()));
    }

    #[test]
    fn missing_binding_and_busy_player() {
        let (mut o, _) = orch(AppConfig::default());
        let mut p = FakePlayer::default();
        let mut prof = profile();
        prof.mapping.get_mut(&EventKind::LidClosing).unwrap().enabled = false;
        o.set_profile(prof);
        let out = lid(&mut o, &mut p, &[(100.0, 0), (15.0, 400)]);
        assert_eq!(fired(&out), vec![(EventKind::LidClosing, FireResult::NoSound)]);

        p.outcome = Some(PlayOutcome::Busy);
        let out = lid(&mut o, &mut p, &[(125.0, 1000)]);
        assert_eq!(fired(&out), vec![(EventKind::LidOverbent, FireResult::Busy)]);
        p.outcome = Some(PlayOutcome::Unavailable);
        assert_eq!(
            o.preview(EventKind::LidOpened, 0, &mut p),
            FireResult::Unavailable
        );
    }

    #[test]
    fn preview_ignores_mute_and_pause() {
        let (mut o, _) = orch(AppConfig::default());
        let mut p = FakePlayer::default();
        o.on_sys(SysSignal::MicInUse { in_use: true }, noon());
        o.set_paused(true);
        assert_eq!(
            o.preview(EventKind::SlapHard, 0, &mut p),
            FireResult::Played { sound: "hard".into() }
        );
        assert_eq!(o.preview(EventKind::SlapMedium, 0, &mut p), FireResult::NoSound);
    }

    #[test]
    fn slap_pipeline_fires_and_falls_back_for_combo() {
        let (mut o, _) = orch(slap_cfg());
        let mut p = FakePlayer::default();
        // Warm-up second, then three slaps 400 ms apart (third is a combo).
        let samples = accel(0, 3000, &[(1500, 0.25), (1900, 0.25), (2300, 0.25)]);
        let out = o.on_accel(&samples, &mut p);
        let kinds: Vec<_> = fired(&out).into_iter().map(|(k, _)| k).collect();
        assert_eq!(
            kinds,
            vec![EventKind::SlapLight, EventKind::SlapLight, EventKind::SlapCombo]
        );
        // No combo sound in the profile: the combo borrows the light slap's sound but keeps its priority.
        let last = p.requests.last().unwrap();
        assert_eq!(last.sound, SoundId::from("light"));
        assert_eq!(last.priority, EventKind::SlapCombo.priority());
    }

    #[test]
    fn slap_gates_report_suppression() {
        let (mut o, _) = orch(slap_cfg());
        let mut p = FakePlayer {
            status: PlaybackStatus {
                active: true,
                ended_at_ms: None,
            },
            ..Default::default()
        };
        let out = o.on_accel(&accel(0, 2500, &[(1500, 0.5)]), &mut p);
        assert!(
            matches!(
                &out[..],
                [Output::SlapSuppressed {
                    at_ms: 1500..=1510,
                    reason: SuppressReason::SelfPlayback
                }]
            ),
            "{out:?}"
        );
    }

    #[test]
    fn accel_ignored_when_disabled() {
        let (mut o, _) = orch(AppConfig::default());
        let mut p = FakePlayer::default();
        assert!(o.on_accel(&accel(0, 2500, &[(1500, 0.5)]), &mut p).is_empty());
    }

    #[test]
    fn set_config_toggles_accel_and_rules() {
        let (mut o, _) = orch(AppConfig::default());
        let mut cfg = slap_cfg();
        cfg.mute = MuteRules {
            quiet_hours: vec![QuietHours {
                start: "11:00".parse().unwrap(),
                end: "13:00".parse().unwrap(),
            }],
            ..MuteRules::default()
        };
        cfg.lid.close_threshold_deg = 25.0;
        let out = o.set_config(cfg.clone(), noon());
        assert_eq!(sensors(&out), vec![SensorCommand::StartAccel]);
        assert!(
            out.iter()
                .any(|x| matches!(x, Output::MuteChanged(s) if s.muted()))
        );
        assert_eq!(o.config().lid.close_threshold_deg, 25.0);

        cfg.slap.enabled = false;
        cfg.paused = true;
        let out = o.set_config(cfg, noon());
        assert_eq!(
            sensors(&out),
            vec![SensorCommand::StopLid, SensorCommand::StopAccel]
        );
        let mut p = FakePlayer::default();
        o.on_input_activity(5);
        assert_eq!(o.profile().id.0, "p");
        assert!(o.on_accel(&[], &mut p).is_empty());
    }
}
