//! Auto-mute evaluator (ADR-0007): combines the four rules with the latest system state.
//!
//! Any active rule mutes playback; events are still detected. The wall clock is passed in as a
//! [`TimeOfDay`], so tests (and the runtime) control time.

use crate::config::{MuteRules, QuietHours, TimeOfDay};
use crate::sys::{SysSignal, SysState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum MuteReason {
    /// Microphone or camera in use.
    Meeting,
    Focus,
    QuietHours,
    /// Default output device muted or at zero volume.
    SystemMuted,
}

/// Result of an evaluation. `reasons` is sorted and empty when not muted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct MuteStatus {
    pub reasons: Vec<MuteReason>,
    /// The focus rule is on but Focus status can't be read (ADR-0007: needs the signed build).
    pub focus_unavailable: bool,
}

impl MuteStatus {
    pub fn muted(&self) -> bool {
        !self.reasons.is_empty()
    }
}

const DAY_MIN: u16 = 24 * 60;

impl QuietHours {
    /// `start` is inclusive, `end` exclusive; `end < start` crosses midnight.
    pub fn contains(&self, t: TimeOfDay) -> bool {
        let (s, e, m) = (self.start.minutes(), self.end.minutes(), t.minutes());
        if s <= e { s <= m && m < e } else { m >= s || m < e }
    }
}

/// Evaluate every rule against `sys` at local time `now`.
pub fn evaluate(rules: &MuteRules, sys: &SysState, now: TimeOfDay) -> MuteStatus {
    let mut reasons = Vec::new();
    if rules.meeting && (sys.mic_in_use || sys.camera_in_use) {
        reasons.push(MuteReason::Meeting);
    }
    if rules.focus && sys.focus == Some(true) {
        reasons.push(MuteReason::Focus);
    }
    if rules.quiet_hours_enabled && rules.quiet_hours.iter().any(|q| q.contains(now)) {
        reasons.push(MuteReason::QuietHours);
    }
    if rules.follow_system && (sys.output_muted || sys.output_volume <= 0.0) {
        reasons.push(MuteReason::SystemMuted);
    }
    MuteStatus {
        reasons,
        focus_unavailable: rules.focus && sys.focus.is_none(),
    }
}

/// Minutes from `now` until the next quiet-hours start or end, so the caller knows when to
/// re-evaluate. `None` when no quiet hours are active in the rules.
pub fn minutes_to_next_boundary(rules: &MuteRules, now: TimeOfDay) -> Option<u16> {
    if !rules.quiet_hours_enabled {
        return None;
    }
    let m = now.minutes();
    rules
        .quiet_hours
        .iter()
        .flat_map(|q| [q.start.minutes(), q.end.minutes()])
        .map(|b| (b + DAY_MIN - m - 1) % DAY_MIN + 1)
        .min()
}

/// Stateful wrapper: keeps the latest system state and reports when the status changes.
#[derive(Debug, Clone)]
pub struct AutoMute {
    rules: MuteRules,
    sys: SysState,
    status: MuteStatus,
}

impl AutoMute {
    pub fn new(rules: MuteRules, now: TimeOfDay) -> Self {
        let sys = SysState::default();
        let status = evaluate(&rules, &sys, now);
        Self { rules, sys, status }
    }

    pub fn status(&self) -> &MuteStatus {
        &self.status
    }

    pub fn sys(&self) -> &SysState {
        &self.sys
    }

    pub fn rules(&self) -> &MuteRules {
        &self.rules
    }

    pub fn set_rules(&mut self, rules: MuteRules, now: TimeOfDay) -> Option<MuteStatus> {
        self.rules = rules;
        self.refresh(now)
    }

    /// Apply a system signal. Returns the new status if it changed.
    pub fn on_signal(&mut self, s: SysSignal, now: TimeOfDay) -> Option<MuteStatus> {
        self.sys.apply(s);
        self.refresh(now)
    }

