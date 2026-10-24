use crate::profile::ProfileId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

impl ConfigError {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}

/// Lid angle event thresholds (ADR-0005).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct LidConfig {
    /// On wake, play `LidOpened` only if the lid is above this angle.
    pub open_min_deg: f32,
    pub close_threshold_deg: f32,
    pub overbend_threshold_deg: f32,
    pub hysteresis_deg: f32,
    pub cooldown_ms: u32,
}

impl Default for LidConfig {
    fn default() -> Self {
        Self {
            open_min_deg: 10.0,
            close_threshold_deg: 20.0,
            overbend_threshold_deg: 120.0,
            hysteresis_deg: 5.0,
            cooldown_ms: 3000,
        }
    }
}

impl LidConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let in_range = |v: f32| (0.0..=180.0).contains(&v);
        if ![
            self.open_min_deg,
            self.close_threshold_deg,
            self.overbend_threshold_deg,
        ]
        .into_iter()
        .all(in_range)
        {
            return Err(ConfigError::invalid("lid angles must be within 0..=180°"));
        }
        if self.close_threshold_deg >= self.overbend_threshold_deg {
            return Err(ConfigError::invalid(
                "close threshold must be below overbend threshold",
            ));
        }
        let gap = self.overbend_threshold_deg - self.close_threshold_deg;
        if !(self.hysteresis_deg > 0.0 && self.hysteresis_deg * 2.0 < gap) {
            return Err(ConfigError::invalid(format!(
                "hysteresis must be > 0 and < half the threshold gap ({gap}°)"
            )));
        }
        Ok(())
    }
}

/// Peak thresholds (in g, gravity removed) for each slap level. Set by calibration (ADR-0006).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct SlapLevels {
    pub light_g: f32,
    pub medium_g: f32,
    pub hard_g: f32,
}

impl Default for SlapLevels {
    /// Pre-calibration guesses from the M4 Air traces (T10: light 0.16–0.38 g, medium 0.2–0.34 g,
    /// hard 0.55–0.68 g; light and medium overlap, so calibration decides).
    fn default() -> Self {
        Self {
            light_g: 0.15,
            medium_g: 0.3,
            hard_g: 0.5,
        }
    }
}

/// Result of the slap calibration wizard (`calibration.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Calibration {
    pub levels: SlapLevels,
    /// Typical high-passed |a| (g) while typing, learned during calibration.
    pub noise_floor_g: f32,
    pub calibrated_unix_ms: u64,
}

/// Slap detection settings (ADR-0006). Opt-in: disabled until the user enables it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct SlapConfig {
    pub enabled: bool,
    pub highpass_hz: f32,
    /// Minimum high-passed |a| (g) that opens an impact window.
    pub trigger_g: f32,
    /// No new impact within this time after one fires (absorbs rebound peaks).
    pub refractory_ms: u32,
    pub levels: SlapLevels,
    pub combo_window_ms: u32,
    /// The N-th slap inside the window (and later ones) becomes a combo.
    pub combo_count: u8,
    /// Use Input Monitoring to suppress impacts right after typing (needs permission).
    pub precise_typing: bool,
}

impl Default for SlapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            highpass_hz: 2.0,
            trigger_g: 0.15,
            refractory_ms: 150,
            levels: SlapLevels::default(),
            combo_window_ms: 5000,
            combo_count: 3,
            precise_typing: false,
        }
    }
}

impl SlapConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let l = &self.levels;
        if !(l.light_g > 0.0 && l.light_g < l.medium_g && l.medium_g < l.hard_g) {
            return Err(ConfigError::invalid(
                "slap levels must satisfy 0 < light < medium < hard",
            ));
        }
        if !(self.trigger_g > 0.0 && self.trigger_g <= l.light_g) {
            return Err(ConfigError::invalid("trigger_g must be > 0 and <= light level"));
        }
        if !(0.1..=20.0).contains(&self.highpass_hz) {
            return Err(ConfigError::invalid("highpass_hz must be within 0.1..=20"));
        }
        if self.combo_count < 2 {
            return Err(ConfigError::invalid("combo_count must be >= 2"));
        }
        if self.refractory_ms == 0
            || self.refractory_ms as u64 * self.combo_count as u64 > self.combo_window_ms as u64
        {
            return Err(ConfigError::invalid(
                "combo window too short for refractory × combo_count",
            ));
        }
        Ok(())
    }
}

/// Wall-clock time of day, serialized as "HH:MM".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(type = "string"))]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

impl TimeOfDay {
    pub fn new(hour: u8, minute: u8) -> Result<Self, ConfigError> {
        if hour > 23 || minute > 59 {
            return Err(ConfigError::invalid(format!(
                "invalid time {hour:02}:{minute:02}"
            )));
        }
        Ok(Self { hour, minute })
    }

