//! T35: synthesize the built-in sounds and write the three built-in profiles as `.spankpack` files.
//!
//!   cargo run --release -p gen-builtin-sounds [-- --out assets/sounds/builtin]
//!
//! Every sound is generated from code with a fixed seed (no recordings), normalized by the normal
//! import pipeline (-16 LUFS, ≤ -1 dBTP), and dedicated to the public domain (CC0-1.0). Running it
//! again produces identical files. `assets/sounds/SOUNDS_LICENSE.md` is rewritten too.

mod synth;

use spank_core::EventKind;
use spank_pack::{Manifest, ManifestSound, Pack, PackEvent, write_pack};
use std::collections::BTreeMap;
use std::path::PathBuf;
use synth::*;

const SEED: u64 = 0x5_9a7c;

struct Sound {
    stem: &'static str,
    zh: &'static str,
    en: &'static str,
    what: &'static str,
    make: fn(&mut fastrand::Rng) -> Vec<f32>,
}

fn sounds() -> Vec<Sound> {
    vec![
        Sound {
            stem: "glass-01",
            zh: "玻璃碎裂 01",
            en: "Glass shatter 01",
            what: "noise burst + 40 decaying high partials",
            make: |r| glass(r, 0.8, 40, 0.08),
        },
        Sound {
            stem: "glass-02",
            zh: "玻璃碎裂 02",
            en: "Glass shatter 02",
            what: "noise burst + 70 shards spread over ~1 s",
            make: |r| glass(r, 1.1, 70, 0.16),
        },
        Sound {
            stem: "glass-crash",
            zh: "玻璃大爆炸",
            en: "Glass avalanche",
            what: "three overlapping shatters",
            make: |r| {
                let mut out = glass(r, 1.6, 60, 0.12);
                for (at, n) in [(0.25, 50), (0.55, 80)] {
                    let g = glass(r, 1.0, n, 0.15);
                    for (i, v) in g.iter().enumerate() {
                        if let Some(o) = out.get_mut(len(at) + i) {
                            *o += v * 0.8;
                        }
                    }
                }
                out
            },
        },
        Sound {
            stem: "metal-snap",
            zh: "金屬斷裂",
            en: "Metal snap",
            what: "click + inharmonic bar modes (310 Hz)",
            make: |r| metal_snap(r, 310.0, 1.4),
        },
        Sound {
            stem: "metal-bend",
            zh: "鋼筋彎折",
            en: "Rebar bending",
            what: "slow stick-slip groan through metal resonances, ending in a ping",
            make: |r| {
                let mut out = friction(
                    r,
                    1.6,
                    |t| 45.0 + 25.0 * (t * 2.2).sin() + 30.0 * t,
                    |t| ahr(t, 0.15, 1.1, 0.12),
                    &[(420.0, 9.0, 1.0), (1150.0, 12.0, 0.6), (2600.0, 15.0, 0.25)],
                );
                let ping = metal_snap(r, 520.0, 0.5);
                for (i, v) in ping.iter().enumerate() {
                    if let Some(o) = out.get_mut(len(1.1) + i) {
                        *o += v * 0.3;
                    }
                }
                out
            },
        },
        Sound {
            stem: "thud-low",
            zh: "悶響 · 低",
            en: "Low thud",
            what: "falling 90 → 45 Hz sine + low-passed noise",
            make: |r| thud(r, 90.0, 45.0, 0.6),
        },
        Sound {
            stem: "creak",
            zh: "吱嘎門軸",
            en: "Door creak",
            what: "stick-slip friction with a sweeping rate through wood resonances",
            make: |r| {
                friction(
                    r,
                    1.5,
                    |t| 18.0 + 70.0 * (PI_F * t / 1.5).sin().powi(2),
                    |t| ahr(t, 0.1, 1.2, 0.1),
                    &[(850.0, 14.0, 1.0), (1700.0, 16.0, 0.7), (3100.0, 18.0, 0.35)],
                )
            },
        },
        Sound {
            stem: "oh",
            zh: "喔！",
            en: "Oh!",
            what: "formant voice, vowel 'o', falling pitch",
            make: |r| voice(r, 0.4, |t| 260.0 - 120.0 * t, |_| O, |t| ahr(t, 0.02, 0.18, 0.06)),
        },
        Sound {
            stem: "hmm",
            zh: "嗯？",
            en: "Hm?",
            what: "formant voice, nasal 'm', rising pitch",
            make: |r| {
                voice(
                    r,
                    0.45,
                    |t| 150.0 + 220.0 * t * t,
                    |_| M,
                    |t| ahr(t, 0.03, 0.3, 0.05),
                )
            },
        },
        Sound {
            stem: "aiyo",
            zh: "哎唷",
            en: "Ouch",
            what: "formant voice, 'a' → 'i' → 'o' with a pitch drop",
            make: |r| {
                voice(
                    r,
                    0.6,
                    |t| {
                        if t < 0.22 {
                            300.0
                        } else {
                            300.0 - 260.0 * (t - 0.22)
                        }
                    },
                    |t| {
                        if t < 0.2 {
                            glide(A, I, t, 0.12, 0.2)
                        } else {
                            glide(I, O, t, 0.2, 0.3)
                        }
                    },
                    |t| ahr(t, 0.015, 0.42, 0.07) * if (0.19..0.24).contains(&t) { 0.55 } else { 1.0 },
                )
            },
        },
        Sound {
            stem: "whimper",
            zh: "嗚…",
            en: "Whimper",
            what: "formant voice, vowel 'u', slow falling pitch with wobble",
            make: |r| {
                voice(
                    r,
                    0.9,
                    |t| 330.0 - 140.0 * t + 12.0 * (t * 38.0).sin(),
                    |_| U,
                    |t| ahr(t, 0.08, 0.55, 0.15) * 0.8,
                )
            },
        },
        Sound {
            stem: "scream",
            zh: "啊——！",
            en: "Aaah!",
            what: "formant voice, vowel 'a', high rising then falling pitch",
            make: |r| {
                voice(
                    r,
                    1.2,
                    |t| 420.0 + 160.0 * (PI_F * t / 1.2).sin(),
                    |t| glide(A, E, t, 0.6, 1.1),
                    |t| ahr(t, 0.03, 0.85, 0.15),
                )
            },
        },
        Sound {
            stem: "yelps",
            zh: "哇哇哇",
            en: "Yelp yelp yelp",
            what: "three quick formant yelps, each higher",
            make: |r| {
                let mut out = vec![0.0; len(1.0)];
                for (k, at) in [0.0f32, 0.28, 0.56].iter().enumerate() {
                    let base = 330.0 + 70.0 * k as f32;
                    let y = voice(
                        r,
                        0.3,
                        move |t| base + 200.0 * t,
                        |t| glide(A, O, t, 0.05, 0.25),
                        |t| ahr(t, 0.01, 0.16, 0.04),
                    );
                    for (i, v) in y.iter().enumerate() {
                        if let Some(o) = out.get_mut(len(*at) + i) {
                            *o += v;
                        }
                    }
                }
                out
            },
        },
    ]
}

