use super::*;
use std::io::Cursor;

fn wav_bytes(rate: u32, channels: u16, frames: usize) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::new(&mut c, spec).unwrap();
    for i in 0..frames * channels as usize {
        w.write_sample(((i % 100) as i16 - 50) * 100).unwrap();
    }
    w.finalize().unwrap();
    c.into_inner()
}

fn sound(file: &str, bytes: &[u8], events: &[EventKind]) -> ManifestSound {
    ManifestSound {
        file: file.into(),
        sha256: sha256_hex(bytes),
        duration_ms: 100,
        suggested_events: events.iter().map(|&k| PackEvent(k)).collect(),
        license: None,
        source_url: None,
    }
}

fn sample_pack() -> Pack {
    let a = wav_bytes(48_000, 1, 4800);
    let b = wav_bytes(48_000, 2, 9600);
    Pack {
        manifest: Manifest {
            schema_version: 1,
            id: "com.example.glass".into(),
            name: BTreeMap::from([("en".into(), "Glass".into()), ("zh-Hant".into(), "玻璃".into())]),
            version: "1.0.0".into(),
            author: Some("someone".into()),
            license: "CC0-1.0".into(),
            sounds: vec![
                sound("sounds/a.wav", &a, &[EventKind::LidOpened]),
                sound("sounds/b.wav", &b, &[EventKind::SlapHard]),
            ],
            profile: Some(BTreeMap::from([
                (PackEvent(EventKind::LidOpened), vec!["sounds/a.wav".into()]),
                (
                    PackEvent(EventKind::SlapHard),
                    vec!["sounds/a.wav".into(), "sounds/b.wav".into()],
                ),
            ])),
        },
        sounds: BTreeMap::from([("sounds/a.wav".into(), a), ("sounds/b.wav".into(), b)]),
        icon: Some(vec![0x89, b'P', b'N', b'G']),
    }
}

fn zip_bytes(pack: &Pack) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    write_pack(&mut c, pack).unwrap();
    c.into_inner()
}

/// A zip with arbitrary entries (name, bytes), plus the sample manifest unless `manifest` is None.
fn raw_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(&mut c);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        z.start_file(*name, opts).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap();
    c.into_inner()
}

fn read(bytes: &[u8]) -> Result<Pack, PackError> {
    read_pack(Cursor::new(bytes))
}

#[test]
fn round_trip() {
    let p = sample_pack();
    let back = read(&zip_bytes(&p)).unwrap();
    assert_eq!(back, p);
}

#[test]
fn rejects_unsafe_paths() {
    for bad in [
        "../evil.wav",
        "/abs.wav",
        "sounds/../../x.wav",
        "a\\b",
        "sounds/./a.wav",
        "C:/x",
    ] {
        let z = raw_zip(&[(bad, b"x")]);
        assert!(matches!(read(&z), Err(PackError::UnsafePath(_))), "{bad}");
    }
    let z = raw_zip(&[("sounds/sub/a.wav", b"x")]);
    assert!(matches!(read(&z), Err(PackError::UnsafePath(_))));
    let z = raw_zip(&[("sounds/a.mp3", b"x")]);
    assert!(matches!(read(&z), Err(PackError::UnsafePath(_))));
}

#[test]
fn rejects_symlinks() {
    let mut c = Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(&mut c);
    z.add_symlink(
        "sounds/a.wav",
        "/etc/passwd",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    z.finish().unwrap();
    assert!(matches!(read(&c.into_inner()), Err(PackError::UnsafePath(_))));
}

#[test]
fn rejects_oversized_and_bombs() {
    // 6 MB of zeros compresses to a few KB but is over the 5 MB sound limit.
    let big = vec![0u8; 6 * 1024 * 1024];
    let z = raw_zip(&[("sounds/big.wav", &big)]);
    assert!(z.len() < 100_000);
    assert!(matches!(read(&z), Err(PackError::TooLarge(_))));
    // Many 4 MB entries exceed the 50 MB total.
    let chunk = vec![0u8; 4 * 1024 * 1024];
    let names: Vec<String> = (0..14).map(|i| format!("sounds/{i}.wav")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), chunk.as_slice())).collect();
    assert!(matches!(read(&raw_zip(&entries)), Err(PackError::TooLarge(_))));
    // Too many entries.
    let names: Vec<String> = (0..600).map(|i| format!("x{i}")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
    assert!(matches!(read(&raw_zip(&entries)), Err(PackError::TooLarge(_))));
}

