//! `.spankpack` sound packs (ADR-0009): a ZIP with `manifest.json`, `sounds/*.wav` and an
//! optional `icon.png`.
//!
//! Packs are untrusted input. [`read_pack`] checks, before anything touches the store:
//! - entry count, manifest / icon / per-sound / total uncompressed sizes (declared *and* actual,
//!   so a lying header can't zip-bomb us);
//! - entry names: relative, no `..`, no backslashes, no symlinks; sounds live under `sounds/`;
//! - the manifest schema, sha256 of every sound, and that each sound is a 48 kHz mono/stereo WAV
//!   of at most 10 s (already normalized, ADR-0008);
//! - profile references point at listed sounds.

#![forbid(unsafe_code)]

pub mod manifest;

pub use manifest::{Manifest, ManifestSound, PackEvent, SCHEMA_VERSION};

use sha2::{Digest, Sha256};
use spank_core::{EventBinding, EventKind, Profile, SoundEntry, SoundId, SoundSource};
use spank_store::{Profiles, Store, StoreError, write_atomic};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

pub const MAX_PACK_FILE_BYTES: u64 = 60 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_SOUND_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
pub const MAX_ICON_BYTES: u64 = 1024 * 1024;
pub const MAX_ENTRIES: usize = 512;
pub const MAX_DURATION_MS: u32 = 10_000;
pub const SAMPLE_RATE: u32 = 48_000;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("not a valid pack archive: {0}")]
    Zip(String),
    #[error("pack is too large: {0}")]
    TooLarge(String),
    #[error("unsafe path in pack: {0:?}")]
    UnsafePath(String),
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("sound {file}: {msg}")]
    Sound { file: String, msg: String },
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A validated pack in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub manifest: Manifest,
    /// file → WAV bytes, for every manifest sound.
    pub sounds: BTreeMap<String, Vec<u8>>,
    pub icon: Option<Vec<u8>>,
}

/// Where packs come from (ADR-0009). Phase 2 adds a remote catalog.
pub trait PackSource {
    fn open(&self) -> Result<Pack, PackError>;
}

pub struct LocalFile(pub PathBuf);

impl PackSource for LocalFile {
    fn open(&self) -> Result<Pack, PackError> {
        read_pack_file(&self.0)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Archive entry names must be plain relative paths.
fn check_path(name: &str) -> Result<(), PackError> {
    let bad = name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.contains(':')
        || name.split('/').any(|c| c == ".." || c == ".");
    if bad {
        Err(PackError::UnsafePath(name.to_owned()))
    } else {
        Ok(())
    }
}

fn check_sound_path(name: &str) -> Result<(), PackError> {
    check_path(name)?;
    let rest = name
        .strip_prefix("sounds/")
        .ok_or_else(|| PackError::UnsafePath(name.to_owned()))?;
    if rest.is_empty() || rest.contains('/') || !rest.to_ascii_lowercase().ends_with(".wav") {
        return Err(PackError::UnsafePath(name.to_owned()));
    }
    Ok(())
}

fn check_id(id: &str) -> Result<(), PackError> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'));
    if ok {
        Ok(())
    } else {
        Err(PackError::Manifest(format!("invalid pack id {id:?}")))
    }
}

/// Duration of a WAV after checking it is 48 kHz, 1–2 channels, ≤ 10 s.
fn wav_duration_ms(file: &str, bytes: &[u8]) -> Result<u32, PackError> {
    let err = |msg: String| PackError::Sound {
        file: file.to_owned(),
        msg,
    };
    let r = hound::WavReader::new(Cursor::new(bytes)).map_err(|e| err(format!("not a WAV file: {e}")))?;
    let spec = r.spec();
    if spec.sample_rate != SAMPLE_RATE {
        return Err(err(format!("sample rate {} (expected 48000)", spec.sample_rate)));
    }
    if !(1..=2).contains(&spec.channels) {
        return Err(err(format!("{} channels (expected 1 or 2)", spec.channels)));
    }
    let ms = r.duration() as u64 * 1000 / SAMPLE_RATE as u64;
    if ms > MAX_DURATION_MS as u64 {
        return Err(err(format!("{ms} ms is longer than 10 s")));
    }
    Ok(ms as u32)
}

/// Read up to `limit` bytes; more than that is an error even if the header said otherwise.
fn read_limited(r: impl Read, limit: u64, what: &str) -> Result<Vec<u8>, PackError> {
    let mut buf = Vec::new();
    r.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(PackError::TooLarge(format!("{what} is over {limit} bytes")));
    }
    Ok(buf)
}

