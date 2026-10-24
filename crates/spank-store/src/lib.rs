//! On-disk storage (design §9).
//!
//! ```text
//! <root>/                      ~/Library/Application Support/<bundle-id>/
//!   config.json                AppConfig
//!   calibration.json           slap calibration
//!   profiles/<id>.json         user profiles (built-in profiles ship in the bundle)
//!   sounds/index.json          sound library index
//!   sounds/<id>.wav            normalized audio (written by spank-audio)
//! ```
//!
//! Every write is atomic (temp file + fsync + rename). Files that fail to parse or validate are
//! moved aside as `<name>.corrupt-<unix_ms>` and replaced by defaults, so a bad file never bricks
//! the app.

#![forbid(unsafe_code)]

mod atomic;
pub mod migrate;
pub mod profiles;

use serde::Serialize;
use serde::de::DeserializeOwned;
use spank_core::{AppConfig, Calibration, ConfigError, Profile, ProfileId, SoundEntry, SoundId, SoundIndex};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use atomic::write_atomic;
pub use profiles::Profiles;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("i/o error at {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Invalid(#[from] ConfigError),
    #[error("invalid id {0:?}: use letters, digits, '.', '_' or '-'")]
    InvalidId(String),
    #[error("{0} is built in and read-only")]
    ReadOnly(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0} already exists")]
    AlreadyExists(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// A value loaded from disk, plus what happened if the file had to be replaced.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    pub recovered: Option<Recovery>,
}

/// A corrupt file was moved aside and defaults were used instead.
#[derive(Debug)]
pub struct Recovery {
    pub backup: PathBuf,
    pub reason: String,
}

pub struct Store {
    root: PathBuf,
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Ids become file names, so they must not contain path separators or other surprises.
pub fn validate_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(StoreError::InvalidId(id.to_owned()))
    }
}

