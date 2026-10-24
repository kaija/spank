//! Shared helpers: replay a fixture trace through the orchestrator.
#![allow(dead_code)]

use serde::Deserialize;
use spank_core::{
    AppConfig, EventKind, FireResult, Orchestrator, Output, PlayOutcome, PlayRequest, PlaybackStatus, Player,
    Profile, TimeOfDay, TriggerEvent,
};
use spank_sensors::replay::Batch;
use spank_sensors::{Trace, TraceReplay};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/traces/m4air-v1")
}

#[derive(Debug, Deserialize)]
pub struct Expected {
    pub tolerance_ms: u64,
    pub scenarios: BTreeMap<String, ExpectedScenario>,
}

#[derive(Debug, Deserialize)]
pub struct ExpectedScenario {
    /// Slap onsets in seconds.
    pub slaps: Vec<f64>,
    /// Lid events in order (snake_case `EventKind`), `null` = not checked.
    pub lid: Option<Vec<EventKind>>,
}

pub fn expected() -> Expected {
    serde_json::from_slice(&std::fs::read(fixtures().join("expected.json")).unwrap()).unwrap()
}

/// Accepts everything, records requests. Never "playing", so the playback gate stays open
/// (the traces were recorded without our own sounds, except `playback` which used afplay).
#[derive(Default)]
pub struct RecordingPlayer {
    pub requests: Vec<PlayRequest>,
}

impl Player for RecordingPlayer {
    fn play(&mut self, req: &PlayRequest) -> PlayOutcome {
        self.requests.push(req.clone());
        PlayOutcome::Started
    }
    fn status(&self) -> PlaybackStatus {
        PlaybackStatus::default()
    }
}

pub fn profile_with_all_sounds() -> Profile {
    let mut p = Profile::new("test", "Test");
    for (k, b) in p.mapping.iter_mut() {
        b.sounds = vec![format!("{k:?}").as_str().into()];
    }
    p
}

pub struct Replayed {
    pub fired: Vec<(TriggerEvent, FireResult)>,
    pub outputs: Vec<Output>,
    pub player: RecordingPlayer,
    pub trace: Trace,
}

/// Replay `<scenario>.ndjson` through an orchestrator with slaps enabled.
pub fn replay(scenario: &str) -> Replayed {
    let trace = Trace::read(&fixtures().join(format!("{scenario}.ndjson"))).unwrap();
    let mut cfg = AppConfig::default();
    cfg.slap.enabled = true;
    let rate = trace.accel_rate_hz().unwrap();
    let noon: TimeOfDay = "12:00".parse().unwrap();
    let mut orch = Orchestrator::new(cfg, profile_with_all_sounds(), rate, noon);
    let mut player = RecordingPlayer::default();
    let mut outputs = orch.start();
    for b in TraceReplay::new(&trace).batches(50) {
        outputs.extend(match b {
            Batch::Lid(s) => orch.on_lid(s, &mut player),
            Batch::Accel(v) => orch.on_accel(&v, &mut player),
        });
    }
    let fired = outputs
        .iter()
        .filter_map(|o| match o {
            Output::Fired { event, result } => Some((*event, result.clone())),
            _ => None,
        })
        .collect();
    Replayed {
        fired,
        outputs,
        player,
        trace,
    }
}
