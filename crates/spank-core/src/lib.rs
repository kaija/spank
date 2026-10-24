//! Pure domain logic for spank (ADR-0010: no OS dependencies, fully unit-testable).
//!
//! - [`event`]: trigger events and their playback priority (ADR-0005, ADR-0006, ADR-0008)
//! - [`profile`]: profiles mapping events to sounds (ADR-0007)
//! - [`config`]: user configuration with validated defaults
//! - [`lid`]: lid angle event state machine (ADR-0005)
//! - [`slap`]: slap suppression, levels and combos (ADR-0006)
//! - [`sound`]: sound library index entries

#![forbid(unsafe_code)]

pub mod config;
pub mod event;
pub mod lid;
pub mod profile;
pub mod slap;
pub mod sound;

pub use config::{
    AppConfig, Calibration, ConfigError, Language, LidConfig, MuteRules, QuietHours, SlapConfig, SlapLevels,
    TimeOfDay,
};
pub use event::{EventKind, EventMeta, Priority, TriggerEvent};
pub use lid::LidDetector;
pub use profile::{EventBinding, Profile, ProfileId, SoundId};
pub use slap::{GateSignals, ImpactCandidate, SlapClassifier, SlapOutcome, SuppressReason};
pub use sound::{SoundEntry, SoundIndex, SoundSource};

/// Writes TypeScript bindings for the settings UI.
/// Run: `cargo test -p spank-core --features ts export_bindings` (CI checks they are up to date).
#[cfg(all(test, feature = "ts"))]
mod ts_export {
    use super::*;
    use ts_rs::TS;

    #[test]
    fn export_bindings() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src/bindings");
        // u64 timestamps fit in a JS number; serde_json emits them as numbers.
        let cfg = ts_rs::Config::new().with_out_dir(&dir).with_large_int("number");
        AppConfig::export_all(&cfg).unwrap();
        Calibration::export_all(&cfg).unwrap();
        Profile::export_all(&cfg).unwrap();
        TriggerEvent::export_all(&cfg).unwrap();
        SoundIndex::export_all(&cfg).unwrap();
        SuppressReason::export_all(&cfg).unwrap();
    }
}