#[test]
fn rejects_bad_manifests() {
    let check = |f: &dyn Fn(&mut Pack), expect: &str| {
        let mut p = sample_pack();
        f(&mut p);
        let err = read(&zip_bytes(&p)).unwrap_err().to_string();
        assert!(err.contains(expect), "{err} should contain {expect}");
    };
    check(&|p| p.manifest.schema_version = 2, "schema_version");
    check(&|p| p.manifest.id = "Bad Id".into(), "invalid pack id");
    check(&|p| p.manifest.license = " ".into(), "license");
    check(
        &|p| p.manifest.sounds[0].sha256 = "00".repeat(32),
        "sha256 mismatch",
    );
    check(&|p| p.manifest.sounds[0].sha256 = "xyz".into(), "64 hex");
    check(
        &|p| {
            let s = p.manifest.sounds[0].clone();
            p.manifest.sounds.push(s);
        },
        "listed twice",
    );
    check(
        &|p| {
            p.manifest
                .profile
                .as_mut()
                .unwrap()
                .insert(PackEvent(EventKind::SlapLight), vec!["sounds/zzz.wav".into()]);
        },
        "unknown sound",
    );
    check(
        &|p| {
            p.sounds.remove("sounds/b.wav");
        },
        "missing from the archive",
    );
    check(
        &|p| {
            p.manifest.sounds.clear();
            p.manifest.profile = None;
        },
        "no sounds",
    );
    assert!(
        read(&raw_zip(&[("sounds/a.wav", b"x")]))
            .unwrap_err()
            .to_string()
            .contains("manifest.json missing")
    );
    assert!(matches!(
        read(&raw_zip(&[("manifest.json", b"{")])),
        Err(PackError::Manifest(_))
    ));
    assert!(matches!(read(b"not a zip"), Err(PackError::Zip(_))));
}

#[test]
fn rejects_wrong_audio_format() {
    let check = |bytes: Vec<u8>, expect: &str| {
        let mut p = sample_pack();
        p.manifest.sounds[0].sha256 = sha256_hex(&bytes);
        p.sounds.insert("sounds/a.wav".into(), bytes);
        let err = read(&zip_bytes(&p)).unwrap_err().to_string();
        assert!(err.contains(expect), "{err}");
    };
    check(wav_bytes(44_100, 1, 4410), "sample rate 44100");
    check(wav_bytes(48_000, 3, 100), "3 channels");
    check(wav_bytes(48_000, 1, 48_000 * 11), "longer than 10 s");
    check(b"RIFF....".to_vec(), "not a WAV");
}

fn store() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("root")).unwrap();
    (d, s)
}

