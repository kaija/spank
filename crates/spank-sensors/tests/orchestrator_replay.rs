//! T27: replay recorded traces through the orchestrator and check the event sequences.

mod common;

use common::*;
use spank_core::{
    AppConfig, EventKind, FireResult, Orchestrator, Output, SensorCommand, SysSignal, TimeOfDay,
};
use spank_sensors::replay::Batch;
use spank_sensors::{Trace, TraceReplay};

fn lid_kinds(fired: &[(spank_core::TriggerEvent, FireResult)]) -> Vec<EventKind> {
    fired.iter().map(|(e, _)| e.kind).filter(|k| k.is_lid()).collect()
}

#[test]
fn lid_event_sequences_match_expected() {
    for (name, sc) in expected().scenarios {
        let Some(want) = sc.lid else { continue };
        let r = replay(&name);
        assert_eq!(lid_kinds(&r.fired), want, "{name}");
        // Every event was played with the bound sound at full app × profile gain.
        for (e, res) in &r.fired {
            assert_eq!(
                res,
                &FireResult::Played {
                    sound: format!("{:?}", e.kind).as_str().into()
                },
                "{name}"
            );
        }
        assert_eq!(r.player.requests.len(), r.fired.len());
        assert!(r.player.requests.iter().all(|q| (q.gain - 0.8).abs() < 1e-6));
    }
}

#[test]
fn lid_events_land_in_the_right_segments() {
    let r = replay("lid");
    let labels: Vec<_> = r
        .fired
        .iter()
        .map(|(e, _)| r.trace.label_at(e.at_ms).unwrap().label.as_str())
        .collect();
    assert_eq!(
        labels,
        vec!["lid_overbend", "lid_overbend", "lid_close_open", "lid_close_open"]
    );
    // Lid motion gates every accelerometer impact in this trace.
    assert!(r.fired.iter().all(|(e, _)| e.kind.is_lid()));
}

/// Replay `lid.ndjson`, injecting system signals at given times (ms).
fn replay_with_signals(signals: &[(u64, SysSignal)]) -> (Vec<(EventKind, u64)>, Vec<SensorCommand>) {
    let trace = Trace::read(&fixtures().join("lid.ndjson")).unwrap();
    let noon: TimeOfDay = "12:00".parse().unwrap();
    let mut cfg = AppConfig::default();
    cfg.slap.enabled = true;
    let mut o = Orchestrator::new(
        cfg,
        profile_with_all_sounds(),
        trace.accel_rate_hz().unwrap(),
        noon,
    );
    let mut p = RecordingPlayer::default();
    let mut out = o.start();
    let mut pending = signals.iter().peekable();
    for b in TraceReplay::new(&trace).batches(50) {
        let at = match &b {
            Batch::Lid(s) => s.at_ms,
            Batch::Accel(v) => v[0].at_ms,
        };
        while let Some((_, s)) = pending.next_if(|(t, _)| *t <= at) {
            out.extend(o.on_sys(*s, noon));
        }
        out.extend(match b {
            Batch::Lid(s) => o.on_lid(s, &mut p),
            Batch::Accel(v) => o.on_accel(&v, &mut p),
        });
    }
    let fired = out
        .iter()
        .filter_map(|x| match x {
            Output::Fired { event, .. } => Some((event.kind, event.at_ms)),
            _ => None,
        })
        .collect();
    let cmds = out
        .iter()
        .filter_map(|x| match x {
            Output::Sensor(c) => Some(*c),
            _ => None,
        })
        .collect();
    (fired, cmds)
}

#[test]
fn sleep_drops_closing_and_wake_fires_opened() {
    // Asleep from 15 s to 20.6 s (lid closes to 10° at ~18 s, reopens to ~100° by 20.6 s).
    let (fired, cmds) = replay_with_signals(&[(15_000, SysSignal::WillSleep), (20_600, SysSignal::DidWake)]);
    let kinds: Vec<_> = fired.iter().map(|f| f.0).collect();
    assert_eq!(
        kinds,
        vec![
            EventKind::LidOverbent,
            EventKind::LidOverbent,
            EventKind::LidOpened,
            EventKind::LidOverbent
        ]
    );
    let opened = fired[2].1;
    assert!(
        (20_600..20_700).contains(&opened),
        "LidOpened at first sample after wake: {opened}"
    );
    assert_eq!(
        cmds,
        vec![
            SensorCommand::StartLid,
            SensorCommand::StartAccel,
            SensorCommand::StopLid,
            SensorCommand::StopAccel,
            SensorCommand::StartLid,
            SensorCommand::StartAccel
        ]
    );
}

#[test]
fn locked_screen_stops_sensors() {
    let (fired, cmds) =
        replay_with_signals(&[(0, SysSignal::ScreenLocked), (16_000, SysSignal::ScreenUnlocked)]);
    let kinds: Vec<_> = fired.iter().map(|f| f.0).collect();
    assert_eq!(kinds, vec![EventKind::LidClosing, EventKind::LidOverbent]);
    assert_eq!(
        cmds[2..],
        [
            SensorCommand::StopLid,
            SensorCommand::StopAccel,
            SensorCommand::StartLid,
            SensorCommand::StartAccel
        ]
    );
}

#[test]
fn muted_meeting_still_detects() {
    let (fired, _) = replay_with_signals(&[(0, SysSignal::MicInUse { in_use: true })]);
    assert_eq!(fired.len(), 4, "events are still detected while muted");
}