    pub fn minutes(self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

impl FromStr for TimeOfDay {
    type Err = ConfigError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || ConfigError::invalid(format!("time must be HH:MM, got {s:?}"));
        let (h, m) = s.split_once(':').ok_or_else(bad)?;
        if h.len() != 2 || m.len() != 2 {
            return Err(bad());
        }
        Self::new(h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?)
    }
}

impl Serialize for TimeOfDay {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A quiet-hours range. `end < start` means it crosses midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct QuietHours {
    pub start: TimeOfDay,
    pub end: TimeOfDay,
    /// User's name for the range, e.g. "午休".
    #[serde(default)]
    pub label: String,
}

/// Automatic mute rules (ADR-0007). Any active rule mutes playback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct MuteRules {
    pub meeting: bool,
    pub focus: bool,
    pub follow_system: bool,
    pub quiet_hours_enabled: bool,
    pub quiet_hours: Vec<QuietHours>,
}

impl Default for MuteRules {
    fn default() -> Self {
        Self {
            meeting: true,
            focus: true,
            follow_system: true,
            quiet_hours_enabled: true,
            quiet_hours: Vec::new(),
        }
    }
}

impl MuteRules {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.quiet_hours.iter().any(|q| q.start == q.end) {
            return Err(ConfigError::invalid("quiet hours start and end must differ"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    System,
    ZhHant,
    En,
}

/// Top-level `config.json` (design §9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    pub active_profile: ProfileId,
    /// Global volume multiplier, 0.0–1.0.
    pub app_volume: f32,
    pub paused: bool,
    pub launch_at_login: bool,
    pub language: Language,
    pub auto_check_updates: bool,
    pub onboarding_done: bool,
    pub lid: LidConfig,
    pub slap: SlapConfig,
    pub mute: MuteRules,
}

impl AppConfig {
    pub const SCHEMA_VERSION: u32 = 1;
    pub const DEFAULT_PROFILE: &'static str = "builtin.destruction";

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version == 0 || self.schema_version > Self::SCHEMA_VERSION {
            return Err(ConfigError::invalid(format!(
                "unsupported schema_version {}",
                self.schema_version
            )));
        }
        if !(0.0..=1.0).contains(&self.app_volume) {
            return Err(ConfigError::invalid("app_volume out of 0..=1"));
        }
        self.lid.validate()?;
        self.slap.validate()?;
        self.mute.validate()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            active_profile: Self::DEFAULT_PROFILE.into(),
            app_volume: 0.8,
            paused: false,
            launch_at_login: true,
            language: Language::System,
            auto_check_updates: true,
            onboarding_done: false,
            lid: LidConfig::default(),
            slap: SlapConfig::default(),
            mute: MuteRules::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        AppConfig::default().validate().unwrap();
    }

    #[test]
    fn defaults_match_adrs() {
        let c = AppConfig::default();
        assert!(!c.slap.enabled, "slap is opt-in");
        assert_eq!(c.slap.trigger_g, 0.15);
        assert_eq!(c.slap.refractory_ms, 150);
        assert_eq!(c.lid.close_threshold_deg, 20.0);
        assert_eq!(c.lid.overbend_threshold_deg, 120.0);
        assert!(c.mute.meeting && c.mute.focus && c.mute.follow_system);
        assert!(c.launch_at_login);
    }

    #[test]
    fn config_round_trip() {
        let mut c = AppConfig::default();
        c.mute.quiet_hours.push(QuietHours {
            start: "23:00".parse().unwrap(),
            end: "07:30".parse().unwrap(),
            label: String::new(),
        });
        c.language = Language::ZhHant;
        let json = serde_json::to_string_pretty(&c).unwrap();
        assert!(json.contains("\"23:00\""));
        assert!(json.contains("\"zh-hant\""));
        assert_eq!(serde_json::from_str::<AppConfig>(&json).unwrap(), c);
    }

    #[test]
    fn partial_config_fills_defaults() {
        let c: AppConfig = serde_json::from_str(r#"{"schema_version":1,"slap":{"enabled":true}}"#).unwrap();
        assert!(c.slap.enabled);
        assert_eq!(c.slap.trigger_g, 0.15);
        assert_eq!(c.lid, LidConfig::default());
        c.validate().unwrap();
    }

    #[test]
    fn rejects_future_schema() {
        let c = AppConfig {
            schema_version: 99,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn lid_validation() {
        let bad = LidConfig {
            close_threshold_deg: 130.0,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = LidConfig {
            hysteresis_deg: 0.0,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = LidConfig {
            hysteresis_deg: 60.0,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = LidConfig {
            overbend_threshold_deg: 200.0,
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn slap_validation() {
        let mut s = SlapConfig::default();
        s.levels.medium_g = 0.1;
        assert!(s.validate().is_err());
        let s = SlapConfig {
            trigger_g: 0.5,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let s = SlapConfig {
            combo_count: 1,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let s = SlapConfig {
            combo_window_ms: 400,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let s = SlapConfig {
            highpass_hz: 0.0,
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn time_of_day_parsing() {
        assert_eq!(
            "07:05".parse::<TimeOfDay>().unwrap(),
            TimeOfDay { hour: 7, minute: 5 }
        );
        assert_eq!("23:59".parse::<TimeOfDay>().unwrap().minutes(), 23 * 60 + 59);
        for bad in ["24:00", "7:05", "12:60", "noon", "12-00", ""] {
            assert!(bad.parse::<TimeOfDay>().is_err(), "{bad} should fail");
        }
        assert_eq!(TimeOfDay::new(9, 0).unwrap().to_string(), "09:00");
        assert!(serde_json::from_str::<TimeOfDay>("\"25:00\"").is_err());
    }

    #[test]
    fn mute_rules_reject_empty_range() {
        let t: TimeOfDay = "10:00".parse().unwrap();
        let m = MuteRules {
            quiet_hours: vec![QuietHours {
                start: t,
                end: t,
                label: String::new(),
            }],
            ..Default::default()
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn app_volume_validation() {
        let c = AppConfig {
            app_volume: -0.1,
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }
}
