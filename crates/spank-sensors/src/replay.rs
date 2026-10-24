//! Replay a recorded [`Trace`] as sensor sources (ADR-0010).
//!
//! By default samples come out as fast as they are read. [`TraceReplay::with_speed`] paces them
//! against the wall clock (`2.0` = twice real time), for manual testing of the running app.

use crate::trace::Trace;
use crate::{AccelSource, LidSource};
use spank_core::{AccelSample, LidSample};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SensorEvent {
    Lid(LidSample),
    Accel(AccelSample),
}

impl SensorEvent {
    pub fn at_ms(&self) -> u64 {
        match self {
            Self::Lid(s) => s.at_ms,
            Self::Accel(s) => s.at_ms,
        }
    }
}

#[derive(Debug, Clone)]
struct Pacer {
    speed: f64,
    origin: Option<(Instant, u64)>,
}

impl Pacer {
    fn wait(&mut self, at_ms: u64) {
        let (start, t0) = *self.origin.get_or_insert((Instant::now(), at_ms));
        let due = Duration::from_secs_f64(at_ms.saturating_sub(t0) as f64 / 1000.0 / self.speed);
        if let Some(left) = due.checked_sub(start.elapsed()) {
            std::thread::sleep(left);
        }
    }
}

#[derive(Debug, Clone)]
pub struct TraceReplay {
    lid: Vec<LidSample>,
    accel: Vec<AccelSample>,
    li: usize,
    ai: usize,
    pacer: Option<Pacer>,
}

impl TraceReplay {
    pub fn new(trace: &Trace) -> Self {
        Self::from_samples(trace.lid.clone(), trace.accel.clone())
    }

    pub fn from_samples(lid: Vec<LidSample>, accel: Vec<AccelSample>) -> Self {
        Self {
            lid,
            accel,
            li: 0,
            ai: 0,
            pacer: None,
        }
    }

    /// Pace output at `speed` × real time. Non-positive or non-finite speeds disable pacing.
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.pacer = (speed.is_finite() && speed > 0.0).then_some(Pacer { speed, origin: None });
        self
    }

    pub fn is_done(&self) -> bool {
        self.li >= self.lid.len() && self.ai >= self.accel.len()
    }

    /// Start over from the first sample.
    pub fn rewind(&mut self) {
        self.li = 0;
        self.ai = 0;
        if let Some(p) = &mut self.pacer {
            p.origin = None;
        }
    }

    fn pace(&mut self, at_ms: u64) {
        if let Some(p) = &mut self.pacer {
            p.wait(at_ms);
        }
    }

    /// Next sample of either kind, in time order (lid first on ties).
    pub fn next_event(&mut self) -> Option<SensorEvent> {
        let lid = self.lid.get(self.li).copied();
        let acc = self.accel.get(self.ai).copied();
        let ev = match (lid, acc) {
            (Some(l), Some(a)) if l.at_ms <= a.at_ms => SensorEvent::Lid(l),
            (_, Some(a)) => SensorEvent::Accel(a),
            (Some(l), None) => SensorEvent::Lid(l),
            (None, None) => return None,
        };
        match ev {
            SensorEvent::Lid(_) => self.li += 1,
            SensorEvent::Accel(_) => self.ai += 1,
        }
        self.pace(ev.at_ms());
        Some(ev)
    }

    /// Consume the rest in batches: each lid sample, plus the accelerometer samples before it
    /// grouped into chunks of at most `batch_ms` (like the ~50 ms core-thread batches, ADR-0013).
    pub fn batches(&mut self, batch_ms: u64) -> Vec<Batch> {
        let mut out = Vec::new();
        let mut acc: Vec<AccelSample> = Vec::new();
        while let Some(ev) = self.next_event() {
            match ev {
                SensorEvent::Lid(l) => {
                    if !acc.is_empty() {
                        out.push(Batch::Accel(std::mem::take(&mut acc)));
                    }
                    out.push(Batch::Lid(l));
                }
                SensorEvent::Accel(a) => {
                    if acc.first().is_some_and(|f| a.at_ms - f.at_ms >= batch_ms) {
                        out.push(Batch::Accel(std::mem::take(&mut acc)));
                    }
                    acc.push(a);
                }
            }
        }
        if !acc.is_empty() {
            out.push(Batch::Accel(acc));
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Batch {
    Lid(LidSample),
    Accel(Vec<AccelSample>),
}

impl LidSource for TraceReplay {
    fn next(&mut self) -> Option<LidSample> {
        let s = *self.lid.get(self.li)?;
        self.li += 1;
        self.pace(s.at_ms);
        Some(s)
    }
}

impl AccelSource for TraceReplay {
    fn next(&mut self) -> Option<AccelSample> {
        let s = *self.accel.get(self.ai)?;
        self.ai += 1;
        self.pace(s.at_ms);
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lid(at_ms: u64) -> LidSample {
        LidSample {
            at_ms,
            angle_deg: at_ms as f32,
        }
    }

    fn acc(at_ms: u64) -> AccelSample {
        AccelSample {
            at_ms,
            x: 0.0,
            y: 0.0,
            z: -1.0,
        }
    }

    fn replay() -> TraceReplay {
        TraceReplay::from_samples(vec![lid(10), lid(30)], vec![acc(0), acc(10), acc(20), acc(40)])
    }

    #[test]
    fn merged_order() {
        let mut r = replay();
        let ts: Vec<_> = std::iter::from_fn(|| r.next_event())
            .map(|e| (e.at_ms(), matches!(e, SensorEvent::Lid(_))))
            .collect();
        assert_eq!(
            ts,
            vec![
                (0, false),
                (10, true),
                (10, false),
                (20, false),
                (30, true),
                (40, false)
            ]
        );
        assert!(r.is_done());
        r.rewind();
        assert!(!r.is_done());
    }

    #[test]
    fn sources_are_independent() {
        let mut r = replay();
        assert_eq!(LidSource::next(&mut r), Some(lid(10)));
        let mut v = Vec::new();
        r.drain(&mut v);
        assert_eq!(v.len(), 4);
        assert_eq!(LidSource::next(&mut r), Some(lid(30)));
        assert_eq!(LidSource::next(&mut r), None);
    }

    #[test]
    fn batches_split_on_lid_and_time() {
        let mut r = replay();
        let b = r.batches(15);
        assert_eq!(
            b,
            vec![
                Batch::Accel(vec![acc(0)]),
                Batch::Lid(lid(10)),
                Batch::Accel(vec![acc(10), acc(20)]),
                Batch::Lid(lid(30)),
                Batch::Accel(vec![acc(40)]),
            ]
        );
    }

    #[test]
    fn paced_replay_takes_real_time() {
        let mut r = TraceReplay::from_samples(vec![], vec![acc(0), acc(100)]).with_speed(4.0);
        let t = Instant::now();
        while AccelSource::next(&mut r).is_some() {}
        let e = t.elapsed();
        assert!(
            e >= Duration::from_millis(24) && e < Duration::from_millis(500),
            "{e:?}"
        );
        assert!(
            TraceReplay::from_samples(vec![], vec![])
                .with_speed(0.0)
                .pacer
                .is_none()
        );
    }
}
