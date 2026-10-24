//! Sound library entries (design §9, ADR-0008). Audio files live next to the index as `<id>.wav`.

use crate::profile::SoundId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SoundSource {
    /// Ships inside the app bundle.
    Builtin,
    /// Imported by the user from a local file.
    User { original_name: String },
    /// Came from a `.spankpack` (ADR-0009).
    Pack { pack_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SoundEntry {
    pub id: SoundId,
    pub name: String,
    pub source: SoundSource,
    pub duration_ms: u32,
    /// Hex sha256 of the stored (normalized) WAV file.
    pub sha256: String,
    /// SPDX identifier, when known.
    #[serde(default)]
    pub license: Option<String>,
    pub added_unix_ms: u64,
}

/// `sounds/index.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SoundIndex {
    pub schema_version: u32,
    #[serde(default)]
    pub sounds: Vec<SoundEntry>,
}

impl SoundIndex {
    pub const SCHEMA_VERSION: u32 = 1;

    pub fn get(&self, id: &SoundId) -> Option<&SoundEntry> {
        self.sounds.iter().find(|s| &s.id == id)
    }
}

impl Default for SoundIndex {
    fn default() -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            sounds: Vec::new(),
        }
    }
}
