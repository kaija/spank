//! Slap calibration wizard (T36, ADR-0006, design §11).
//!
//! Measured light and medium slaps overlap between people (T10: 0.16–0.38 g vs 0.2–0.34 g), so
//! the strength levels come from the user's own hand:
//!
//! 1. **Light, medium, hard**: one slap each. A slap must be clearly stronger than the step below
//!    it and clearly weaker than the step above it ([`LEVEL_RATIO`]); otherwise the step reports
//!    [`CalStatus::TooWeak`] / [`CalStatus::TooStrong`] and waits for another try.
//! 2. **Typing**: [`TYPING_MS`] of typing. The 95th percentile of the vibration peaks becomes the
//!    noise floor, and the light level is kept above it so typing doesn't count as a slap.
//!
//! The levels sit between the measured slaps (geometric means), so a slap halfway between "my
//! light" and "my medium" is decided fairly. Detection uses its own [`ImpactExtractor`] with a low
//! trigger ([`CAL_TRIGGER_G`]) so a light slap below the default trigger still registers.

use crate::config::{Calibration, SlapConfig, SlapLevels};
use crate::impact::{ImpactExtractor, ImpactParams};
use crate::sample::AccelSample;
use serde::{Deserialize, Serialize};

/// Trigger used while calibrating (default detection starts at 0.15 g).
pub const CAL_TRIGGER_G: f32 = 0.06;
/// A step's slap must be this many times stronger than the step below (and weaker than above).
pub const LEVEL_RATIO: f32 = 1.2;
/// A "light" slap above this is certainly not light.
pub const LIGHT_MAX_G: f32 = 0.6;
/// A slap step without any impact for this long reports [`CalStatus::Timeout`].
pub const STEP_TIMEOUT_MS: u64 = 10_000;
/// How long the typing step records.
pub const TYPING_MS: u64 = 5_000;
/// The light level sits at this fraction of the measured light slap…
pub const LIGHT_MARGIN: f32 = 0.6;
/// …but at least this multiple of the typing noise floor…
pub const TYPING_MARGIN: f32 = 1.3;
/// …and never above this fraction of the light slap, nor below [`MIN_LIGHT_G`].
pub const LIGHT_CAP: f32 = 0.85;
pub const MIN_LIGHT_G: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CalStep {
    Light,
    Medium,
    Hard,
    Typing,
}

impl CalStep {
    pub const ALL: [CalStep; 4] = [CalStep::Light, CalStep::Medium, CalStep::Hard, CalStep::Typing];