impl Store {
    /// `~/Library/Application Support/<bundle_id>` on macOS.
    pub fn default_root(bundle_id: &str) -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join(bundle_id))
    }

    /// Open (and create if needed) a store rooted at `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for dir in [root.clone(), root.join("profiles"), root.join("sounds")] {
            std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub fn calibration_path(&self) -> PathBuf {
        self.root.join("calibration.json")
    }

    pub fn sounds_dir(&self) -> PathBuf {
        self.root.join("sounds")
    }

    pub fn sound_index_path(&self) -> PathBuf {
        self.sounds_dir().join("index.json")
    }

    /// Where the audio for `id` lives. Fails for ids that are not safe file names.
    pub fn sound_file(&self, id: &SoundId) -> Result<PathBuf> {
        validate_id(&id.0)?;
        Ok(self.sounds_dir().join(format!("{}.wav", id.0)))
    }

    fn profile_path(&self, id: &ProfileId) -> Result<PathBuf> {
        validate_id(&id.0)?;
        Ok(self.root.join("profiles").join(format!("{}.json", id.0)))
    }

    // ---- generic helpers ------------------------------------------------------------------

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| StoreError::Json {
            path: path.to_owned(),
            source,
        })?;
        bytes.push(b'\n');
        write_atomic(path, &bytes).map_err(io_err(path))
    }

    /// Read `path`; missing → `None`. Parse/validation failures are returned as `Err(reason)`.
    fn read_raw(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        match std::fs::read(path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_err(path)(e)),
        }
    }

    fn quarantine(&self, path: &Path, reason: String) -> Result<Recovery> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let backup = path.with_file_name(format!("{name}.corrupt-{}", now_ms()));
        std::fs::rename(path, &backup).map_err(io_err(path))?;
        Ok(Recovery { backup, reason })
    }

    /// Load a JSON file, falling back to `default()` when it is missing or unusable.
    fn load_or_recover<T: DeserializeOwned>(
        &self,
        path: &Path,
        parse: impl FnOnce(&[u8]) -> std::result::Result<T, String>,
        default: impl FnOnce() -> T,
    ) -> Result<Loaded<T>> {
        let Some(bytes) = self.read_raw(path)? else {
            return Ok(Loaded {
                value: default(),
                recovered: None,
            });
        };
        match parse(&bytes) {
            Ok(value) => Ok(Loaded {
                value,
                recovered: None,
            }),
            Err(reason) => {
                let recovery = self.quarantine(path, reason)?;
                Ok(Loaded {
                    value: default(),
                    recovered: Some(recovery),
                })
            }
        }
    }

    // ---- config -------------------------------------------------------------------------

    pub fn load_config(&self) -> Result<Loaded<AppConfig>> {
        self.load_or_recover(
            &self.config_path(),
            |b| {
                let v: serde_json::Value = serde_json::from_slice(b).map_err(|e| e.to_string())?;
                let v = migrate::migrate_config(v).map_err(|e| e.to_string())?;
                let c: AppConfig = serde_json::from_value(v).map_err(|e| e.to_string())?;
                c.validate().map_err(|e| e.to_string())?;
                Ok(c)
            },
            AppConfig::default,
        )
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        config.validate()?;
        self.write_json(&self.config_path(), config)
    }

    // ---- calibration --------------------------------------------------------------------

    pub fn load_calibration(&self) -> Result<Loaded<Option<Calibration>>> {
        self.load_or_recover(
            &self.calibration_path(),
            |b| {
                serde_json::from_slice::<Calibration>(b)
                    .map(Some)
                    .map_err(|e| e.to_string())
            },
            || None,
        )
    }

    pub fn save_calibration(&self, c: &Calibration) -> Result<()> {
        self.write_json(&self.calibration_path(), c)
    }

    // ---- profiles -----------------------------------------------------------------------

    /// All user profiles, sorted by name. Unreadable files are skipped (and reported).
    pub fn list_profiles(&self) -> Result<(Vec<Profile>, Vec<Recovery>)> {
        let dir = self.root.join("profiles");
        let mut profiles = Vec::new();
        let mut recovered = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(io_err(&dir))? {
            let path = entry.map_err(io_err(&dir))?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let bytes = std::fs::read(&path).map_err(io_err(&path))?;
            match parse_profile(&bytes) {
                Ok(p) => profiles.push(p),
                Err(reason) => recovered.push(self.quarantine(&path, reason)?),
            }
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Ok((profiles, recovered))
    }

    pub fn load_profile(&self, id: &ProfileId) -> Result<Profile> {
        let path = self.profile_path(id)?;
        let bytes = self
            .read_raw(&path)?
            .ok_or_else(|| StoreError::NotFound(format!("profile {}", id.0)))?;
        parse_profile(&bytes).map_err(|r| StoreError::Invalid(ConfigError::Invalid(r)))
    }

    /// Create or overwrite a user profile. Built-in profiles cannot be saved here.
    pub fn save_profile(&self, profile: &Profile) -> Result<()> {
        if profile.builtin {
            return Err(StoreError::ReadOnly(format!("profile {}", profile.id.0)));
        }
        profile.validate()?;
        let path = self.profile_path(&profile.id)?;
        self.write_json(&path, profile)
    }

    pub fn delete_profile(&self, id: &ProfileId) -> Result<()> {
        let path = self.profile_path(id)?;
        std::fs::remove_file(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StoreError::NotFound(format!("profile {}", id.0)),
            _ => io_err(&path)(e),
        })
    }

    // ---- sound index --------------------------------------------------------------------

    pub fn load_sound_index(&self) -> Result<Loaded<SoundIndex>> {
        self.load_or_recover(
            &self.sound_index_path(),
            |b| {
                let idx: SoundIndex = serde_json::from_slice(b).map_err(|e| e.to_string())?;
                if idx.schema_version == 0 || idx.schema_version > SoundIndex::SCHEMA_VERSION {
                    return Err(format!(
                        "unsupported sound index schema_version {}",
                        idx.schema_version
                    ));
                }
                Ok(idx)
            },
            SoundIndex::default,
        )
    }

    pub fn save_sound_index(&self, index: &SoundIndex) -> Result<()> {
        self.write_json(&self.sound_index_path(), index)
    }

    /// Register a sound whose WAV is already at [`Store::sound_file`].
    pub fn add_sound(&self, entry: SoundEntry) -> Result<()> {
        let file = self.sound_file(&entry.id)?;
        if !file.exists() {
            return Err(StoreError::NotFound(format!("audio file {}", file.display())));
        }
        let mut idx = self.load_sound_index()?.value;
        if idx.get(&entry.id).is_some() {
            return Err(StoreError::AlreadyExists(format!("sound {}", entry.id.0)));
        }
        idx.sounds.push(entry);
        self.save_sound_index(&idx)
    }

    pub fn rename_sound(&self, id: &SoundId, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ConfigError::Invalid("sound name must not be empty".into()).into());
        }
        let mut idx = self.load_sound_index()?.value;
        let entry = idx
            .sounds
            .iter_mut()
            .find(|s| &s.id == id)
            .ok_or_else(|| StoreError::NotFound(format!("sound {}", id.0)))?;
        entry.name = name.to_owned();
        self.save_sound_index(&idx)
    }

    /// Remove a sound from the index and delete its audio file. Built-in sounds are read-only.
    /// Returns the profiles that still reference it, so the caller can warn or clean up.
    pub fn remove_sound(&self, id: &SoundId) -> Result<Vec<ProfileId>> {
        let mut idx = self.load_sound_index()?.value;
        let pos = idx
            .sounds
            .iter()
            .position(|s| &s.id == id)
            .ok_or_else(|| StoreError::NotFound(format!("sound {}", id.0)))?;
        if idx.sounds[pos].source == spank_core::SoundSource::Builtin {
            return Err(StoreError::ReadOnly(format!("sound {}", id.0)));
        }
        idx.sounds.remove(pos);
        self.save_sound_index(&idx)?;
        let file = self.sound_file(id)?;
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err(&file)(e)),
        }
        let (profiles, _) = self.list_profiles()?;
        Ok(profiles
            .into_iter()
            .filter(|p| p.referenced_sounds().contains(&id))
            .map(|p| p.id)
            .collect())
    }
}

fn parse_profile(bytes: &[u8]) -> std::result::Result<Profile, String> {
    let p: Profile = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    p.validate().map_err(|e| e.to_string())?;
    Ok(p)
}

#[cfg(test)]
mod tests;
