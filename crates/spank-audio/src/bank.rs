//! Preloaded sounds (ADR-0008): the active profile's sounds are decoded into memory so a trigger
//! never touches the disk.

use crate::mixer::Clip;
use crate::wav::read_wav;
use spank_core::SoundId;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Arc;

/// Default memory cap for decoded audio (~30 MB, design §8).
pub const DEFAULT_LIMIT_BYTES: usize = 30 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BankError {
    #[error("could not read {id}: {source}")]
    Read { id: String, source: io::Error },
    #[error("sound bank full: {needed} bytes needed, limit {limit}")]
    Full { needed: usize, limit: usize },
}

#[derive(Debug, Clone)]
pub struct SoundBank {
    clips: HashMap<SoundId, Arc<Clip>>,
    limit: usize,
}

impl Default for SoundBank {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT_BYTES)
    }
}

impl SoundBank {
    pub fn new(limit_bytes: usize) -> Self {
        Self {
            clips: HashMap::new(),
            limit: limit_bytes,
        }
    }

    pub fn get(&self, id: &SoundId) -> Option<Arc<Clip>> {
        self.clips.get(id).cloned()
    }

    pub fn contains(&self, id: &SoundId) -> bool {
        self.clips.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.clips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.clips.values().map(|c| c.bytes()).sum()
    }

    /// Add (or replace) a decoded clip.
    pub fn insert(&mut self, id: SoundId, clip: Clip) -> Result<(), BankError> {
        let existing = self.clips.get(&id).map_or(0, |c| c.bytes());
        let needed = self.bytes() - existing + clip.bytes();
        if needed > self.limit {
            return Err(BankError::Full {
                needed,
                limit: self.limit,
            });
        }
        self.clips.insert(id, Arc::new(clip));
        Ok(())
    }

    /// Decode a stored WAV into the bank.
    pub fn load_wav(&mut self, id: SoundId, path: &Path) -> Result<(), BankError> {
        let (samples, channels, rate) = read_wav(path).map_err(|source| BankError::Read {
            id: id.0.clone(),
            source,
        })?;
        self.insert(id, Clip::new(samples, channels, rate))
    }

    /// Decode WAV bytes (built-in sounds ship inside `.spankpack` files) into the bank.
    pub fn load_wav_bytes(&mut self, id: SoundId, bytes: &[u8]) -> Result<(), BankError> {
        let (samples, channels, rate) = crate::wav::decode_wav(bytes).map_err(|source| BankError::Read {
            id: id.0.clone(),
            source,
        })?;
        self.insert(id, Clip::new(samples, channels, rate))
    }

    /// Drop every sound not in `keep`.
    pub fn retain(&mut self, keep: &[&SoundId]) {
        self.clips.retain(|k, _| keep.contains(&k));
    }

    /// Keep exactly `ids`: drop the rest, load missing ones with `path_of`. Errors for individual
    /// sounds are collected so one bad file doesn't block the profile.
    pub fn sync<'a>(
        &mut self,
        ids: impl IntoIterator<Item = &'a SoundId>,
        path_of: impl Fn(&SoundId) -> Option<std::path::PathBuf>,
    ) -> Vec<BankError> {
        let want: Vec<&SoundId> = ids.into_iter().collect();
        self.clips.retain(|k, _| want.contains(&k));
        let mut errors = Vec::new();
        for id in want {
            if self.contains(id) {
                continue;
            }
            let Some(path) = path_of(id) else {
                errors.push(BankError::Read {
                    id: id.0.clone(),
                    source: io::Error::new(io::ErrorKind::NotFound, "unknown sound"),
                });
                continue;
            };
            if let Err(e) = self.load_wav(id.clone(), &path) {
                errors.push(e);
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::ImportedSound;
    use crate::wav::write_wav;

    fn wav(dir: &Path, name: &str, frames: usize) -> std::path::PathBuf {
        let p = dir.join(format!("{name}.wav"));
        let s = ImportedSound {
            samples: vec![0.25; frames],
            channels: 1,
            sample_rate: 48_000,
            source_rate: 48_000,
            trimmed_leading_ms: 0,
            truncated: false,
            original_ms: (frames * 1000 / 48_000) as u32,
            gain_db: 0.0,
            loudness_lufs: -16.0,
            true_peak_dbtp: -1.0,
        };
        write_wav(&s, &p).unwrap();
        p
    }

    #[test]
    fn sync_loads_and_drops() {
        let dir = tempfile::tempdir().unwrap();
        wav(dir.path(), "a", 4800);
        wav(dir.path(), "b", 4800);
        let mut bank = SoundBank::default();
        let path_of = |id: &SoundId| Some(dir.path().join(format!("{}.wav", id.0)));
        let ids: Vec<SoundId> = vec!["a".into(), "b".into()];
        assert!(bank.sync(&ids, path_of).is_empty());
        assert_eq!(bank.len(), 2);
        let a = bank.get(&"a".into()).unwrap();
        assert_eq!(a.duration_ms(), 100);
        assert!((a.samples[0] - 0.25).abs() < 1e-3);

        let ids: Vec<SoundId> = vec!["b".into(), "missing".into()];
        let errs = bank.sync(&ids, path_of);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].to_string().contains("missing"));
        assert_eq!(bank.len(), 1);
        assert!(!bank.contains(&"a".into()));

        let errs = bank.sync(&["x".into()], |_| None);
        assert_eq!(errs.len(), 1);
        assert!(bank.is_empty());
    }

    #[test]
    fn load_from_bytes_and_retain() {
        let dir = tempfile::tempdir().unwrap();
        let p = wav(dir.path(), "a", 480);
        let mut bank = SoundBank::default();
        bank.load_wav_bytes("a".into(), &std::fs::read(&p).unwrap())
            .unwrap();
        assert!(bank.load_wav_bytes("b".into(), b"junk").is_err());
        bank.retain(&[]);
        assert!(bank.is_empty());
    }

    #[test]
    fn memory_limit() {
        let mut bank = SoundBank::new(1000);
        bank.insert("a".into(), Clip::new(vec![0.0; 200], 1, 48_000))
            .unwrap();
        assert_eq!(bank.bytes(), 800);
        // Replacing counts only the difference.
        bank.insert("a".into(), Clip::new(vec![0.0; 250], 1, 48_000))
            .unwrap();
        assert!(matches!(
            bank.insert("b".into(), Clip::new(vec![0.0; 10], 1, 48_000)),
            Err(BankError::Full { .. })
        ));
    }
}