pub fn read_pack_file(path: &Path) -> Result<Pack, PackError> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_PACK_FILE_BYTES {
        return Err(PackError::TooLarge(format!("file is {size} bytes")));
    }
    read_pack(std::fs::File::open(path)?)
}

/// Parse and fully validate a pack.
pub fn read_pack(r: impl Read + Seek) -> Result<Pack, PackError> {
    let zip_err = |e: zip::result::ZipError| PackError::Zip(e.to_string());
    let mut zip = zip::ZipArchive::new(r).map_err(zip_err)?;
    if zip.len() > MAX_ENTRIES {
        return Err(PackError::TooLarge(format!("{} entries", zip.len())));
    }

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(zip_err)?;
        let name = entry.name().map_err(zip_err)?.into_owned();
        check_path(name.trim_end_matches('/'))?;
        if entry.is_symlink() {
            return Err(PackError::UnsafePath(name));
        }
        if entry.is_dir() {
            continue;
        }
        let limit = match name.as_str() {
            "manifest.json" => MAX_MANIFEST_BYTES,
            "icon.png" => MAX_ICON_BYTES,
            n if n.starts_with("sounds/") => {
                check_sound_path(n)?;
                MAX_SOUND_BYTES
            }
            // Unknown extra files are ignored but still count toward the total.
            _ => MAX_SOUND_BYTES,
        };
        if entry.size() > limit {
            return Err(PackError::TooLarge(format!(
                "{name} declares {} bytes",
                entry.size()
            )));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_TOTAL_BYTES {
            return Err(PackError::TooLarge("total uncompressed size over 50 MB".into()));
        }
        let bytes = read_limited(entry, limit, &name)?;
        if files.insert(name.clone(), bytes).is_some() {
            return Err(PackError::Zip(format!("duplicate entry {name}")));
        }
    }

    let manifest_bytes = files
        .remove("manifest.json")
        .ok_or_else(|| PackError::Manifest("manifest.json missing".into()))?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| PackError::Manifest(e.to_string()))?;
    validate_manifest(&manifest)?;

    let mut sounds = BTreeMap::new();
    for s in &manifest.sounds {
        let bytes = files.remove(&s.file).ok_or_else(|| PackError::Sound {
            file: s.file.clone(),
            msg: "listed in manifest but missing from the archive".into(),
        })?;
        if sha256_hex(&bytes) != s.sha256.to_ascii_lowercase() {
            return Err(PackError::Sound {
                file: s.file.clone(),
                msg: "sha256 mismatch".into(),
            });
        }
        wav_duration_ms(&s.file, &bytes)?;
        sounds.insert(s.file.clone(), bytes);
    }
    let icon = files.remove("icon.png");
    Ok(Pack {
        manifest,
        sounds,
        icon,
    })
}

fn validate_manifest(m: &Manifest) -> Result<(), PackError> {
    if m.schema_version != SCHEMA_VERSION {
        return Err(PackError::Manifest(format!(
            "unsupported schema_version {}",
            m.schema_version
        )));
    }
    check_id(&m.id)?;
    if m.version.trim().is_empty() || m.license.trim().is_empty() {
        return Err(PackError::Manifest("version and license are required".into()));
    }
    if m.sounds.is_empty() {
        return Err(PackError::Manifest("pack has no sounds".into()));
    }
    let mut seen = BTreeSet::new();
    for s in &m.sounds {
        check_sound_path(&s.file)?;
        if !seen.insert(s.file.as_str()) {
            return Err(PackError::Manifest(format!("{} listed twice", s.file)));
        }
        if s.sha256.len() != 64 || !s.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(PackError::Manifest(format!(
                "{}: sha256 must be 64 hex chars",
                s.file
            )));
        }
    }
    if let Some(profile) = &m.profile {
        for files in profile.values() {
            if let Some(f) = files.iter().find(|f| !seen.contains(f.as_str())) {
                return Err(PackError::Manifest(format!(
                    "profile references unknown sound {f}"
                )));
            }
        }
    }
    Ok(())
}

/// Write a pack (deflate-compressed ZIP).
pub fn write_pack(w: impl Write + Seek, pack: &Pack) -> Result<(), PackError> {
    let zip_err = |e: zip::result::ZipError| PackError::Zip(e.to_string());
    let mut z = zip::ZipWriter::new(w);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("manifest.json", opts).map_err(zip_err)?;
    z.write_all(&serde_json::to_vec_pretty(&pack.manifest).map_err(|e| PackError::Manifest(e.to_string()))?)?;
    for (file, bytes) in &pack.sounds {
        check_sound_path(file)?;
        z.start_file(file.as_str(), opts).map_err(zip_err)?;
        z.write_all(bytes)?;
    }
    if let Some(icon) = &pack.icon {
        z.start_file("icon.png", opts).map_err(zip_err)?;
        z.write_all(icon)?;
    }
    z.finish().map_err(zip_err)?;
    Ok(())
}

