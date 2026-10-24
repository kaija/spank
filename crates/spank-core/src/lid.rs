//! Lid angle event state machine (ADR-0005, design §4.1).
//!
//! Feed it every polled angle with [`LidDetector::on_sample`], and call [`LidDetector::on_wake`] /
//! [`LidDetector::on_sleep`] on system power events. It emits at most one event per call.

use crate::config::LidConfig;
use crate::event::{EventKind, EventMeta, TriggerEvent};
use std::collections::BTreeMap;

/// Angle change rate above which the lid counts as "moving" (used by slap suppression).
pub const MOTION_DEG_PER_S: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    /// Below the close threshold.
    Low,
    Mid,
    /// Above the overbend threshold.
    High,
}

#[derive(Debug, Clone)]
pub struct LidDetector {
    cfg: LidConfig,
    zone: Option<Zone>,
    last: Option<(f32, u64)>,
    last_motion_ms: Option<u64>,
    last_fired: BTreeMap<EventKind, u64>,
}

impl LidDetector {
    pub fn new(cfg: LidConfig) -> Self {
        Self {
            cfg,
            zone: None,
            last: None,
            last_motion_ms: None,
            last_fired: BTreeMap::new(),
        }
    }

    /// Swap thresholds at runtime. The current zone is re-derived on the next sample without firing.
    pub fn set_config(&mut self, cfg: LidConfig) {
        self.cfg = cfg;
        self.zone = None;
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

    /// Process one polled angle sample.
    pub fn on_sample(&mut self, angle_deg: f32, at_ms: u64) -> Option<TriggerEvent> {
        if let Some((prev, prev_ms)) = self.last {
            let dt = at_ms.saturating_sub(prev_ms);
            if dt > 0 && (angle_deg - prev).abs() / (dt as f32 / 1000.0) > MOTION_DEG_PER_S {
                self.last_motion_ms = Some(at_ms);
            }
        }
        self.last = Some((angle_deg, at_ms));

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
        let kind = match zone {
            Zone::Low => EventKind::LidClosing,
            Zone::High => EventKind::LidOverbent,
            Zone::Mid => return None,
        };
        self.fire(kind, angle_deg, at_ms)
    }

    /// The system woke up. Fires `LidOpened` if the lid is open far enough.
    pub fn on_wake(&mut self, angle_deg: f32, at_ms: u64) -> Option<TriggerEvent> {
        self.last = Some((angle_deg, at_ms));
        self.zone = Some(self.raw_zone(angle_deg));
        if angle_deg > self.cfg.open_min_deg {
            self.fire(EventKind::LidOpened, angle_deg, at_ms)
        } else {
            None
        }
    }

    /// The system is going to sleep: forget state so the wake sample starts fresh.
    pub fn on_sleep(&mut self) {
        self.zone = None;
        self.last = None;
    }

    fn raw_zone(&self, a: f32) -> Zone {
        if a < self.cfg.close_threshold_deg {
            Zone::Low
        } else if a > self.cfg.overbend_threshold_deg {
            Zone::High
        } else {
            Zone::Mid
        }
    }

    /// Zone transition with hysteresis: leaving Low/High needs to pass the threshold by `hysteresis_deg`.
    fn next_zone(&self, prev: Zone, a: f32) -> Zone {
        let c = &self.cfg;
        match prev {
            Zone::Low if a <= c.close_threshold_deg + c.hysteresis_deg => Zone::Low,
            Zone::High if a >= c.overbend_threshold_deg - c.hysteresis_deg => Zone::High,
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
    fn overbend_fires_once_when_crossing_up() {
        let mut d = det();
        let fired = run(&mut d, &[(100.0, 0), (118.0, 200), (121.0, 400), (125.0, 600)]);
        assert_eq!(fired, vec![EventKind::LidOverbent]);
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
    fn overbend_rearms_below_threshold_minus_hysteresis() {
        let mut d = det();
        let fired = run(
            &mut d,
            &[
                (100.0, 0),
                (121.0, 1000),
                (116.0, 5000),
                (121.0, 9000),
                (114.0, 13000),
                (121.0, 17000),
            ],
        );
        assert_eq!(fired, vec![EventKind::LidOverbent, EventKind::LidOverbent]);
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
    fn jump_from_low_to_high_fires_overbend() {
        let mut d = det();
        let fired = run(&mut d, &[(10.0, 0), (125.0, 200)]);
        assert_eq!(fired, vec![EventKind::LidOverbent]);
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
    fn config_change_relearns_zone() {
        let mut d = det();
        run(&mut d, &[(100.0, 0)]);
        d.set_config(LidConfig {
            overbend_threshold_deg: 90.0,
            ..LidConfig::default()
        });
        assert_eq!(d.config().overbend_threshold_deg, 90.0);
        // 100° is now above overbend, but the first sample after a change only learns the zone.
        assert_eq!(run(&mut d, &[(100.0, 200)]), vec![]);
        assert_eq!(
            run(&mut d, &[(80.0, 400), (95.0, 4000)]),
            vec![EventKind::LidOverbent]
        );
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
        assert_eq!(
            run(&mut d, &samples),
            vec![EventKind::LidClosing, EventKind::LidOverbent]
        );
    }
}
