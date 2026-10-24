//! Audio for spank (ADR-0008).
//!
//! - [`import`]: turn a user file (wav/mp3/m4a/aac/flac/ogg) into normalized 48 kHz audio
//! - [`wav`]: store / load the normalized audio as WAV
//! - [`mixer`]: single-voice priority mixer (pure, runs in the audio callback)
//! - [`bank`]: decoded sounds preloaded in memory
//! - [`player`]: cpal output kept paused between sounds; implements [`spank_core::Player`]

#![forbid(unsafe_code)]

pub mod bank;
pub mod import;
pub mod mixer;
pub mod player;
pub mod wav;

pub use bank::{BankError, SoundBank};
pub use mixer::{Clip, MixOutcome, Mixer};
pub use player::{AudioPlayer, OutputFormat, PlayerError};

pub use import::{ImportError, ImportedSound, import_file};
pub use wav::{read_wav, sha256_file, write_wav};