const PI_F: f32 = std::f32::consts::PI;

struct ProfileDef {
    id: &'static str,
    zh: &'static str,
    en: &'static str,
    volume_note: &'static str,
    events: &'static [(EventKind, &'static [&'static str])],
}

const PROFILES: &[ProfileDef] = &[
    ProfileDef {
        id: "destruction",
        zh: "毀滅",
        en: "Destruction",
        volume_note: "glass and metal",
        events: &[
            (EventKind::LidOpened, &["glass-01", "glass-02"]),
            (EventKind::LidClosing, &["thud-low"]),
            (EventKind::LidOverbent, &["metal-snap", "metal-bend"]),
            (EventKind::SlapLight, &["glass-01"]),
            (EventKind::SlapMedium, &["glass-02"]),
            (EventKind::SlapHard, &["metal-snap"]),
            (EventKind::SlapCombo, &["glass-crash"]),
        ],
    },
    ProfileDef {
        id: "whine",
        zh: "哀哀叫",
        en: "Whiner",
        volume_note: "ow, ouch, screams",
        events: &[
            (EventKind::LidOpened, &["oh"]),
            (EventKind::LidClosing, &["whimper"]),
            (EventKind::LidOverbent, &["scream"]),
            (EventKind::SlapLight, &["hmm"]),
            (EventKind::SlapMedium, &["aiyo"]),
            (EventKind::SlapHard, &["scream", "aiyo"]),
            (EventKind::SlapCombo, &["yelps"]),
        ],
    },
    ProfileDef {
        id: "classic",
        zh: "經典",
        en: "Classic",
        volume_note: "creaks and thuds",
        events: &[
            (EventKind::LidOpened, &["creak"]),
            (EventKind::LidClosing, &["thud-low"]),
            (EventKind::LidOverbent, &["creak"]),
            (EventKind::SlapLight, &["thud-low"]),
            (EventKind::SlapMedium, &["thud-low"]),
            (EventKind::SlapHard, &["thud-low"]),
        ],
    },
];

fn sha256_hex(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out: PathBuf = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("assets/sounds/builtin"));
    std::fs::create_dir_all(&out).expect("create output dir");

