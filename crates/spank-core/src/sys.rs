//! System signals (design §7). `spank-sysmon` produces them from OS notifications and listeners;
//! the auto-mute evaluator and the orchestrator consume them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SysSignal {
    /// The system is about to sleep.
    WillSleep,
    /// The system woke up.
    DidWake,
    ScreenLocked,
    ScreenUnlocked,
    /// Some input device (microphone) is in use by any process.
    MicInUse {
        in_use: bool,
    },
    /// Some camera is in use by any process.
    CameraInUse {
        in_use: bool,
    },
    /// Focus / Do Not Disturb. `None`: not readable (no permission or unsupported build, ADR-0007).
    Focus {
        focused: Option<bool>,
    },
    /// Default output device mute and volume (0.0–1.0).
    Output {
        muted: bool,
        volume: f32,
    },
}

/// Latest value of every signal. Defaults to "nothing in use, awake, unlocked, audible".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SysState {
    pub asleep: bool,
    pub locked: bool,
    pub mic_in_use: bool,
    pub camera_in_use: bool,
    pub focus: Option<bool>,
    pub output_muted: bool,
    pub output_volume: f32,
}

impl Default for SysState {
    fn default() -> Self {
        Self {
            asleep: false,
            locked: false,
            mic_in_use: false,
            camera_in_use: false,
            focus: None,
            output_muted: false,
            output_volume: 1.0,
        }
    }
}

impl SysState {
    /// Apply a signal. Returns true if anything changed.
    pub fn apply(&mut self, s: SysSignal) -> bool {
        let before = *self;
        match s {
            SysSignal::WillSleep => self.asleep = true,
            SysSignal::DidWake => self.asleep = false,
            SysSignal::ScreenLocked => self.locked = true,
            SysSignal::ScreenUnlocked => self.locked = false,
            SysSignal::MicInUse { in_use } => self.mic_in_use = in_use,
            SysSignal::CameraInUse { in_use } => self.camera_in_use = in_use,
            SysSignal::Focus { focused } => self.focus = focused,
            SysSignal::Output { muted, volume } => {
                self.output_muted = muted;
                self.output_volume = volume.clamp(0.0, 1.0);
            }
        }
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_tracks_changes() {
        let mut s = SysState::default();
        assert!(s.apply(SysSignal::MicInUse { in_use: true }));
        assert!(!s.apply(SysSignal::MicInUse { in_use: true }));
        assert!(s.apply(SysSignal::WillSleep));
        assert!(s.asleep);
        assert!(s.apply(SysSignal::DidWake));
        assert!(s.apply(SysSignal::ScreenLocked) && s.locked);
        assert!(s.apply(SysSignal::ScreenUnlocked) && !s.locked);
        assert!(s.apply(SysSignal::CameraInUse { in_use: true }) && s.camera_in_use);
        assert!(s.apply(SysSignal::Focus { focused: Some(true) }));
        assert!(s.apply(SysSignal::Output {
            muted: true,
            volume: 1.7
        }));
        assert_eq!(s.output_volume, 1.0);
    }

    #[test]
    fn serde_shape() {
        let j = serde_json::to_string(&SysSignal::MicInUse { in_use: true }).unwrap();
        assert_eq!(j, r#"{"type":"mic_in_use","in_use":true}"#);
        let back: SysSignal = serde_json::from_str(&j).unwrap();
        assert_eq!(back, SysSignal::MicInUse { in_use: true });
    }
}
