//! Pure domain logic for spank (ADR-0010: no OS dependencies, fully unit-testable).
//!
//! - [`event`]: trigger events and their playback priority (ADR-0005, ADR-0006, ADR-0008)
//! - [`profile`]: profiles mapping events to sounds (ADR-0007)
//! - [`config`]: user configuration with validated defaults

#![forbid(unsafe_code)]

pub mod config;
pub mod event;
pub mod profile;

pub use config::{
    AppConfig, ConfigError, Language, LidConfig, MuteRules, QuietHours, SlapConfig, SlapLevels, TimeOfDay,
};
pub use event::{EventKind, EventMeta, Priority, TriggerEvent};
pub use profile::{EventBinding, Profile, ProfileId, SoundId};