    let mut wavs: BTreeMap<&str, (Vec<u8>, u32)> = BTreeMap::new();
    let defs = sounds();
    for (n, s) in defs.iter().enumerate() {
        // One seed per sound, so editing one sound doesn't change the others.
        let mut rng = fastrand::Rng::with_seed(SEED + n as u64 * 7919);
        let raw = soft_clip((s.make)(&mut rng), 2.5);
        let processed = spank_audio::import::process(raw, 1, SAMPLE_RATE).expect("normalize");
        let bytes = spank_audio::encode_wav(&processed);
        println!(
            "{:<12} {:>5} ms  {:>6.1} LUFS  {:>5.1} dBTP  {:>4} KB",
            s.stem,
            processed.duration_ms(),
            processed.loudness_lufs,
            processed.true_peak_dbtp,
            bytes.len() / 1024
        );
        wavs.insert(s.stem, (bytes, processed.duration_ms()));
    }

    let mut total = 0;
    for p in PROFILES {
        let used: Vec<&str> = {
            let mut v: Vec<&str> = p.events.iter().flat_map(|(_, s)| s.iter().copied()).collect();
            v.sort();
            v.dedup();
            v
        };
        let file = |stem: &str| format!("sounds/{stem}.wav");
        let manifest = Manifest {
            schema_version: spank_pack::SCHEMA_VERSION,
            id: format!("dev.spank.builtin.{}", p.id),
            name: BTreeMap::from([("zh-Hant".into(), p.zh.into()), ("en".into(), p.en.into())]),
            version: "1.0.0".into(),
            author: Some("spank (procedurally generated)".into()),
            license: "CC0-1.0".into(),
            sounds: used
                .iter()
                .map(|stem| {
                    let (bytes, ms) = &wavs[stem];
                    ManifestSound {
                        file: file(stem),
                        sha256: sha256_hex(bytes),
                        duration_ms: *ms,
                        suggested_events: p
                            .events
                            .iter()
                            .filter(|(_, s)| s.contains(stem))
                            .map(|(k, _)| PackEvent(*k))
                            .collect(),
                        license: None,
                        source_url: None,
                    }
                })
                .collect(),
            profile: Some(
                p.events
                    .iter()
                    .map(|(k, s)| (PackEvent(*k), s.iter().map(|x| file(x)).collect()))
                    .collect(),
            ),
        };
        let pack = Pack {
            manifest,
            sounds: used.iter().map(|s| (file(s), wavs[s].0.clone())).collect(),
            icon: None,
        };
        let path = out.join(format!("{}.spankpack", p.id));
        write_pack(std::fs::File::create(&path).expect("create pack"), &pack).expect("write pack");
        // Read it back through the validating reader, as the app will.
        spank_pack::read_pack_file(&path).expect("pack validates");
        let size = std::fs::metadata(&path).unwrap().len();
        total += size;
        println!("→ {} ({} KB, {})", path.display(), size / 1024, p.volume_note);
    }
    println!("total {} KB", total / 1024);
    assert!(total < 5 * 1024 * 1024, "built-in sounds must stay under 5 MB");

    let mut md = String::from(
        "# Built-in sounds\n\n\
         Every built-in sound is **procedurally generated** by `tools/gen-builtin-sounds` (fixed seed, no \
         recordings or samples from anyone else) and dedicated to the public domain under \
         [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/).\n\n\
         Regenerate with `cargo run --release -p gen-builtin-sounds`; the output is identical each time.\n\n\
         | File | 名稱 | Name | How it is made |\n|------|------|------|----------------|\n",
    );
    for s in &defs {
        md.push_str(&format!(
            "| `{}.wav` | {} | {} | {} |\n",
            s.stem, s.zh, s.en, s.what
        ));
    }
    md.push_str("\n| Profile | Pack |\n|---------|------|\n");
    for p in PROFILES {
        md.push_str(&format!(
            "| {}（{}） | `builtin/{}.spankpack` |\n",
            p.zh, p.en, p.id
        ));
    }
    let license = out.parent().unwrap_or(&out).join("SOUNDS_LICENSE.md");
    std::fs::write(&license, md).expect("write license");
    println!("→ {}", license.display());
}

const SAMPLE_RATE: u32 = 48_000;

/// Peak-normalize, then tanh saturation: tames sharp transients so the true-peak limit doesn't
/// leave percussive sounds much quieter than the -16 LUFS target.
fn soft_clip(mut v: Vec<f32>, drive: f32) -> Vec<f32> {
    let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-9);
    let norm = drive.tanh();
    for x in &mut v {
        *x = (*x / peak * drive).tanh() / norm;
    }
    v
}
