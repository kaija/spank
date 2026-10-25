//! Lid angle event state machine (ADR-0005, design §4.1).
//!
//! Feed it every polled angle with [`LidDetector::on_sample`], and call [`LidDetector::on_wake`] /
//! [`LidDetector::on_sleep`] on system power events. It emits at most one event per call.
//!
//! `LidOverbent` is not a one-shot event: [`LidDetector::opening`] says whether the lid is being
//! opened inside the opening range (`opening_start_deg..=overbend_threshold_deg`), and the
//! orchestrator plays, pauses and resumes the sound from that.

use crate::config::LidConfig;
use crate::event::{EventKind, EventMeta, TriggerEvent};
use std::collections::BTreeMap;

/// Angle change rate above which the lid counts as "moving" (used by slap suppression).
pub const MOTION_DEG_PER_S: f32 = 5.0;
/// The lid counts as opening once it has risen this far from where it rested (the sensor
/// reports whole degrees, so a single 1° step may be jitter).
pub const OPENING_RISE_DEG: f32 = 2.0;
/// Opening stops when the lid has not risen for this long…
pub const OPENING_HOLD_MS: u64 = 300;
/// …or has dropped this far below the highest angle of the current opening.
pub const OPENING_DROP_DEG: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    /// Below the close threshold.
    Low,
    Above,
}

/// Is the lid being opened right now?
#[derive(Debug, Clone, Copy, PartialEq)]
enum Motion {
    /// Not rising. `low` is the lowest angle since it stopped.
    Resting { low: f32 },
    /// Rising. `peak` is the highest angle so far, reached at `peak_ms`.
    Rising { peak: f32, peak_ms: u64 },
}

#[derive(Debug, Clone)]
pub struct LidDetector {
    cfg: LidConfig,
    zone: Option<Zone>,
    motion: Option<Motion>,
    last: Option<(f32, u64)>,
    last_motion_ms: Option<u64>,
    last_fired: BTreeMap<EventKind, u64>,
}

impl LidDetector {
    pub fn new(cfg: LidConfig) -> Self {
        Self {
            cfg,
            zone: None,
            motion: None,
            last: None,
            last_motion_ms: None,
            last_fired: BTreeMap::new(),
        }
    }

    /// Swap thresholds at runtime. The current zone is re-derived on the next sample without firing.
    pub fn set_config(&mut self, cfg: LidConfig) {
        self.cfg = cfg;
        self.zone = None;
        self.motion = None;
    }

    pub fn config(&self) -> &LidConfig {
        &self.cfg
    }

    /// Latest angle seen, if any.
    pub fn angle(&self) -> Option<f32> {
        self.last.map(|(a, _)| a)
    }

    /// When the lid last moved faster than [`MOTION_DEG_PER_S`].
    pub fn last_motion_ms(&self) -> Option<u64> {
        self.last_motion_ms
    }

    /// The lid is being opened inside the opening range: `LidOverbent` should be playing.
    pub fn opening(&self) -> bool {
        matches!(self.motion, Some(Motion::Rising { .. }))
            && self.angle().is_some_and(|a| self.in_opening_range(a))
    }

    /// The lid is below the opening range (the next opening starts the sound from the top).
    pub fn below_opening_range(&self) -> bool {
        self.angle().is_some_and(|a| a < self.cfg.opening_start_deg)
    }

    fn in_opening_range(&self, a: f32) -> bool {
        (self.cfg.opening_start_deg..=self.cfg.overbend_threshold_deg).contains(&a)
    }

    fn track_motion(&mut self, a: f32, at_ms: u64) {
        self.motion = Some(match self.motion {
            None => Motion::Resting { low: a },
            Some(Motion::Resting { low }) if a - low >= OPENING_RISE_DEG => Motion::Rising {
                peak: a,
                peak_ms: at_ms,
            },
            Some(Motion::Resting { low }) => Motion::Resting { low: low.min(a) },
            Some(Motion::Rising { peak, .. }) if a > peak => Motion::Rising {
                peak: a,
                peak_ms: at_ms,
            },
            Some(Motion::Rising { peak, peak_ms })
                if peak - a < OPENING_DROP_DEG && at_ms.saturating_sub(peak_ms) <= OPENING_HOLD_MS =>
            {
                Motion::Rising { peak, peak_ms }
            }
            Some(Motion::Rising { .. }) => Motion::Resting { low: a },
        });
    }

    /// Process one polled angle sample.
    pub fn on_sample(&mut self, angle_deg: f32, at_ms: u64) -> Option<TriggerEvent> {
        if let Some((prev, prev_ms)) = self.last {
            let dt = at_ms.saturating_sub(prev_ms);
            if dt > 0 && (angle_deg - prev).abs() / (dt as f32 / 1000.0) > MOTION_DEG_PER_S {
                self.last_motion_ms = Some(at_ms);
            }
        }
        self.last = Some((angle_deg, at_ms));
        self.track_motion(angle_deg, at_ms);

        let Some(prev_zone) = self.zone else {
            // First sample (or after a config change): learn the zone, never fire.
            self.zone = Some(self.raw_zone(angle_deg));
            return None;
        };
        let zone = self.next_zone(prev_zone, angle_deg);
        self.zone = Some(zone);
        if zone == prev_zone {
            return None;
        }
        match zone {
            Zone::Low => self.fire(EventKind::LidClosing, angle_deg, at_ms),
            Zone::Above => None,
        }
    }

