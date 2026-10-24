//! T23 gate: replay every labeled trace and score slap detection against the hand-annotated
//! onsets in `fixtures/traces/m4air-v1/expected.json` (ADR-0010: precision ≥ 0.95, recall ≥ 0.90).

mod common;

use common::*;
use spank_core::EventKind;

const MIN_PRECISION: f64 = 0.95;
const MIN_RECALL: f64 = 0.90;

#[test]
fn slap_precision_and_recall_on_m4air_traces() {
    let exp = expected();
    let (mut tp, mut fp, mut fn_) = (0usize, 0usize, 0usize);
    let mut report = Vec::new();
    for (name, sc) in &exp.scenarios {
        let r = replay(name);
        let detected: Vec<u64> = r
            .fired
            .iter()
            .filter(|(e, _)| e.kind.is_slap())
            .map(|(e, _)| e.at_ms)
            .collect();
        let mut used = vec![false; sc.slaps.len()];
        let (mut s_tp, mut s_fp) = (0, 0);
        for &d in &detected {
            let hit = sc
                .slaps
                .iter()
                .enumerate()
                .position(|(i, &s)| !used[i] && (d as f64 - s * 1000.0).abs() <= exp.tolerance_ms as f64);
            match hit {
                Some(i) => {
                    used[i] = true;
                    s_tp += 1;
                }
                None => {
                    s_fp += 1;
                    let label = r.trace.label_at(d).map_or("-", |l| l.label.as_str());
                    report.push(format!(
                        "  false positive: {name} {:.3}s ({label})",
                        d as f64 / 1000.0
                    ));
                }
            }
        }
        for (i, s) in sc.slaps.iter().enumerate() {
            if !used[i] {
                report.push(format!("  missed: {name} {s:.2}s"));
            }
        }
        let s_fn = sc.slaps.len() - s_tp;
        report.push(format!(
            "{name:<9} expected {:>2}  detected {:>2}  tp {s_tp:>2} fp {s_fp} fn {s_fn}",
            sc.slaps.len(),
            detected.len()
        ));
        tp += s_tp;
        fp += s_fp;
        fn_ += s_fn;
    }
    let precision = tp as f64 / (tp + fp).max(1) as f64;
    let recall = tp as f64 / (tp + fn_).max(1) as f64;
    report.push(format!(
        "precision {precision:.3}  recall {recall:.3}  (tp {tp}, fp {fp}, fn {fn_})"
    ));
    let report = report.join("\n");
    println!("{report}");
    assert!(
        precision >= MIN_PRECISION && recall >= MIN_RECALL,
        "slap detection below target:\n{report}"
    );
}

#[test]
fn slap_levels_follow_peak() {
    // Hard slaps and table bumps are the strongest; at least one of each level appears.
    let r = replay("baseline");
    let kinds: Vec<EventKind> = r
        .fired
        .iter()
        .map(|(e, _)| e.kind)
        .filter(|k| k.is_slap())
        .collect();
    for k in [
        EventKind::SlapLight,
        EventKind::SlapMedium,
        EventKind::SlapHard,
        EventKind::SlapCombo,
    ] {
        assert!(kinds.contains(&k), "{k:?} missing from {kinds:?}");
    }
    let bumps = replay("typing");
    assert!(
        bumps.fired.iter().any(|(e, _)| e.kind == EventKind::SlapHard),
        "table bumps (1.3–1.9 g) should be hard slaps"
    );
}
