//! Built-in sounds and profiles ship as `.spankpack` files in the app bundle (T35). They are loaded
//! read-only at startup: audio stays in memory, nothing is copied into the user's library.

use crate::{PackError, read_pack_file, wav_duration_ms};
use spank_core::{EventBinding, EventKind, Profile, ProfileId, SoundEntry, SoundId, SoundSource};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Prefix of built-in pack ids; `dev.spank.builtin.destruction` → profile `builtin.destruction`.
pub const PACK_PREFIX: &str = "dev.spank.builtin.";

#[derive(Debug, Clone, Default)]
pub struct BuiltinSet {
    pub profiles: Vec<Profile>,
    /// Library entries (`SoundSource::Builtin`), sorted by id.
    pub sounds: Vec<SoundEntry>,
    /// WAV bytes by sound id.
    pub audio: BTreeMap<SoundId, Arc<[u8]>>,
}

fn stem(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.strip_suffix(".wav")
        .or_else(|| name.strip_suffix(".WAV"))
        .unwrap_or(name)
}

/// Load every `*.spankpack` in `dir`. Broken packs are skipped and reported.
pub fn load_builtin(dir: &Path) -> (BuiltinSet, Vec<String>) {
    let mut set = BuiltinSet::default();
    let mut errors = Vec::new();
    let mut paths: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "spankpack"))
            .collect(),
        Err(e) => return (set, vec![format!("{}: {e}", dir.display())]),
    };
    paths.sort();
    for path in paths {
        if let Err(e) = add_pack(&mut set, &path) {
            errors.push(format!("{}: {e}", path.display()));
        }
    }
    set.sounds.sort_by(|a, b| a.id.cmp(&b.id));
    set.profiles.sort_by(|a, b| a.id.cmp(&b.id));
    (set, errors)
}

fn add_pack(set: &mut BuiltinSet, path: &Path) -> Result<(), PackError> {
    let pack = read_pack_file(path)?;
    let m = &pack.manifest;
    let name =
        m.id.strip_prefix(PACK_PREFIX)
            .ok_or_else(|| PackError::Manifest(format!("built-in pack id must start with {PACK_PREFIX}")))?;
    let mut ids = BTreeMap::new();
    for s in &m.sounds {
        let id = SoundId(format!("builtin.{}", stem(&s.file)));
        let bytes = &pack.sounds[&s.file];
        if !set.audio.contains_key(&id) {
            set.sounds.push(SoundEntry {
                id: id.clone(),
                name: stem(&s.file).to_owned(),
                source: SoundSource::Builtin,
                duration_ms: wav_duration_ms(&s.file, bytes)?,
                sha256: s.sha256.clone(),
                license: s.license.clone().or_else(|| Some(m.license.clone())),
                added_unix_ms: 0,
            });
            set.audio.insert(id.clone(), Arc::from(bytes.as_slice()));
        }
        ids.insert(s.file.clone(), id);
    }
    if let Some(map) = &m.profile {
        let mut p = Profile::new(ProfileId(format!("builtin.{name}")), m.display_name());
        p.builtin = true;
        for k in EventKind::ALL {
            let files = map.get(&crate::PackEvent(k)).map(Vec::as_slice).unwrap_or(&[]);
            p.mapping.insert(
                k,
                EventBinding {
                    enabled: !files.is_empty(),
                    sounds: files.iter().map(|f| ids[f].clone()).collect(),
                },
            );
        }
        set.profiles.push(p);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds/builtin")
    }

    #[test]
    fn shipped_builtin_packs_load() {
        let (set, errors) = load_builtin(&assets());
        assert!(errors.is_empty(), "{errors:?}");
        let ids: Vec<_> = set.profiles.iter().map(|p| p.id.0.as_str()).collect();
        assert_eq!(ids, vec!["builtin.classic", "builtin.destruction"]);
        assert!(set.profiles.iter().all(|p| p.builtin && p.validate().is_ok()));
        // Every referenced sound is present, and shared sounds are loaded once.
        for p in &set.profiles {
            for s in p.referenced_sounds() {
                assert!(set.audio.contains_key(s), "{} missing", s.0);
            }
        }
        assert_eq!(set.sounds.len(), set.audio.len());
        assert!(
            set.sounds
                .iter()
                .all(|s| s.source == SoundSource::Builtin && s.duration_ms > 0)
        );
        let destruction = &set.profiles[1];
        assert_eq!(destruction.name, "全玻璃");
        // Every built-in profile covers all seven events (ADR-0016).
        for p in &set.profiles {
            for k in EventKind::ALL {
                assert!(p.playable(k).is_some(), "{} has no sound for {k:?}", p.id.0);
            }
        }
        let total: usize = set.audio.values().map(|a| a.len()).sum();
        // ADR-0016 budget for the uncompressed built-in audio.
        assert!(total < 8 * 1024 * 1024);
    }

    #[test]
    fn bad_dirs_and_packs_are_reported() {
        let (set, errors) = load_builtin(Path::new("/nonexistent"));
        assert!(set.profiles.is_empty());
        assert_eq!(errors.len(), 1);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.spankpack"), b"nope").unwrap();
        let (_, errors) = load_builtin(dir.path());
        assert_eq!(errors.len(), 1);
        assert_eq!(stem("sounds/a.WAV"), "a");
    }
}