    fn slap_index(self) -> Option<usize> {
        match self {
            CalStep::Light => Some(0),
            CalStep::Medium => Some(1),
            CalStep::Hard => Some(2),
            CalStep::Typing => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CalStatus {
    /// Slap step: waiting for a slap.
    Waiting,
    /// Typing step: recording; `progress` goes 0 → 1.
    Recording { progress: f32 },
    /// The step is done. For typing, `peak_g` is the learned noise floor.
    Success { peak_g: f32 },
    /// Too close to (or below) the step below.
    TooWeak { peak_g: f32 },
    /// Too close to (or above) the step above.
    TooStrong { peak_g: f32 },
    /// No slap within [`STEP_TIMEOUT_MS`].
    Timeout,
}

/// Wizard state sent to the UI (event `calibration`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CalibrationProgress {
    pub step: CalStep,
    pub status: CalStatus,
    pub light_g: Option<f32>,
    pub medium_g: Option<f32>,
    pub hard_g: Option<f32>,
    pub typing_g: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CalError {
    #[error("calibration is missing the {0:?} slap")]
    Missing(CalStep),
    #[error("calibration produced invalid levels: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct Calibrator {
    extractor: ImpactExtractor,
    step: CalStep,
    status: CalStatus,
    step_started_ms: u64,
    /// Accepted peaks: light, medium, hard.
    peaks: [Option<f32>; 3],
    typing_started_ms: Option<u64>,
    typing_peaks: Vec<f32>,
    typing_g: Option<f32>,
    /// Newest sample time seen.
    last_at_ms: u64,
    /// Largest high-passed |a| since the last [`Calibrator::take_peak`].
    wave_peak: f32,
}

impl Calibrator {
    /// Start at the light step. `base` supplies the filter settings; `now_ms` shares the sensor epoch.
    pub fn new(base: &SlapConfig, sample_rate_hz: f32, now_ms: u64) -> Self {
        let mut params = ImpactParams::new(base, sample_rate_hz);
        params.trigger_g = CAL_TRIGGER_G;
        Self {
            extractor: ImpactExtractor::new(params),
            step: CalStep::Light,
            status: CalStatus::Waiting,
            step_started_ms: now_ms,
            peaks: [None; 3],
            typing_started_ms: None,
            typing_peaks: Vec::new(),
            typing_g: None,
            last_at_ms: now_ms,
            wave_peak: 0.0,
        }
    }

    pub fn step(&self) -> CalStep {
        self.step
    }

    pub fn status(&self) -> CalStatus {
        self.status
    }

    pub fn progress(&self) -> CalibrationProgress {
        CalibrationProgress {
            step: self.step,
            status: self.status,
            light_g: self.peaks[0],
            medium_g: self.peaks[1],
            hard_g: self.peaks[2],
            typing_g: self.typing_g,
        }
    }

    /// Go to `step`. A step that already succeeded shows its result again unless `reset` (retry).
    pub fn set_step(&mut self, step: CalStep, reset: bool, now_ms: u64) {
        self.step = step;
        self.step_started_ms = now_ms.max(self.last_at_ms);
        match step.slap_index() {
            Some(i) => {
                if reset {
                    self.peaks[i] = None;
                }
                self.status = match self.peaks[i] {
                    Some(peak_g) => CalStatus::Success { peak_g },
                    None => CalStatus::Waiting,
                };
            }
            None => {
                if reset || self.typing_g.is_none() {
                    self.typing_g = None;
                    self.typing_peaks.clear();
                    self.typing_started_ms = Some(self.step_started_ms);
                    self.status = CalStatus::Recording { progress: 0.0 };
                } else if let Some(peak_g) = self.typing_g {
                    self.status = CalStatus::Success { peak_g };
                }
            }
        }
    }

    /// Feed a batch. Returns true when [`Calibrator::progress`] changed.
    pub fn push(&mut self, batch: &[AccelSample]) -> bool {
        let before = self.progress();
        let mut impacts = Vec::new();
        self.extractor.extend(batch, &mut impacts);
        let batch_peak = self.extractor.take_peak();
        self.wave_peak = self.wave_peak.max(batch_peak);
        if let Some(last) = batch.last() {
            self.last_at_ms = self.last_at_ms.max(last.at_ms);
        }
        let now = self.last_at_ms;

        match self.step.slap_index() {
            Some(i) => {
                if !matches!(self.status, CalStatus::Success { .. }) {
                    // The strongest impact in the batch is the slap (anything else is its echo).
                    if let Some(peak) = impacts.iter().map(|c| c.peak_g).reduce(f32::max) {
                        self.status = self.judge(i, peak);
                        if let CalStatus::Success { peak_g } = self.status {
                            self.peaks[i] = Some(peak_g);
                        }
                    } else if self.status == CalStatus::Waiting
                        && now.saturating_sub(self.step_started_ms) >= STEP_TIMEOUT_MS
                    {
                        self.status = CalStatus::Timeout;
                    }
                }
            }
            None => {
                if let (CalStatus::Recording { .. }, Some(start)) = (self.status, self.typing_started_ms) {
                    if !batch.is_empty() {
                        self.typing_peaks.push(batch_peak);
                    }
                    let elapsed = now.saturating_sub(start);
                    if elapsed >= TYPING_MS && !self.typing_peaks.is_empty() {
                        let g = percentile(&mut self.typing_peaks, 0.95);
                        self.typing_g = Some(g);
                        self.status = CalStatus::Success { peak_g: g };
                    } else {
                        // Report progress in 10 % steps so the UI isn't flooded.
                        let p = ((elapsed as f32 / TYPING_MS as f32) * 10.0).floor() / 10.0;
                        self.status = CalStatus::Recording { progress: p.min(0.9) };
                    }
                }
            }
        }
        self.progress() != before
    }

    /// Largest high-passed |a| since the last call (live waveform).
    pub fn take_peak(&mut self) -> f32 {
        std::mem::take(&mut self.wave_peak)
    }

    fn judge(&self, i: usize, peak_g: f32) -> CalStatus {
        let below = self.peaks[..i].iter().flatten().copied().reduce(f32::max);
        let above = self.peaks[i + 1..].iter().flatten().copied().reduce(f32::min);
        if below.is_some_and(|b| peak_g < b * LEVEL_RATIO) {
            CalStatus::TooWeak { peak_g }
        } else if above.is_some_and(|a| peak_g * LEVEL_RATIO > a) || (i == 0 && peak_g > LIGHT_MAX_G) {
            CalStatus::TooStrong { peak_g }
        } else {
            CalStatus::Success { peak_g }
        }
    }

    /// The calibration and the slap settings it implies (`base` with new levels and trigger).
    /// Typing is optional; without it the light level only depends on the light slap.
    pub fn result(&self, base: &SlapConfig, now_unix_ms: u64) -> Result<(Calibration, SlapConfig), CalError> {
        let [l, m, h] = self.peaks;
        let l = l.ok_or(CalError::Missing(CalStep::Light))?;
        let m = m.ok_or(CalError::Missing(CalStep::Medium))?;
        let h = h.ok_or(CalError::Missing(CalStep::Hard))?;
        let levels = levels_from(l, m, h, self.typing_g);
        let mut slap = base.clone();
        slap.trigger_g = SlapConfig::default().trigger_g.min(levels.light_g);
        slap.levels = levels.clone();
        slap.validate().map_err(|e| CalError::Invalid(e.to_string()))?;
        Ok((
            Calibration {
                levels,
                noise_floor_g: self.typing_g.unwrap_or(0.0),
                calibrated_unix_ms: now_unix_ms,
            },
            slap,
        ))
    }
}

/// Level thresholds from measured light / medium / hard peaks (`l < m < h`).
pub fn levels_from(l: f32, m: f32, h: f32, typing_g: Option<f32>) -> SlapLevels {
    let mut light_g = l * LIGHT_MARGIN;
    if let Some(t) = typing_g {
        light_g = light_g.max(t * TYPING_MARGIN);
    }
    SlapLevels {
        light_g: light_g.min(l * LIGHT_CAP).max(MIN_LIGHT_G),
        medium_g: (l * m).sqrt(),
        hard_g: (m * h).sqrt(),
    }
}

fn percentile(v: &mut [f32], q: f32) -> f32 {
    v.sort_by(f32::total_cmp);
    let i = ((v.len() - 1) as f32 * q).round() as usize;
    v[i.min(v.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const FS: f32 = 800.0;

    /// Damped 30 Hz ring like a slap, `amp` g, starting at `t0` seconds.
    fn ring(t: f32, t0: f32, amp: f32) -> f32 {
        if t < t0 {
            return 0.0;
        }
        let d = t - t0;
        amp * (-d / 0.04).exp() * (2.0 * PI * 30.0 * d).sin()
    }

    /// Samples from `from` to `to` seconds (absolute time), in 50 ms batches.
    fn batches(from: f32, to: f32, f: impl Fn(f32) -> f32) -> Vec<Vec<AccelSample>> {
        let n0 = (from * FS) as usize;
        let n1 = (to * FS) as usize;
        let samples: Vec<_> = (n0..n1)
            .map(|i| {
                let t = i as f32 / FS;
                AccelSample {
                    at_ms: (t * 1000.0) as u64,
                    x: 0.0,
                    y: 0.0,
                    z: -1.0 + f(t),
                }
            })
            .collect();
        samples.chunks(40).map(|c| c.to_vec()).collect()
    }

    fn feed(c: &mut Calibrator, from: f32, to: f32, f: impl Fn(f32) -> f32) {
        for b in batches(from, to, f) {
            c.push(&b);
        }
    }

    fn cal() -> Calibrator {
        let mut c = Calibrator::new(&SlapConfig::default(), FS, 0);
        feed(&mut c, 0.0, 1.2, |_| 0.0); // warm-up
        c
    }

    fn slap(c: &mut Calibrator, at: f32, amp: f32) -> CalStatus {
        feed(c, at - 0.2, at + 0.4, |t| ring(t, at, amp));
        c.status()
    }

    #[test]
    fn full_run_produces_ordered_levels() {
        let mut c = cal();
        assert_eq!(
            slap(&mut c, 1.5, 0.12),
            CalStatus::Success {
                peak_g: c.progress().light_g.unwrap()
            }
        );
        c.set_step(CalStep::Medium, false, 2000);
        assert!(matches!(slap(&mut c, 2.5, 0.35), CalStatus::Success { .. }));
        c.set_step(CalStep::Hard, false, 3000);
        assert!(matches!(slap(&mut c, 3.5, 0.9), CalStatus::Success { .. }));
        c.set_step(CalStep::Typing, false, 4000);
        assert_eq!(c.status(), CalStatus::Recording { progress: 0.0 });
        // Typing: small 25 Hz buzz.
        feed(&mut c, 4.0, 9.2, |t| 0.02 * (2.0 * PI * 25.0 * t).sin());
        let CalStatus::Success { peak_g: typing } = c.status() else {
            panic!("{:?}", c.status());
        };
        assert!(typing > 0.005 && typing < 0.05, "{typing}");

        let p = c.progress();
        let (l, m, h) = (p.light_g.unwrap(), p.medium_g.unwrap(), p.hard_g.unwrap());
        assert!(l < m && m < h);
        let (calib, slap_cfg) = c.result(&SlapConfig::default(), 42).unwrap();
        let lv = &calib.levels;
        assert!(
            lv.light_g < l && l < lv.medium_g && lv.medium_g < m && m < lv.hard_g && lv.hard_g < h,
            "{lv:?}"
        );
        assert_eq!(calib.calibrated_unix_ms, 42);
        assert_eq!(calib.noise_floor_g, typing);
        assert!(slap_cfg.trigger_g <= lv.light_g);
        slap_cfg.validate().unwrap();
    }

    #[test]
    fn rejects_slaps_that_dont_separate_levels() {
        let mut c = cal();
        slap(&mut c, 1.5, 0.3);
        c.set_step(CalStep::Medium, false, 2000);
        assert!(matches!(slap(&mut c, 2.5, 0.31), CalStatus::TooWeak { .. }));
        // Another try in the same step is judged again.
        assert!(matches!(slap(&mut c, 3.5, 0.5), CalStatus::Success { .. }));
        c.set_step(CalStep::Hard, false, 4000);
        assert!(matches!(slap(&mut c, 4.5, 0.45), CalStatus::TooWeak { .. }));
        // Going back to medium keeps its result; retrying it is judged against light and hard.
        c.set_step(CalStep::Medium, false, 5000);
        assert!(matches!(c.status(), CalStatus::Success { .. }));
        c.set_step(CalStep::Hard, false, 5000);
        assert!(matches!(slap(&mut c, 5.5, 1.2), CalStatus::Success { .. }));
        c.set_step(CalStep::Medium, true, 6000);
        assert_eq!(c.status(), CalStatus::Waiting);
        assert!(matches!(slap(&mut c, 6.5, 1.1), CalStatus::TooStrong { .. }));
        // A very hard "light" slap.
        c.set_step(CalStep::Light, true, 7000);
        assert!(matches!(slap(&mut c, 7.5, 1.0), CalStatus::TooStrong { .. }));
    }

    #[test]
    fn times_out_and_retries() {
        let mut c = cal();
        feed(&mut c, 1.2, 11.5, |_| 0.0);
        assert_eq!(c.status(), CalStatus::Timeout);
        // A slap after the timeout still counts.
        assert!(matches!(slap(&mut c, 12.0, 0.2), CalStatus::Success { .. }));
        // Success is sticky: later slaps in the same step don't change it.
        let before = c.progress();
        slap(&mut c, 13.0, 0.5);
        assert_eq!(c.progress(), before);
    }

    #[test]
    fn result_needs_all_three_slaps() {
        let mut c = cal();
        assert_eq!(
            c.result(&SlapConfig::default(), 0),
            Err(CalError::Missing(CalStep::Light))
        );
        slap(&mut c, 1.5, 0.2);
        assert_eq!(
            c.result(&SlapConfig::default(), 0),
            Err(CalError::Missing(CalStep::Medium))
        );
    }

    #[test]
    fn levels_respect_typing_floor_and_caps() {
        let a = levels_from(0.2, 0.4, 0.8, None);
        assert!((a.light_g - 0.12).abs() < 1e-6);
        assert!((a.medium_g - (0.08f32).sqrt()).abs() < 1e-6);
        // Loud typing pushes the light level up, but never above 85 % of the light slap.
        let b = levels_from(0.2, 0.4, 0.8, Some(0.11));
        assert!((b.light_g - 0.143).abs() < 1e-3, "{}", b.light_g);
        let c = levels_from(0.2, 0.4, 0.8, Some(0.5));
        assert!((c.light_g - 0.17).abs() < 1e-6);
        // Tiny slaps still get a sane minimum.
        assert_eq!(levels_from(0.06, 0.1, 0.2, None).light_g, MIN_LIGHT_G);
    }

    #[test]
    fn typing_step_reports_progress() {
        let mut c = cal();
        c.set_step(CalStep::Typing, false, 1200);
        feed(&mut c, 1.2, 3.8, |_| 0.0);
        let CalStatus::Recording { progress } = c.status() else {
            panic!()
        };
        assert!((0.4..=0.6).contains(&progress), "{progress}");
        assert_eq!(c.take_peak(), 0.0, "silence");
        feed(&mut c, 3.8, 4.0, |t| ring(t, 3.85, 0.3));
        assert!(c.take_peak() > 0.2);
        assert_eq!(c.take_peak(), 0.0);
    }

    #[test]
    fn serde_shapes() {
        let p = CalibrationProgress {
            step: CalStep::Medium,
            status: CalStatus::TooWeak { peak_g: 0.25 },
            light_g: Some(0.2),
            medium_g: None,
            hard_g: None,
            typing_g: None,
        };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["step"], "medium");
        assert_eq!(j["status"]["status"], "too_weak");
        assert_eq!(serde_json::from_value::<CalibrationProgress>(j).unwrap(), p);
    }
}