/// What an import added to the library.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    /// pack file → sound id in the library (existing ids are reused for identical audio).
    pub sounds: BTreeMap<String, SoundId>,
    pub profile: Option<Profile>,
}

/// Library id for a sound: derived from its content, so re-importing the same pack is idempotent.
fn sound_id_for(sha256: &str) -> SoundId {
    SoundId(format!("pack-{}", &sha256[..16]))
}

fn file_stem(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_owned())
}

/// Add a validated pack's sounds to the library and, if it has one, create its profile.
pub fn import_pack(store: &Store, profiles: &Profiles, pack: &Pack) -> Result<ImportReport, PackError> {
    let index = store.load_sound_index()?.value;
    let mut sounds = BTreeMap::new();
    for s in &pack.manifest.sounds {
        let bytes = &pack.sounds[&s.file];
        let sha = sha256_hex(bytes);
        let id = sound_id_for(&sha);
        if index.get(&id).is_none_or(|e| e.sha256 != sha) {
            let path = store.sound_file(&id)?;
            write_atomic(&path, bytes)?;
            if index.get(&id).is_none() {
                store.add_sound(SoundEntry {
                    id: id.clone(),
                    name: file_stem(&s.file),
                    source: SoundSource::Pack {
                        pack_id: pack.manifest.id.clone(),
                    },
                    duration_ms: wav_duration_ms(&s.file, bytes)?,
                    sha256: sha,
                    license: s.license.clone().or_else(|| Some(pack.manifest.license.clone())),
                    added_unix_ms: unix_ms(),
                })?;
            }
        }
        sounds.insert(s.file.clone(), id);
    }
    let profile = match &pack.manifest.profile {
        None => None,
        Some(map) => {
            let mut p = profiles.create(store, pack.manifest.display_name())?;
            for k in EventKind::ALL {
                let files = map.get(&PackEvent(k)).map(Vec::as_slice).unwrap_or(&[]);
                p.mapping.insert(
                    k,
                    EventBinding {
                        enabled: !files.is_empty(),
                        sounds: files.iter().map(|f| sounds[f].clone()).collect(),
                    },
                );
            }
            profiles.save(store, &p)?;
            Some(p)
        }
    };
    Ok(ImportReport { sounds, profile })
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pack metadata chosen by the user when exporting.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportMeta {
    pub id: String,
    pub name: BTreeMap<String, String>,
    pub version: String,
    pub author: Option<String>,
    pub license: String,
}

/// Build a pack from a profile and the sounds it uses.
pub fn export_pack(store: &Store, profile: &Profile, meta: ExportMeta) -> Result<Pack, PackError> {
    check_id(&meta.id)?;
    let index = store.load_sound_index()?.value;
    let mut sounds = BTreeMap::new();
    let mut entries = Vec::new();
    let mut file_of: BTreeMap<SoundId, String> = BTreeMap::new();
    for id in profile.referenced_sounds() {
        let entry = index
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("sound {}", id.0)))?;
        let bytes = std::fs::read(store.sound_file(id)?)?;
        let file = format!("sounds/{}.wav", id.0);
        let suggested = profile
            .mapping
            .iter()
            .filter(|(_, b)| b.sounds.contains(id))
            .map(|(k, _)| PackEvent(*k))
            .collect();
        entries.push(ManifestSound {
            file: file.clone(),
            sha256: sha256_hex(&bytes),
            duration_ms: wav_duration_ms(&file, &bytes)?,
            suggested_events: suggested,
            license: entry.license.clone(),
            source_url: None,
        });
        file_of.insert(id.clone(), file.clone());
        sounds.insert(file, bytes);
    }
    let mapping: BTreeMap<PackEvent, Vec<String>> = profile
        .mapping
        .iter()
        .filter(|(_, b)| b.is_playable())
        .map(|(k, b)| {
            (
                PackEvent(*k),
                b.sounds.iter().map(|s| file_of[s].clone()).collect(),
            )
        })
        .collect();
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        id: meta.id,
        name: meta.name,
        version: meta.version,
        author: meta.author,
        license: meta.license,
        sounds: entries,
        profile: Some(mapping),
    };
    validate_manifest(&manifest)?;
    Ok(Pack {
        manifest,
        sounds,
        icon: None,
    })
}

/// Fuzzing entry point (`fuzz/`): must never panic on any input.
pub fn fuzz_read(data: &[u8]) {
    let _ = read_pack(Cursor::new(data));
}

#[cfg(test)]
mod tests;
