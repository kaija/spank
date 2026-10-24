use super::*;
use spank_core::{EventKind, SlapLevels, SoundSource};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("dev.spank.app")).unwrap();
    (dir, s)
}

fn entry(id: &str, source: SoundSource) -> SoundEntry {
    SoundEntry {
        id: id.into(),
        name: format!("Sound {id}"),
        source,
        duration_ms: 1200,
        sha256: "00".repeat(32),
        license: Some("CC0-1.0".into()),
        added_unix_ms: 1,
    }
}

fn put_wav(s: &Store, id: &str) {
    std::fs::write(s.sound_file(&id.into()).unwrap(), b"RIFF").unwrap();
}

fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn open_creates_layout() {
    let (_d, s) = store();
    assert!(s.root().join("profiles").is_dir());
    assert!(s.sounds_dir().is_dir());
    assert!(
        Store::default_root("dev.spank.app")
            .unwrap()
            .ends_with("dev.spank.app")
    );
}

#[test]
fn missing_config_gives_defaults() {
    let (_d, s) = store();
    let l = s.load_config().unwrap();
    assert_eq!(l.value, AppConfig::default());
    assert!(l.recovered.is_none());
}

#[test]
fn config_round_trip() {
    let (_d, s) = store();
    let mut c = AppConfig {
        app_volume: 0.4,
        ..Default::default()
    };
    c.slap.enabled = true;
    s.save_config(&c).unwrap();
    assert_eq!(s.load_config().unwrap().value, c);
}

#[test]
fn save_rejects_invalid_config() {
    let (_d, s) = store();
    let c = AppConfig {
        app_volume: 3.0,
        ..Default::default()
    };
    assert!(matches!(s.save_config(&c), Err(StoreError::Invalid(_))));
    assert!(!s.config_path().exists());
}

#[test]
fn corrupt_config_is_backed_up_and_replaced() {
    let (_d, s) = store();
    std::fs::write(s.config_path(), b"{ not json").unwrap();
    let l = s.load_config().unwrap();
    assert_eq!(l.value, AppConfig::default());
    let r = l.recovered.unwrap();
    assert!(
        r.backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("config.json.corrupt-")
    );
    assert_eq!(std::fs::read(&r.backup).unwrap(), b"{ not json");
    assert!(!s.config_path().exists());
}

