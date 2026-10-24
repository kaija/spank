//! Payloads shared with the settings UI (Tauri commands and events, design §10.3).

use crate::event::TriggerEvent;
use crate::mute::MuteStatus;
use crate::orchestrator::FireResult;
use crate::profile::{Profile, ProfileId};
use crate::sample::SensorCaps;
use crate::sound::SoundEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FocusAuthorization {
    #[default]
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
}

/// Snapshot of the running engine (`get_state`, event `engine_state`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RuntimeState {
    pub paused: bool,
    pub mute: MuteStatus,
    pub active_profile: ProfileId,
    pub lid_angle: Option<f32>,
    pub lid_running: bool,
    pub accel_running: bool,
    pub caps: SensorCaps,
    pub focus_authorization: FocusAuthorization,
    /// False when no audio output device could be opened.
    pub audio_ok: bool,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            paused: false,
            mute: MuteStatus::default(),
            active_profile: crate::config::AppConfig::DEFAULT_PROFILE.into(),
            lid_angle: None,
            lid_running: false,
            accel_running: false,
            caps: SensorCaps::default(),
            focus_authorization: FocusAuthorization::default(),
            audio_ok: true,
        }
    }
}

/// Event `trigger_fired`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FiredEvent {
    pub event: TriggerEvent,
    pub result: FireResult,
}

/// Result of importing one file into the library (`import_sound`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ImportOutcome {
    Added {
        sound: SoundEntry,
        /// Cut to the first 10 s (ADR-0008).
        truncated: bool,
        original_ms: u32,
    },
    Failed {
        /// `too_large`, `unsupported`, `silent` or `other`.
        reason: String,
        message: String,
    },
}

/// Result of `import_pack`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PackImported {
    pub sounds: u32,
    pub profile: Option<Profile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_shapes() {
        let s = RuntimeState::default();
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["focus_authorization"], "not_determined");
        assert_eq!(j["active_profile"], "builtin.destruction");
        let o = ImportOutcome::Failed {
            reason: "too_large".into(),
            message: "x".into(),
        };
        assert_eq!(serde_json::to_value(&o).unwrap()["status"], "failed");
        assert_eq!(serde_json::from_value::<RuntimeState>(j).unwrap(), s);
    }
}