    /// Re-evaluate time-based rules. Returns the new status if it changed.
    pub fn refresh(&mut self, now: TimeOfDay) -> Option<MuteStatus> {
        let next = evaluate(&self.rules, &self.sys, now);
        if next == self.status {
            return None;
        }
        self.status = next.clone();
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> TimeOfDay {
        s.parse().unwrap()
    }

    fn qh(a: &str, b: &str) -> QuietHours {
        QuietHours {
            start: t(a),
            end: t(b),
        }
    }

    fn all_off() -> MuteRules {
        MuteRules {
            meeting: false,
            focus: false,
            follow_system: false,
            quiet_hours_enabled: false,
            quiet_hours: vec![],
        }
    }

    #[test]
    fn quiet_hours_same_day_and_across_midnight() {
        let day = qh("09:00", "17:30");
        assert!(day.contains(t("09:00")));
        assert!(day.contains(t("17:29")));
        assert!(!day.contains(t("17:30")));
        assert!(!day.contains(t("08:59")));

        let night = qh("23:00", "07:00");
        for inside in ["23:00", "23:59", "00:00", "06:59"] {
            assert!(night.contains(t(inside)), "{inside}");
        }
        for outside in ["07:00", "12:00", "22:59"] {
            assert!(!night.contains(t(outside)), "{outside}");
        }
    }

    #[test]
    fn default_state_is_not_muted() {
        let s = evaluate(&MuteRules::default(), &SysState::default(), t("12:00"));
        assert!(!s.muted());
        assert!(s.focus_unavailable, "focus rule on, focus unknown");
    }

    #[test]
    fn each_rule_mutes_only_when_enabled() {
        let now = t("12:00");
        let sys = SysState {
            mic_in_use: true,
            focus: Some(true),
            output_muted: true,
            ..Default::default()
        };
        assert!(!evaluate(&all_off(), &sys, now).muted());

        let rules = MuteRules {
            quiet_hours: vec![qh("11:00", "13:00")],
            ..MuteRules::default()
        };
        let s = evaluate(&rules, &sys, now);
        assert_eq!(
            s.reasons,
            vec![
                MuteReason::Meeting,
                MuteReason::Focus,
                MuteReason::QuietHours,
                MuteReason::SystemMuted
            ]
        );
        assert!(!s.focus_unavailable);

        let rules = MuteRules {
            quiet_hours_enabled: false,
            ..rules
        };
        assert!(
            !evaluate(&rules, &sys, now)
                .reasons
                .contains(&MuteReason::QuietHours)
        );
    }

    #[test]
    fn camera_counts_as_meeting_and_zero_volume_as_muted() {
        let rules = MuteRules {
            meeting: true,
            follow_system: true,
            ..all_off()
        };
        let sys = SysState {
            camera_in_use: true,
            output_volume: 0.0,
            ..Default::default()
        };
        assert_eq!(
            evaluate(&rules, &sys, t("10:00")).reasons,
            vec![MuteReason::Meeting, MuteReason::SystemMuted]
        );
    }

    #[test]
    fn multiple_quiet_ranges() {
        let rules = MuteRules {
            quiet_hours: vec![qh("12:00", "13:00"), qh("22:00", "06:00")],
            ..all_off()
        };
        let rules = MuteRules {
            quiet_hours_enabled: true,
            ..rules
        };
        let at = |s| evaluate(&rules, &SysState::default(), t(s)).muted();
        assert!(at("12:30"));
        assert!(at("02:00"));
        assert!(!at("14:00"));
    }

    #[test]
    fn next_boundary() {
        let rules = MuteRules {
            quiet_hours: vec![qh("23:00", "07:00")],
            ..MuteRules::default()
        };
        assert_eq!(minutes_to_next_boundary(&rules, t("22:00")), Some(60));
        assert_eq!(minutes_to_next_boundary(&rules, t("23:00")), Some(8 * 60));
        assert_eq!(minutes_to_next_boundary(&rules, t("06:59")), Some(1));
        assert_eq!(minutes_to_next_boundary(&MuteRules::default(), t("06:59")), None);
        let off = MuteRules {
            quiet_hours_enabled: false,
            ..rules
        };
        assert_eq!(minutes_to_next_boundary(&off, t("22:00")), None);
    }

    #[test]
    fn auto_mute_reports_changes_only() {
        let mut m = AutoMute::new(MuteRules::default(), t("12:00"));
        assert!(!m.status().muted());
        let s = m
            .on_signal(SysSignal::MicInUse { in_use: true }, t("12:00"))
            .unwrap();
        assert_eq!(s.reasons, vec![MuteReason::Meeting]);
        assert!(
            m.on_signal(SysSignal::CameraInUse { in_use: true }, t("12:00"))
                .is_none()
        );
        assert!(m.sys().camera_in_use);
        m.on_signal(SysSignal::MicInUse { in_use: false }, t("12:00"));
        let s = m
            .on_signal(SysSignal::CameraInUse { in_use: false }, t("12:00"))
            .unwrap();
        assert!(!s.muted());

        let rules = MuteRules {
            quiet_hours: vec![qh("23:00", "07:00")],
            ..MuteRules::default()
        };
        assert!(m.set_rules(rules, t("12:00")).is_none());
        assert_eq!(m.rules().quiet_hours.len(), 1);
        assert!(m.refresh(t("22:59")).is_none());
        assert!(m.refresh(t("23:00")).unwrap().muted());
        assert!(!m.refresh(t("07:00")).unwrap().muted());
    }

    #[test]
    fn focus_signal_none_marks_unavailable() {
        let mut m = AutoMute::new(MuteRules::default(), t("12:00"));
        let s = m
            .on_signal(SysSignal::Focus { focused: Some(false) }, t("12:00"))
            .unwrap();
        assert!(!s.focus_unavailable);
        let s = m
            .on_signal(SysSignal::Focus { focused: Some(true) }, t("12:00"))
            .unwrap();
        assert_eq!(s.reasons, vec![MuteReason::Focus]);
    }
}
