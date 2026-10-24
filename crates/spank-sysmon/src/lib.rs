//! System signals (design §7) as a stream of [`SysSignal`] changes.
//!
//! | Signal | Source |
//! |--------|--------|
//! | sleep / wake | `NSWorkspaceWillSleep` / `DidWake` notifications |
//! | screen lock | distributed `com.apple.screenIsLocked` / `screenIsUnlocked` |
//! | microphone | CoreAudio process objects (`kAudioProcessPropertyIsRunningInput`, macOS 14+), excluding this process |
//! | camera | CoreMediaIO `kCMIODevicePropertyDeviceIsRunningSomewhere` |
//! | output mute / volume | CoreAudio default output device `mute` / `volm` |
//! | Focus | `INFocusStatusCenter.isFocused`, polled (there is no change notification); `None` unless authorized |
//!
//! Everything except Focus is listener-driven. Notifications are delivered on the main queue, so
//! the main thread must run its run loop (the Tauri app does). The sink is called from one
//! monitor thread, only when a value changes.

pub use spank_core::SysSignal;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{
    FocusAuthorization, SysMonitor, camera_in_use, focus_authorization, focus_status, mic_in_use,
    output_state, request_focus_authorization,
};

/// Remembers the last value of each signal and drops repeats.
#[derive(Debug, Default, Clone)]
pub struct Dedup {
    mic: Option<bool>,
    camera: Option<bool>,
    focus: Option<Option<bool>>,
    output: Option<(bool, u32)>,
}

impl Dedup {
    /// Returns the signal if it changes something (sleep/wake/lock always pass).
    pub fn filter(&mut self, s: SysSignal) -> Option<SysSignal> {
        let changed = match s {
            SysSignal::MicInUse { in_use } => self.mic.replace(in_use) != Some(in_use),
            SysSignal::CameraInUse { in_use } => self.camera.replace(in_use) != Some(in_use),
            SysSignal::Focus { focused } => self.focus.replace(focused) != Some(focused),
            // Volume is compared at 1 % steps so slider noise doesn't spam.
            SysSignal::Output { muted, volume } => {
                let v = (muted, (volume.clamp(0.0, 1.0) * 100.0).round() as u32);
                self.output.replace(v) != Some(v)
            }
            _ => true,
        };
        changed.then_some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_drops_repeats() {
        let mut d = Dedup::default();
        assert!(d.filter(SysSignal::MicInUse { in_use: false }).is_some());
        assert!(d.filter(SysSignal::MicInUse { in_use: false }).is_none());
        assert!(d.filter(SysSignal::MicInUse { in_use: true }).is_some());
        assert!(d.filter(SysSignal::CameraInUse { in_use: true }).is_some());
        assert!(d.filter(SysSignal::CameraInUse { in_use: true }).is_none());
        assert!(d.filter(SysSignal::Focus { focused: None }).is_some());
        assert!(d.filter(SysSignal::Focus { focused: None }).is_none());
        let out = |volume| SysSignal::Output { muted: false, volume };
        assert!(d.filter(out(0.5)).is_some());
        assert!(d.filter(out(0.501)).is_none());
        assert!(d.filter(out(0.52)).is_some());
        assert!(d.filter(SysSignal::DidWake).is_some());
        assert!(d.filter(SysSignal::DidWake).is_some());
    }
}
