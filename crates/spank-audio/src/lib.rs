//! Audio for spank (ADR-0008).
//!
//! - [`import`]: turn a user file (wav/mp3/m4a/aac/flac/ogg) into normalized 48 kHz audio
//! - [`wav`]: store / load the normalized audio as WAV
//!
//! Playback (paused cpal stream, priority mixer) comes in T19.

#![forbid(unsafe_code)]

pub mod import;
pub mod wav;

pub use import::{ImportError, ImportedSound, import_file};
pub use wav::{read_wav, sha256_file, write_wav};