#[test]
fn import_creates_sounds_and_profile_idempotently() {
    let (_d, s) = store();
    let lib = Profiles::default();
    let pack = read(&zip_bytes(&sample_pack())).unwrap();
    let r = import_pack(&s, &lib, &pack).unwrap();
    assert_eq!(r.sounds.len(), 2);
    let idx = s.load_sound_index().unwrap().value;
    assert_eq!(idx.sounds.len(), 2);
    let a = idx.get(&r.sounds["sounds/a.wav"]).unwrap();
    assert_eq!(a.name, "a");
    assert_eq!(a.duration_ms, 100);
    assert_eq!(a.license.as_deref(), Some("CC0-1.0"));
    assert_eq!(
        a.source,
        SoundSource::Pack {
            pack_id: "com.example.glass".into()
        }
    );
    assert!(s.sound_file(&a.id).unwrap().exists());

    let p = r.profile.unwrap();
    assert_eq!(p.name, "玻璃");
    assert_eq!(p.mapping[&EventKind::SlapHard].sounds.len(), 2);
    assert!(p.mapping[&EventKind::LidOpened].enabled);
    assert!(!p.mapping[&EventKind::SlapLight].enabled);
    assert_eq!(s.load_profile(&p.id).unwrap(), p);

    // Importing again reuses the sounds (content-addressed ids) and creates a second profile.
    let r2 = import_pack(&s, &lib, &pack).unwrap();
    assert_eq!(r2.sounds, r.sounds);
    assert_eq!(s.load_sound_index().unwrap().value.sounds.len(), 2);

    let mut no_profile = pack.clone();
    no_profile.manifest.profile = None;
    assert!(import_pack(&s, &lib, &no_profile).unwrap().profile.is_none());
}

#[test]
fn export_then_import_round_trip() {
    let (_d, s) = store();
    let lib = Profiles::default();
    let imported = import_pack(&s, &lib, &sample_pack()).unwrap();
    let profile = imported.profile.unwrap();
    let meta = ExportMeta {
        id: "dev.spank.mine".into(),
        name: BTreeMap::from([("en".into(), "Mine".into())]),
        version: "0.1.0".into(),
        author: None,
        license: "CC0-1.0".into(),
    };
    let pack = export_pack(&s, &profile, meta.clone()).unwrap();
    assert_eq!(pack.sounds.len(), 2);
    let back = read(&zip_bytes(&pack)).unwrap();
    assert_eq!(back.manifest.profile.as_ref().unwrap().len(), 2);
    let hard = &back
        .manifest
        .sounds
        .iter()
        .find(|x| x.suggested_events.contains(&PackEvent(EventKind::SlapHard)))
        .unwrap();
    assert!(hard.file.starts_with("sounds/pack-"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mine.spankpack");
    write_pack(std::fs::File::create(&path).unwrap(), &pack).unwrap();
    assert_eq!(LocalFile(path).open().unwrap(), back);

    assert!(
        export_pack(
            &s,
            &profile,
            ExportMeta {
                id: "BAD".into(),
                ..meta.clone()
            }
        )
        .is_err()
    );
    let mut missing = profile.clone();
    missing.mapping.get_mut(&EventKind::SlapLight).unwrap().sounds = vec!["nope".into()];
    assert!(matches!(
        export_pack(&s, &missing, meta),
        Err(PackError::Store(_))
    ));
}

#[test]
fn pack_file_size_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.spankpack");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(MAX_PACK_FILE_BYTES + 1).unwrap();
    assert!(matches!(read_pack_file(&path), Err(PackError::TooLarge(_))));
}

/// Stable-toolchain stand-in for `cargo fuzz`: random mutations of a valid pack must never panic.
/// Stable stand-in for `cargo fuzz run read_pack` (which needs nightly). Longer rounds:
/// `SPANK_FUZZ_ITERS=500000 SPANK_FUZZ_SEED=7 cargo test --release -p spank-pack mutated_packs_never_panic`.
#[test]
fn mutated_packs_never_panic() {
    let env = |k: &str, d: u64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let base = zip_bytes(&sample_pack());
    let mut rng = fastrand::Rng::with_seed(env("SPANK_FUZZ_SEED", 0x5a4b));
    for _ in 0..env("SPANK_FUZZ_ITERS", 3000) {
        let mut b = base.clone();
        for _ in 0..rng.usize(1..8) {
            match rng.u8(..3) {
                0 => {
                    let i = rng.usize(..b.len());
                    b[i] = rng.u8(..);
                }
                1 => b.truncate(rng.usize(..b.len())),
                _ => {
                    let i = rng.usize(..b.len());
                    b.insert(i, rng.u8(..));
                }
            }
            if b.is_empty() {
                break;
            }
        }
        fuzz_read(&b);
    }
}
