//! Slap classification and suppression (ADR-0006, design §4.2 steps 2–3).
//!
//! The impact extractor (T23) turns raw accelerometer data into [`ImpactCandidate`]s. This module
//! decides whether each candidate becomes a slap event: suppression gates first, then strength
//! level, then combo detection.

use crate::config::SlapConfig;
use crate::event::{EventKind, EventMeta, TriggerEvent};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Ignore impacts while the lid moves and for this long after.
pub const LID_MOTION_GRACE_MS: u64 = 500;
/// Ignore impacts while our own sound plays and for this long after (speaker vibration).
pub const PLAYBACK_GRACE_MS: u64 = 200;
/// Precise typing mode: ignore impacts this long after keyboard / trackpad input.
pub const INPUT_GRACE_MS: u64 = 300;

/// One impact window from the accelerometer, gravity removed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ImpactCandidate {
    pub at_ms: u64,
    /// Peak high-passed |a| in g.
    pub peak_g: f32,
    /// Sum of squared |a| over the window.
    pub energy: f32,
    pub duration_ms: u16,
    /// Share of energy above ~50 Hz (typing taps are high, slaps are low).
    pub hf_ratio: f32,
}

/// Context the gates need, supplied by the orchestrator.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GateSignals {
    /// From [`crate::lid::LidDetector::last_motion_ms`].
    pub lid_last_motion_ms: Option<u64>,
    pub playback_active: bool,
    pub playback_ended_ms: Option<u64>,
    /// Last keyboard / trackpad input (only provided in precise typing mode).
    pub last_input_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SuppressReason {
    Disabled,
    LidMoving,
    SelfPlayback,
    Typing,
    BelowThreshold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SlapOutcome {
    Fire(TriggerEvent),
    Suppressed(SuppressReason),
}

#[derive(Debug, Clone)]
pub struct SlapClassifier {
    cfg: SlapConfig,
    /// Accepted slap timestamps inside the combo window.
    recent: VecDeque<u64>,
}

fn within(t: Option<u64>, now: u64, grace: u64) -> bool {
    t.is_some_and(|t| now.saturating_sub(t) <= grace)
}

impl SlapClassifier {
    pub fn new(cfg: SlapConfig) -> Self {
        Self {
            cfg,
            recent: VecDeque::new(),
        }
    }

    pub fn set_config(&mut self, cfg: SlapConfig) {
        self.cfg = cfg;
        self.recent.clear();
    }

    pub fn config(&self) -> &SlapConfig {
        &self.cfg
    }

    /// Why this candidate should be dropped, if it should.
    pub fn gate(&self, c: &ImpactCandidate, g: &GateSignals) -> Option<SuppressReason> {
        let now = c.at_ms;
        if !self.cfg.enabled {
            Some(SuppressReason::Disabled)
        } else if within(g.lid_last_motion_ms, now, LID_MOTION_GRACE_MS) {
            Some(SuppressReason::LidMoving)
        } else if g.playback_active || within(g.playback_ended_ms, now, PLAYBACK_GRACE_MS) {
            Some(SuppressReason::SelfPlayback)
        } else if self.cfg.precise_typing && within(g.last_input_ms, now, INPUT_GRACE_MS) {
            Some(SuppressReason::Typing)
        } else if c.peak_g < self.cfg.levels.light_g {
            Some(SuppressReason::BelowThreshold)
        } else {
            None
        }
    }

    /// Strength level for an accepted candidate.
    pub fn level(&self, peak_g: f32) -> EventKind {
        let l = &self.cfg.levels;
        if peak_g >= l.hard_g {
            EventKind::SlapHard
        } else if peak_g >= l.medium_g {
            EventKind::SlapMedium
        } else {
            EventKind::SlapLight
        }
    }

    pub fn process(&mut self, c: &ImpactCandidate, g: &GateSignals) -> SlapOutcome {
        if let Some(reason) = self.gate(c, g) {
            return SlapOutcome::Suppressed(reason);
        }
        let window = self.cfg.combo_window_ms as u64;
        while self
            .recent
            .front()
            .is_some_and(|&t| c.at_ms.saturating_sub(t) > window)
        {
            self.recent.pop_front();
        }
        self.recent.push_back(c.at_ms);

        let count = self.recent.len();
        let (kind, meta) = if count >= self.cfg.combo_count as usize {
            let count = count.min(u8::MAX as usize) as u8;
            (
                EventKind::SlapCombo,
                EventMeta::Combo {
                    count,
                    peak_g: c.peak_g,
                },
            )
        } else {
            (self.level(c.peak_g), EventMeta::Slap { peak_g: c.peak_g })
        };
        SlapOutcome::Fire(TriggerEvent {
            kind,
            at_ms: c.at_ms,
            meta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled() -> SlapConfig {
        SlapConfig {
            enabled: true,
            ..SlapConfig::default()
        } // levels 0.2 / 0.45 / 0.6
    }

    fn imp(at_ms: u64, peak_g: f32) -> ImpactCandidate {
        ImpactCandidate {
            at_ms,
            peak_g,
            energy: 0.0,
            duration_ms: 40,
            hf_ratio: 0.2,
        }
    }

    fn kind(o: SlapOutcome) -> Option<EventKind> {
        match o {
            SlapOutcome::Fire(e) => Some(e.kind),
            SlapOutcome::Suppressed(_) => None,
        }
    }

    #[test]
    fn levels() {
        let mut c = SlapClassifier::new(enabled());
        assert_eq!(
            kind(c.process(&imp(0, 0.37), &GateSignals::default())),
            Some(EventKind::SlapLight)
        );
        assert_eq!(
            kind(c.process(&imp(10_000, 0.5), &GateSignals::default())),
            Some(EventKind::SlapMedium)
        );
        assert_eq!(
            kind(c.process(&imp(20_000, 0.78), &GateSignals::default())),
            Some(EventKind::SlapHard)
        );
        assert_eq!(c.level(0.6), EventKind::SlapHard);
        assert_eq!(c.level(0.45), EventKind::SlapMedium);
    }

    #[test]
    fn disabled_by_default() {
        let mut c = SlapClassifier::new(SlapConfig::default());
        assert_eq!(
            c.process(&imp(0, 0.7), &GateSignals::default()),
            SlapOutcome::Suppressed(SuppressReason::Disabled)
        );
    }

    #[test]
    fn below_light_threshold_is_dropped() {
        let mut c = SlapClassifier::new(enabled());
        assert_eq!(
            c.process(&imp(0, 0.15), &GateSignals::default()),
            SlapOutcome::Suppressed(SuppressReason::BelowThreshold)
        );
    }

    #[test]
    fn combo_on_third_slap_within_window() {
        // Spike trace: combo slaps ~330 ms apart.
        let mut c = SlapClassifier::new(enabled());
        let g = GateSignals::default();
        let kinds: Vec<_> = [0, 332, 644, 976, 1328]
            .iter()
            .map(|&t| kind(c.process(&imp(t, 0.35), &g)))
            .collect();
        assert_eq!(
            kinds,
            vec![
                Some(EventKind::SlapLight),
                Some(EventKind::SlapLight),
                Some(EventKind::SlapCombo),
                Some(EventKind::SlapCombo),
                Some(EventKind::SlapCombo)
            ]
        );
        match c.process(&imp(1600, 0.35), &g) {
            SlapOutcome::Fire(TriggerEvent {
                meta: EventMeta::Combo { count, .. },
                ..
            }) => assert_eq!(count, 6),
            other => panic!("expected combo, got {other:?}"),
        }
    }

    #[test]
    fn combo_window_expires() {
        let mut c = SlapClassifier::new(enabled());
        let g = GateSignals::default();
        c.process(&imp(0, 0.4), &g);
        c.process(&imp(2000, 0.4), &g);
        // 3rd slap 5.5 s after the 1st: the 1st has left the 5 s window, so only 2 remain.
        assert_eq!(kind(c.process(&imp(5500, 0.4), &g)), Some(EventKind::SlapLight));
        assert_eq!(kind(c.process(&imp(5800, 0.4), &g)), Some(EventKind::SlapCombo));
    }

    #[test]
    fn lid_motion_gate() {
        let mut c = SlapClassifier::new(enabled());
        let g = GateSignals {
            lid_last_motion_ms: Some(1000),
            ..Default::default()
        };
        assert_eq!(
            c.process(&imp(1400, 0.7), &g),
            SlapOutcome::Suppressed(SuppressReason::LidMoving)
        );
        assert_eq!(kind(c.process(&imp(1501, 0.7), &g)), Some(EventKind::SlapHard));
    }

    #[test]
    fn playback_gate() {
        let mut c = SlapClassifier::new(enabled());
        let playing = GateSignals {
            playback_active: true,
            ..Default::default()
        };
        assert_eq!(
            c.process(&imp(0, 0.7), &playing),
            SlapOutcome::Suppressed(SuppressReason::SelfPlayback)
        );
        let ended = GateSignals {
            playback_ended_ms: Some(1000),
            ..Default::default()
        };
        assert_eq!(
            c.process(&imp(1150, 0.7), &ended),
            SlapOutcome::Suppressed(SuppressReason::SelfPlayback)
        );
        assert!(kind(c.process(&imp(1250, 0.7), &ended)).is_some());
    }

    #[test]
    fn typing_gate_only_in_precise_mode() {
        let g = GateSignals {
            last_input_ms: Some(1000),
            ..Default::default()
        };
        let mut c = SlapClassifier::new(enabled());
        assert!(
            kind(c.process(&imp(1100, 0.7), &g)).is_some(),
            "ignored unless precise_typing"
        );

        let mut c = SlapClassifier::new(SlapConfig {
            precise_typing: true,
            ..enabled()
        });
        assert_eq!(
            c.process(&imp(1100, 0.7), &g),
            SlapOutcome::Suppressed(SuppressReason::Typing)
        );
        assert!(kind(c.process(&imp(1400, 0.7), &g)).is_some());
    }

    #[test]
    fn suppressed_candidates_do_not_count_toward_combo() {
        let mut c = SlapClassifier::new(enabled());
        let moving = GateSignals {
            lid_last_motion_ms: Some(0),
            ..Default::default()
        };
        c.process(&imp(100, 0.4), &moving);
        c.process(&imp(400, 0.4), &moving);
        assert_eq!(
            kind(c.process(&imp(900, 0.4), &GateSignals::default())),
            Some(EventKind::SlapLight)
        );
    }

    #[test]
    fn set_config_resets_combo_state() {
        let mut c = SlapClassifier::new(enabled());
        let g = GateSignals::default();
        c.process(&imp(0, 0.4), &g);
        c.process(&imp(300, 0.4), &g);
        c.set_config(enabled());
        assert!(c.config().enabled);
        assert_eq!(kind(c.process(&imp(600, 0.4), &g)), Some(EventKind::SlapLight));
    }
}