#[test]
fn invalid_or_future_config_is_recovered() {
    let (_d, s) = store();
    std::fs::write(s.config_path(), br#"{"schema_version":1,"app_volume":7}"#).unwrap();
    assert!(s.load_config().unwrap().recovered.is_some());
    std::fs::write(s.config_path(), br#"{"schema_version":99}"#).unwrap();
    let r = s.load_config().unwrap().recovered.unwrap();
    assert!(r.reason.contains("newer"), "{}", r.reason);
}

#[test]
fn calibration_round_trip() {
    let (_d, s) = store();
    assert_eq!(s.load_calibration().unwrap().value, None);
    let c = Calibration {
        levels: SlapLevels {
            light_g: 0.25,
            medium_g: 0.4,
            hard_g: 0.65,
        },
        noise_floor_g: 0.05,
        calibrated_unix_ms: 42,
    };
    s.save_calibration(&c).unwrap();
    assert_eq!(s.load_calibration().unwrap().value, Some(c));
    std::fs::write(s.calibration_path(), b"[]").unwrap();
    let l = s.load_calibration().unwrap();
    assert_eq!(l.value, None);
    assert!(l.recovered.is_some());
}

#[test]
fn profile_crud() {
    let (_d, s) = store();
    let mut p = Profile::new("office", "辦公室安靜版");
    p.mapping.get_mut(&EventKind::SlapHard).unwrap().sounds = vec!["ow".into()];
    s.save_profile(&p).unwrap();
    s.save_profile(&Profile::new("a-first", "A first")).unwrap();
    assert_eq!(s.load_profile(&"office".into()).unwrap(), p);

    let (list, bad) = s.list_profiles().unwrap();
    assert!(bad.is_empty());
    assert_eq!(
        list.iter().map(|p| p.id.0.as_str()).collect::<Vec<_>>(),
        vec!["a-first", "office"]
    );

    s.delete_profile(&"office".into()).unwrap();
    assert!(matches!(
        s.load_profile(&"office".into()),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        s.delete_profile(&"office".into()),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn builtin_and_invalid_profiles_are_rejected() {
    let (_d, s) = store();
    let mut p = Profile::new("builtin.destruction", "毀滅");
    p.builtin = true;
    assert!(matches!(s.save_profile(&p), Err(StoreError::ReadOnly(_))));
    let bad = Profile::new("../escape", "x");
    assert!(matches!(s.save_profile(&bad), Err(StoreError::InvalidId(_))));
    let mut loud = Profile::new("loud", "Loud");
    loud.volume = 2.0;
    assert!(matches!(s.save_profile(&loud), Err(StoreError::Invalid(_))));
}

#[test]
fn list_profiles_quarantines_broken_files() {
    let (_d, s) = store();
    s.save_profile(&Profile::new("ok", "OK")).unwrap();
    std::fs::write(s.root().join("profiles/broken.json"), b"nope").unwrap();
    std::fs::write(s.root().join("profiles/notes.txt"), b"ignored").unwrap();
    let (list, bad) = s.list_profiles().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(bad.len(), 1);
    let names = files(&s.root().join("profiles"));
    assert!(names.iter().any(|n| n.starts_with("broken.json.corrupt-")));
    assert!(names.contains(&"notes.txt".to_string()));
}

#[test]
fn id_validation() {
    for good in [
        "office",
        "builtin.destruction",
        "a_b-c.1",
        "550e8400-e29b-41d4-a716-446655440000",
    ] {
        assert!(validate_id(good).is_ok(), "{good}");
    }
    for bad in ["", ".hidden", "../x", "a/b", "a b", "名", &"x".repeat(129)] {
        assert!(validate_id(bad).is_err(), "{bad}");
    }
}

#[test]
fn sound_index_crud() {
    let (_d, s) = store();
    assert!(s.load_sound_index().unwrap().value.sounds.is_empty());

    // The audio file must exist before registering.
    assert!(matches!(
        s.add_sound(entry(
            "glass-01",
            SoundSource::User {
                original_name: "glass.mp3".into()
            }
        )),
        Err(StoreError::NotFound(_))
    ));
    put_wav(&s, "glass-01");
    s.add_sound(entry(
        "glass-01",
        SoundSource::User {
            original_name: "glass.mp3".into(),
        },
    ))
    .unwrap();
    assert!(matches!(
        s.add_sound(entry("glass-01", SoundSource::Builtin)),
        Err(StoreError::AlreadyExists(_))
    ));

    s.rename_sound(&"glass-01".into(), "  玻璃碎裂 01 ").unwrap();
    assert_eq!(
        s.load_sound_index()
            .unwrap()
            .value
            .get(&"glass-01".into())
            .unwrap()
            .name,
        "玻璃碎裂 01"
    );
    assert!(s.rename_sound(&"glass-01".into(), "   ").is_err());
    assert!(matches!(
        s.rename_sound(&"nope".into(), "x"),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn remove_sound_deletes_file_and_reports_users() {
    let (_d, s) = store();
    put_wav(&s, "ow");
    s.add_sound(entry(
        "ow",
        SoundSource::User {
            original_name: "ow.wav".into(),
        },
    ))
    .unwrap();
    let mut p = Profile::new("cry", "哀哀叫");
    p.mapping.get_mut(&EventKind::SlapLight).unwrap().sounds = vec!["ow".into()];
    s.save_profile(&p).unwrap();
    s.save_profile(&Profile::new("other", "Other")).unwrap();

    let users = s.remove_sound(&"ow".into()).unwrap();
    assert_eq!(users, vec![ProfileId::from("cry")]);
    assert!(!s.sound_file(&"ow".into()).unwrap().exists());
    assert!(s.load_sound_index().unwrap().value.sounds.is_empty());
    assert!(matches!(
        s.remove_sound(&"ow".into()),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn builtin_sounds_are_read_only() {
    let (_d, s) = store();
    put_wav(&s, "builtin.glass");
    s.add_sound(entry("builtin.glass", SoundSource::Builtin)).unwrap();
    assert!(matches!(
        s.remove_sound(&"builtin.glass".into()),
        Err(StoreError::ReadOnly(_))
    ));
}

#[test]
fn bad_sound_index_is_recovered() {
    let (_d, s) = store();
    std::fs::write(s.sound_index_path(), br#"{"schema_version":5,"sounds":[]}"#).unwrap();
    let l = s.load_sound_index().unwrap();
    assert!(l.value.sounds.is_empty());
    assert!(l.recovered.unwrap().reason.contains("schema_version"));
}

#[test]
fn sound_file_rejects_unsafe_ids() {
    let (_d, s) = store();
    assert!(s.sound_file(&"../../etc/passwd".into()).is_err());
}
