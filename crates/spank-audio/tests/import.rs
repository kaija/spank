//! Fixtures: 0.3 s silence + 1.0 s 440 Hz tone at -12 dBFS, 22.05 kHz stereo (ffmpeg-generated),
//! plus a 6-channel 16 kHz tone. Regenerate with the ffmpeg commands in tests/fixtures/README.md.

use spank_audio::import::{self, MAX_TRUE_PEAK_DBTP, TARGET_LUFS};
use spank_audio::{ImportError, ImportedSound, import_file, read_wav, sha256_file, write_wav};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Integrated loudness via an independent ebur128 pass over the output.
fn measured_lufs(s: &ImportedSound) -> f64 {
    let mut m = ebur128::EbuR128::new(s.channels as u32, s.sample_rate, ebur128::Mode::I).unwrap();
    let mut fed = 0;
    while fed < 48_000 * 4 / 10 {
        m.add_frames_f32(&s.samples).unwrap();
        fed += s.frames();
    }
    m.loudness_global().unwrap()
}

fn sine(rate: u32, secs: f32, channels: usize, amp: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    (0..n)
        .flat_map(|i| {
            std::iter::repeat_n(
                amp * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin(),
                channels,
            )
        })
        .collect()
}

#[test]
fn imports_every_supported_format() {
    for name in [
        "tone.wav",
        "tone.mp3",
        "tone.m4a",
        "tone.aac",
        "tone.flac",
        "tone.ogg",
    ] {
        let s = import_file(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(s.sample_rate, 48_000, "{name}");
        assert_eq!(s.source_rate, 22_050, "{name}");
        assert_eq!(s.channels, 2, "{name}");
        assert!(!s.truncated, "{name}");
        // Encoders add some priming silence, so allow slack on the trimmed lead and length.
        assert!(
            (250..=400).contains(&s.trimmed_leading_ms),
            "{name}: trimmed {} ms",
            s.trimmed_leading_ms
        );
        assert!(
            (950..=1150).contains(&s.duration_ms()),
            "{name}: duration {} ms",
            s.duration_ms()
        );
        let lufs = measured_lufs(&s);
        assert!((lufs - TARGET_LUFS).abs() <= 0.5, "{name}: {lufs} LUFS");
        assert!(
            (s.loudness_lufs - TARGET_LUFS).abs() <= 0.5,
            "{name}: reported {} LUFS",
            s.loudness_lufs
        );
        assert!(
            s.true_peak_dbtp <= MAX_TRUE_PEAK_DBTP + 0.1,
            "{name}: {} dBTP",
            s.true_peak_dbtp
        );
    }
}

#[test]
fn surround_is_mixed_to_mono() {
    // ffmpeg upmixes the mono sine into the center channel only; it must survive the downmix.
    let s = import_file(&fixture("surround.wav")).unwrap();
    assert_eq!(s.channels, 1);
    assert_eq!(s.source_rate, 16_000);
    assert!((measured_lufs(&s) - TARGET_LUFS).abs() <= 0.5);
}

#[test]
fn rejects_files_over_5_mb() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("big.wav");
    std::fs::write(&p, vec![0u8; 5 * 1024 * 1024 + 1]).unwrap();
    assert!(matches!(import_file(&p), Err(ImportError::TooLarge(_))));
}

#[test]
fn rejects_garbage_and_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("noise.mp3");
    std::fs::write(&p, b"definitely not audio, just some bytes pretending").unwrap();
    assert!(matches!(import_file(&p), Err(ImportError::Unsupported(_))));
    assert!(matches!(
        import_file(&dir.path().join("missing.wav")),
        Err(ImportError::Io(_))
    ));
}

#[test]
fn silent_input_is_rejected() {
    assert!(matches!(
        import::process(vec![0.0; 48_000], 1, 48_000),
        Err(ImportError::Silent)
    ));
    assert!(matches!(
        import::process(vec![0.001; 48_000], 1, 48_000),
        Err(ImportError::Silent)
    ));
}

#[test]
fn long_input_is_truncated_to_10_s() {
    let s = import::process(sine(48_000, 12.0, 1, 0.3), 1, 48_000).unwrap();
    assert!(s.truncated);
    assert_eq!(s.duration_ms(), 10_000);
}

#[test]
fn short_clip_is_normalized() {
    // A 150 ms "ow" is shorter than one 400 ms loudness block.
    let s = import::process(sine(44_100, 0.15, 1, 0.1), 1, 44_100).unwrap();
    assert_eq!(s.channels, 1);
    assert!((140..=160).contains(&s.duration_ms()), "{}", s.duration_ms());
    assert!(
        (measured_lufs(&s) - TARGET_LUFS).abs() <= 0.5,
        "{}",
        measured_lufs(&s)
    );
}

#[test]
fn loud_peaky_input_is_peak_limited() {
    // A short click train: very high peak-to-loudness ratio, so reaching -16 LUFS would clip.
    let mut samples = vec![0.0f32; 48_000];
    for i in (0..48_000).step_by(4_800) {
        samples[i] = 0.9;
    }
    let s = import::process(samples, 1, 48_000).unwrap();
    assert!(
        s.true_peak_dbtp <= MAX_TRUE_PEAK_DBTP + 0.01,
        "{}",
        s.true_peak_dbtp
    );
    assert!(
        s.loudness_lufs < TARGET_LUFS,
        "limited below target: {}",
        s.loudness_lufs
    );
    assert!(s.samples.iter().all(|x| x.abs() <= 1.0));
}

#[test]
fn quiet_input_is_boosted() {
    let s = import::process(sine(48_000, 1.0, 2, 0.01), 2, 48_000).unwrap();
    assert!(s.gain_db > 20.0, "{}", s.gain_db);
    assert!((measured_lufs(&s) - TARGET_LUFS).abs() <= 0.5);
}

#[test]
fn invalid_pcm_parameters_are_rejected() {
    assert!(matches!(
        import::process(vec![0.5; 10], 0, 48_000),
        Err(ImportError::Processing(_))
    ));
    assert!(matches!(
        import::process(vec![0.5; 10], 1, 0),
        Err(ImportError::Processing(_))
    ));
}

#[test]
fn wav_round_trip_and_hash() {
    let s = import_file(&fixture("tone.flac")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("glass-01.wav");
    let sha = write_wav(&s, &out).unwrap();
    assert_eq!(sha.len(), 64);
    assert_eq!(sha, sha256_file(&out).unwrap());
    assert!(!dir.path().join("glass-01.wav.tmp").exists());

    let (samples, channels, rate) = read_wav(&out).unwrap();
    assert_eq!((channels, rate), (2, 48_000));
    assert_eq!(samples.len(), s.samples.len());
    let max_err = samples
        .iter()
        .zip(&s.samples)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(max_err < 1e-3, "16-bit quantization error {max_err}");

    // The stored file imports cleanly again (already at target loudness, no lead to trim).
    let again = import_file(&out).unwrap();
    assert!(again.gain_db.abs() < 0.5);
    assert_eq!(again.trimmed_leading_ms, 0);
}

#[test]
fn encode_wav_matches_write_wav() {
    let s = spank_audio::import::process([0.1, -0.2, 0.3, 0.0].repeat(12_000), 1, 48_000).unwrap();
    let bytes = spank_audio::encode_wav(&s);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("x.wav");
    spank_audio::write_wav(&s, &p).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), bytes);
}