    /// The system woke up. Fires `LidOpened` if the lid is open far enough.
    pub fn on_wake(&mut self, angle_deg: f32, at_ms: u64) -> Option<TriggerEvent> {
        self.last = Some((angle_deg, at_ms));
        self.zone = Some(self.raw_zone(angle_deg));
        self.motion = Some(Motion::Resting { low: angle_deg });
        if angle_deg > self.cfg.open_min_deg {
            self.fire(EventKind::LidOpened, angle_deg, at_ms)
        } else {
            None
        }
    }

    /// The system is going to sleep: forget state so the wake sample starts fresh.
    pub fn on_sleep(&mut self) {
        self.zone = None;
        self.motion = None;
        self.last = None;
    }

    fn raw_zone(&self, a: f32) -> Zone {
        if a < self.cfg.close_threshold_deg {
            Zone::Low
        } else {
            Zone::Above
        }
    }

    /// Zone transition with hysteresis: leaving Low needs to pass the threshold by `hysteresis_deg`.
    fn next_zone(&self, prev: Zone, a: f32) -> Zone {
        let c = &self.cfg;
        match prev {
            Zone::Low if a <= c.close_threshold_deg + c.hysteresis_deg => Zone::Low,
            _ => self.raw_zone(a),
        }
    }

    fn fire(&mut self, kind: EventKind, angle_deg: f32, at_ms: u64) -> Option<TriggerEvent> {
        if let Some(&t) = self.last_fired.get(&kind)
            && at_ms.saturating_sub(t) < self.cfg.cooldown_ms as u64
        {
            return None;
        }
        self.last_fired.insert(kind, at_ms);
        Some(TriggerEvent {
            kind,
            at_ms,
            meta: EventMeta::Lid { angle_deg },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Defaults: open_min 10°, close 20°, overbend 120°, hysteresis 5°, cooldown 3 s.
    fn det() -> LidDetector {
        LidDetector::new(LidConfig::default())
    }

    /// Feed (angle, ms) pairs, return the kinds fired.
    fn run(d: &mut LidDetector, samples: &[(f32, u64)]) -> Vec<EventKind> {
        samples
            .iter()
            .filter_map(|&(a, t)| d.on_sample(a, t))
            .map(|e| e.kind)
            .collect()
    }

    #[test]
    fn first_sample_never_fires() {
        let mut d = det();
        assert_eq!(run(&mut d, &[(5.0, 0)]), vec![]);
        let mut d = det();
        assert_eq!(run(&mut d, &[(130.0, 0)]), vec![]);
    }

    #[test]
    fn closing_fires_once_when_crossing_down() {
        let mut d = det();
        let fired = run(
            &mut d,
            &[
                (100.0, 0),
                (60.0, 200),
                (25.0, 400),
                (19.0, 600),
                (14.0, 800),
                (12.0, 1000),
            ],
        );
        assert_eq!(fired, vec![EventKind::LidClosing]);
    }

    #[test]
    fn hysteresis_prevents_chatter_at_threshold() {
        let mut d = det();
        // Jitter around 20° must fire only once.
        let fired = run(
            &mut d,
            &[
                (30.0, 0),
                (19.0, 4000),
                (21.0, 8000),
                (19.0, 12000),
                (24.0, 16000),
                (19.0, 20000),
            ],
        );
        assert_eq!(fired, vec![EventKind::LidClosing]);
        // Going past close + hysteresis (25°) re-arms.
        let fired = run(&mut d, &[(26.0, 24000), (18.0, 28000)]);
        assert_eq!(fired, vec![EventKind::LidClosing]);
    }

    #[test]
    fn cooldown_suppresses_quick_repeats() {
        let mut d = det();
        // Re-armed and crossed again within 3 s: suppressed.
        let fired = run(&mut d, &[(30.0, 0), (19.0, 100), (30.0, 500), (19.0, 900)]);
        assert_eq!(fired, vec![EventKind::LidClosing]);
        // After the cooldown it fires again.
        let fired = run(&mut d, &[(30.0, 3500), (19.0, 3600)]);
        assert_eq!(fired, vec![EventKind::LidClosing]);
    }

    #[test]
    fn wake_fires_opened_only_above_open_min() {
        let mut d = det();
        let e = d.on_wake(108.0, 0).unwrap();
        assert_eq!(e.kind, EventKind::LidOpened);
        assert_eq!(e.meta, EventMeta::Lid { angle_deg: 108.0 });

        let mut d = det();
        assert!(d.on_wake(8.0, 0).is_none());
    }

    #[test]
    fn wake_learns_zone_without_firing_closing() {
        let mut d = det();
        d.on_sleep();
        // Woke with the lid already low (e.g. clamshell): opened fires (15° > 10°) but not closing.
        assert_eq!(d.on_wake(15.0, 0).map(|e| e.kind), Some(EventKind::LidOpened));
        assert_eq!(run(&mut d, &[(14.0, 200), (12.0, 400)]), vec![]);
    }

    #[test]
    fn wake_respects_cooldown() {
        let mut d = det();
        assert!(d.on_wake(100.0, 0).is_some());
        d.on_sleep();
        assert!(d.on_wake(100.0, 1000).is_none());
        d.on_sleep();
        assert!(d.on_wake(100.0, 4000).is_some());
    }

    #[test]
    fn motion_tracking() {
        let mut d = det();
        run(&mut d, &[(100.0, 0), (100.0, 200), (101.0, 400)]); // 1°/200 ms = 5°/s, not > 5
        assert_eq!(d.last_motion_ms(), None);
        run(&mut d, &[(97.0, 600)]); // 20°/s
        assert_eq!(d.last_motion_ms(), Some(600));
        assert_eq!(d.angle(), Some(97.0));
    }

    #[test]
    fn replays_spike_trace_shape() {
        // Shape of the recorded lid_move segment (108° → 14° → 125°), sampled at 5 Hz.
        let mut d = det();
        let mut samples = Vec::new();
        let mut t = 0;
        for a in (14..=108).rev().step_by(2) {
            samples.push((a as f32, t));
            t += 200;
        }
        for a in (14..=125).step_by(3) {
            samples.push((a as f32, t));
            t += 200;
        }
        assert_eq!(run(&mut d, &samples), vec![EventKind::LidClosing]);
    }

    /// Feed (angle, ms) pairs, return `opening()` after each.
    fn opening(d: &mut LidDetector, samples: &[(f32, u64)]) -> Vec<bool> {
        samples
            .iter()
            .map(|&(a, t)| {
                d.on_sample(a, t);
                d.opening()
            })
            .collect()
    }

    #[test]
    fn opening_inside_range_only() {
        let mut d = det();
        // 10° → 130° at 30 Hz-ish steps of 3°: rising, but only 30..=120 counts.
        let mut t = 0;
        let mut inside = Vec::new();
        for a in (10..=130).step_by(3) {
            d.on_sample(a as f32, t);
            inside.push((a, d.opening()));
            t += 33;
        }
        for (a, on) in inside {
            // The first sample only learns the resting angle; 10 → 13 is a 3° rise.
            let want = (30..=120).contains(&a);
            assert_eq!(on, want, "at {a}°");
        }
        assert!(!d.below_opening_range());
        d.on_sample(20.0, t);
        assert!(d.below_opening_range());
    }

    #[test]
    fn opening_stops_when_lid_rests_and_resumes() {
        let mut d = det();
        let on = opening(
            &mut d,
            &[
                (40.0, 0),
                (42.0, 33), // +2°: opening
                (45.0, 66),
                (45.0, 200),
                (45.0, 366), // 300 ms since the last rise: still holding
                (45.0, 400), // > 300 ms: stopped
                (46.0, 600), // +1° could be jitter
                (47.0, 633), // +2° from rest: opening again
            ],
        );
        assert_eq!(on, vec![false, true, true, true, true, false, false, true]);
    }

    #[test]
    fn opening_stops_when_lid_goes_down() {
        let mut d = det();
        let on = opening(
            &mut d,
            &[
                (40.0, 0),
                (45.0, 33),
                (50.0, 66),
                (49.0, 99),
                (48.0, 132),
                (47.0, 165),
            ],
        );
        assert_eq!(on, vec![false, true, true, true, false, false]);
    }

    #[test]
    fn jitter_is_not_opening() {
        let mut d = det();
        let on = opening(
            &mut d,
            &[(60.0, 0), (61.0, 200), (60.0, 400), (61.0, 600), (60.0, 800)],
        );
        assert!(on.iter().all(|o| !o));
    }

    #[test]
    fn sleep_wake_and_config_change_reset_opening() {
        let mut d = det();
        opening(&mut d, &[(40.0, 0), (45.0, 33)]);
        assert!(d.opening());
        d.on_sleep();
        assert!(!d.opening());
        d.on_wake(50.0, 100);
        assert!(!d.opening());
        assert_eq!(opening(&mut d, &[(53.0, 133)]), vec![true]);
        d.set_config(LidConfig {
            opening_start_deg: 60.0,
            ..LidConfig::default()
        });
        assert_eq!(d.config().opening_start_deg, 60.0);
        assert!(!d.opening());
        // 53 → 58 rises, but below the new start angle.
        assert_eq!(
            opening(&mut d, &[(53.0, 166), (58.0, 199), (62.0, 232)]),
            vec![false, false, true]
        );
    }
}
