//! Impact feature extractor (ADR-0006, design §4.2 step 1).
//!
//! Turns the raw accelerometer stream into [`ImpactCandidate`]s:
//!
//! 1. Per-axis first-order high-pass (`highpass_hz`) removes gravity and slow tilting; `m = |a_hp|`.
//! 2. A window of `window_ms` opens when `m > max(trigger_g, NOISE_K × noise_floor)` and the last
//!    window opened at least `refractory_ms` ago. The noise floor is a slow EMA of `m`, so continuous
//!    activity such as typing raises the bar.
//! 3. When the window is full its features decide whether it is an impact:
//!    - mean frequency ≥ `MIN_FREQ_HZ`: slaps and table bumps ring at 20–35 Hz, while lifting,
//!      carrying and lid motion stay below ~12 Hz;
//!    - not a rebound: inside `REBOUND_MS` of an accepted impact the peak must reach
//!      `REBOUND_RATIO` × that impact's peak (a hard slap echoes ~100–150 ms later at ~30 %).
//! 4. After an accepted impact the noise floor is held for `NOISE_HOLD_MS`, so a fast combo is not
//!    masked by its own previous slap.
//!
//! Parameters come from the T10 traces (`fixtures/traces/m4air-v1`, see docs/spike-report.md T23):
//! slaps ring at ≥ 20 Hz and stand ≥ 21 × above the noise floor, typing reaches at most 8 ×, and
//! motion stays ≤ 12 Hz. Timing uses the sample count at the nominal rate, because reports arrive
//! in ~20 ms batches that share a timestamp.

use crate::config::SlapConfig;
use crate::sample::AccelSample;
use crate::slap::ImpactCandidate;

/// Window length that collects the impact features.
pub const WINDOW_MS: u32 = 80;
/// Trigger must exceed this multiple of the noise floor.
pub const NOISE_K: f32 = 12.0;
/// Time constant of the noise floor EMA.
pub const NOISE_TAU_MS: u32 = 1000;
/// Noise floor is frozen this long after an accepted impact starts.
pub const NOISE_HOLD_MS: u32 = 250;
/// Energy-weighted mean frequency below this is motion, not an impact.
pub const MIN_FREQ_HZ: f32 = 16.0;
/// A window this soon after an accepted impact must reach `REBOUND_RATIO` × its peak.
pub const REBOUND_MS: u32 = 250;
pub const REBOUND_RATIO: f32 = 0.45;
/// No impacts while the high-pass filter settles; the noise floor starts as the mean over this time.
pub const WARMUP_MS: u32 = 1000;

/// Tunables of [`ImpactExtractor`]. User-facing ones come from [`SlapConfig`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactParams {
    /// Nominal accelerometer rate (reports arrive in batches, so timestamps can't be used).
    pub sample_rate_hz: f32,
    pub highpass_hz: f32,
    pub trigger_g: f32,
    pub refractory_ms: u32,
    pub window_ms: u32,
    pub noise_k: f32,
    pub noise_tau_ms: u32,
    pub noise_hold_ms: u32,
    pub min_freq_hz: f32,
    pub rebound_ms: u32,
    pub rebound_ratio: f32,
    pub warmup_ms: u32,
}

impl ImpactParams {
    pub fn new(cfg: &SlapConfig, sample_rate_hz: f32) -> Self {
        Self {
            sample_rate_hz,
            highpass_hz: cfg.highpass_hz,
            trigger_g: cfg.trigger_g,
            refractory_ms: cfg.refractory_ms,
            window_ms: WINDOW_MS,
            noise_k: NOISE_K,
            noise_tau_ms: NOISE_TAU_MS,
            noise_hold_ms: NOISE_HOLD_MS,
            min_freq_hz: MIN_FREQ_HZ,
            rebound_ms: REBOUND_MS,
            rebound_ratio: REBOUND_RATIO,
            warmup_ms: WARMUP_MS,
        }
    }

    fn samples(&self, ms: u32) -> u64 {
        (ms as f64 * self.sample_rate_hz as f64 / 1000.0).round().max(1.0) as u64
    }
}

/// Why windows were rejected, for diagnostics and tuning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractorStats {
    pub windows: u64,
    pub accepted: u64,
    pub low_frequency: u64,
    pub rebound: u64,
}

#[derive(Debug, Clone)]
struct Window {
    onset: u64,
    onset_ms: u64,
    /// (hx, hy, hz, m) per sample.
    buf: Vec<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub struct ImpactExtractor {
    p: ImpactParams,
    alpha: f32,
    ema: f32,
    window_len: u64,
    refractory: u64,
    hold: u64,
    rebound: u64,
    warmup: u64,
    // state
    n: u64,
    prev: Option<[f32; 3]>,
    hp: [f32; 3],
    warm_sum: f64,
    noise_floor: f32,
    hold_until: u64,
    last_onset: Option<u64>,
    last_accepted: Option<(u64, f32)>,
    window: Option<Window>,
    stats: ExtractorStats,
    /// Largest `m` since the last [`ImpactExtractor::take_peak`] (live waveform).
    peak_since_take: f32,
}

impl ImpactExtractor {
    pub fn new(p: ImpactParams) -> Self {
        let fs = p.sample_rate_hz.max(1.0);
        let rc = 1.0 / (2.0 * std::f32::consts::PI * p.highpass_hz);
        let dt = 1.0 / fs;
        Self {
            alpha: rc / (rc + dt),
            ema: 1.0 - (-1.0 / (fs * p.noise_tau_ms.max(1) as f32 / 1000.0)).exp(),
            window_len: p.samples(p.window_ms),
            refractory: p.samples(p.refractory_ms),
            hold: p.samples(p.noise_hold_ms),
            rebound: p.samples(p.rebound_ms),
            warmup: p.samples(p.warmup_ms),
            p,
            n: 0,
            prev: None,
            hp: [0.0; 3],
            warm_sum: 0.0,
            noise_floor: 0.0,
            hold_until: 0,
            last_onset: None,
            last_accepted: None,
            window: None,
            stats: ExtractorStats::default(),
            peak_since_take: 0.0,
        }
    }

    /// Largest high-passed |a| (g) seen since the previous call, for the live waveform.
    pub fn take_peak(&mut self) -> f32 {
        std::mem::take(&mut self.peak_since_take)
    }

    pub fn params(&self) -> &ImpactParams {
        &self.p
    }

    pub fn stats(&self) -> ExtractorStats {
        self.stats
    }

    /// Current noise floor estimate (g).
    pub fn noise_floor(&self) -> f32 {
        self.noise_floor
    }

    /// True once the warm-up is over and impacts can be reported.
    pub fn is_warm(&self) -> bool {
        self.n >= self.warmup
    }

    /// Forget all state (e.g. after the sensor was stopped and restarted).
    pub fn reset(&mut self) {
        *self = Self::new(self.p.clone());
    }

    /// Process a batch, appending any impacts found to `out`.
    pub fn extend(&mut self, samples: &[AccelSample], out: &mut Vec<ImpactCandidate>) {
        out.extend(samples.iter().filter_map(|s| self.push(s)));
    }

    /// Process one sample. Returns an impact when a window completes and passes the checks.
    pub fn push(&mut self, s: &AccelSample) -> Option<ImpactCandidate> {
        let raw = [s.x, s.y, s.z];
        if let Some(prev) = self.prev {
            for (i, h) in self.hp.iter_mut().enumerate() {
                *h = self.alpha * (*h + raw[i] - prev[i]);
            }
        }
        self.prev = Some(raw);
        let [hx, hy, hz] = self.hp;
        let m = (hx * hx + hy * hy + hz * hz).sqrt();
        let idx = self.n;
        self.n += 1;
        self.peak_since_take = self.peak_since_take.max(m);

        if idx < self.warmup {
            self.warm_sum += m as f64;
            if idx + 1 == self.warmup {
                self.noise_floor = (self.warm_sum / self.warmup as f64) as f32;
            }
            return None;
        }

        if let Some(w) = self.window.as_mut() {
            w.buf.push([hx, hy, hz, m]);
            if w.buf.len() as u64 >= self.window_len {
                let w = self.window.take().expect("window present");
                return self.finish(w);
            }
            return None;
        }

        let threshold = self.p.trigger_g.max(self.p.noise_k * self.noise_floor);
        let refractory_over = self.last_onset.is_none_or(|o| idx - o >= self.refractory);
        if m > threshold && refractory_over {
            self.last_onset = Some(idx);
            let mut buf = Vec::with_capacity(self.window_len as usize);
            buf.push([hx, hy, hz, m]);
            self.window = Some(Window {
                onset: idx,
                onset_ms: s.at_ms,
                buf,
            });
            if self.window_len <= 1 {
                let w = self.window.take().expect("window present");
                return self.finish(w);
            }
            return None;
        }
        self.update_noise(idx, m);
        None
    }

    fn update_noise(&mut self, idx: u64, m: f32) {
        if idx >= self.hold_until {
            self.noise_floor += self.ema * (m - self.noise_floor);
        }
    }

    fn finish(&mut self, w: Window) -> Option<ImpactCandidate> {
        self.stats.windows += 1;
        let fs = self.p.sample_rate_hz;
        let peak = w.buf.iter().map(|r| r[3]).fold(0.0f32, f32::max);
        let energy_sum: f32 = w.buf.iter().map(|r| r[3] * r[3]).sum();
        let diff_sum: f32 = w
            .buf
            .windows(2)
            .map(|p| (0..3).map(|a| (p[1][a] - p[0][a]).powi(2)).sum::<f32>())
            .sum();
        let mean_freq_hz = mean_frequency(diff_sum, energy_sum, fs);
        let above_half = w.buf.iter().filter(|r| r[3] >= 0.5 * peak).count();

        let rebound = self
            .last_accepted
            .is_some_and(|(at, p)| w.onset - at < self.rebound && peak < self.p.rebound_ratio * p);
        let accepted = mean_freq_hz >= self.p.min_freq_hz && !rebound;

        if accepted {
            self.stats.accepted += 1;
            self.hold_until = w.onset + self.hold;
            self.last_accepted = Some((w.onset, peak));
            return Some(ImpactCandidate {
                at_ms: w.onset_ms,
                peak_g: peak,
                energy: energy_sum / fs,
                duration_ms: (above_half as f32 * 1000.0 / fs).round() as u16,
                mean_freq_hz,
            });
        }
        if rebound {
            self.stats.rebound += 1;
        } else {
            self.stats.low_frequency += 1;
        }
        // Rejected windows count as background activity.
        for (i, r) in w.buf.iter().enumerate() {
            self.update_noise(w.onset + i as u64, r[3]);
        }
        None
    }
}

/// Energy-weighted mean frequency from the first-difference energy ratio: a sine of frequency f
/// sampled at fs has `Σ(Δx)² / Σx² = (2 sin(π f / fs))²`.
fn mean_frequency(diff_sum: f32, energy_sum: f32, fs: f32) -> f32 {
    if energy_sum <= 0.0 {
        return 0.0;
    }
    let x = ((diff_sum / energy_sum).sqrt() / 2.0).min(1.0);
    fs / std::f32::consts::PI * x.asin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    const FS: f32 = 800.0;

    fn params() -> ImpactParams {
        ImpactParams::new(&SlapConfig::default(), FS)
    }

    /// `secs` of samples from `f(t_seconds) -> (x, y, z)` on top of 1 g gravity.
    fn signal(secs: f32, f: impl Fn(f32) -> [f32; 3]) -> Vec<AccelSample> {
        (0..(secs * FS) as usize)
            .map(|i| {
                let t = i as f32 / FS;
                let [x, y, z] = f(t);
                AccelSample {
                    at_ms: (t * 1000.0) as u64,
                    x,
                    y,
                    z: z - 1.0,
                }
            })
            .collect()
    }

    /// Damped 30 Hz ring starting at `t0` with amplitude `amp` g, like a slap.
    fn ring(t: f32, t0: f32, amp: f32, hz: f32) -> f32 {
        if t < t0 {
            return 0.0;
        }
        let d = t - t0;
        amp * (-d / 0.04).exp() * (2.0 * PI * hz * d).sin()
    }

    fn run(samples: &[AccelSample]) -> (Vec<ImpactCandidate>, ImpactExtractor) {
        let mut x = ImpactExtractor::new(params());
        let mut out = Vec::new();
        x.extend(samples, &mut out);
        (out, x)
    }

    #[test]
    fn quiet_signal_has_no_impacts() {
        let (out, x) = run(&signal(3.0, |_| [0.0, 0.0, 0.0]));
        assert!(out.is_empty());
        assert!(x.is_warm());
        assert_eq!(x.noise_floor(), 0.0);
    }

    #[test]
    fn slap_is_detected_with_features() {
        let s = signal(3.0, |t| [ring(t, 1.5, 0.15, 30.0), 0.0, ring(t, 1.5, 0.5, 30.0)]);
        let (out, x) = run(&s);
        assert_eq!(out.len(), 1, "{:?}", x.stats());
        let c = out[0];
        assert!((1500..1520).contains(&c.at_ms), "{}", c.at_ms);
        assert!(c.peak_g > 0.3 && c.peak_g < 0.6, "{}", c.peak_g);
        assert!((24.0..36.0).contains(&c.mean_freq_hz), "{}", c.mean_freq_hz);
        assert!(c.energy > 0.0 && c.duration_ms > 0);
        assert_eq!(x.stats().accepted, 1);
    }

    #[test]
    fn slow_motion_is_rejected_as_low_frequency() {
        // A 6 Hz sway at 0.3 g, like lifting the laptop.
        let s = signal(3.0, |t| [0.0, 0.0, ring(t, 1.5, 0.6, 6.0)]);
        let (out, x) = run(&s);
        assert!(out.is_empty());
        assert!(x.stats().low_frequency >= 1);
    }

    #[test]
    fn nothing_fires_during_warmup() {
        let s = signal(2.0, |t| [0.0, 0.0, ring(t, 0.3, 0.5, 30.0)]);
        let (out, _) = run(&s);
        assert!(out.is_empty());
    }

    #[test]
    fn continuous_activity_raises_noise_floor() {
        // 30 Hz buzz at 0.05 g, then a 0.25 g tap: 0.25 < 12 × noise floor, so it's masked.
        let buzz = |t: f32| 0.05 * (2.0 * PI * 31.0 * t).sin();
        let s = signal(4.0, |t| [0.0, 0.0, buzz(t) + ring(t, 3.0, 0.3, 30.0)]);
        let (out, x) = run(&s);
        assert!(out.is_empty(), "{out:?}");
        assert!(x.noise_floor() > 0.02, "{}", x.noise_floor());
        // The same tap on a quiet signal fires.
        let (out, _) = run(&signal(4.0, |t| [0.0, 0.0, ring(t, 3.0, 0.3, 30.0)]));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn refractory_and_rebound() {
        // Hard slap then a 30 % echo 120 ms later: the echo is inside the refractory window.
        let s = signal(3.0, |t| {
            [0.0, 0.0, ring(t, 1.5, 0.7, 30.0) + ring(t, 1.62, 0.2, 30.0)]
        });
        assert_eq!(run(&s).0.len(), 1);
        // Echo after the refractory (180 ms) but weak: rejected as a rebound.
        let s = signal(3.0, |t| {
            [0.0, 0.0, ring(t, 1.5, 0.7, 30.0) + ring(t, 1.68, 0.25, 30.0)]
        });
        let (out, x) = run(&s);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(x.stats().rebound, 1);
    }

    #[test]
    fn fast_combo_slaps_200ms_apart_all_fire() {
        let s = signal(3.0, |t| {
            let z: f32 = (0..5).map(|i| ring(t, 1.2 + 0.2 * i as f32, 0.35, 30.0)).sum();
            [0.0, 0.0, z]
        });
        let (out, x) = run(&s);
        assert_eq!(out.len(), 5, "{:?}", x.stats());
    }

    #[test]
    fn take_peak_reports_and_clears() {
        let s = signal(3.0, |t| [0.0, 0.0, ring(t, 1.5, 0.5, 30.0)]);
        let (_, mut x) = run(&s);
        let p = x.take_peak();
        assert!(p > 0.3 && p < 0.6, "{p}");
        assert_eq!(x.take_peak(), 0.0);
    }

    #[test]
    fn reset_restarts_warmup() {
        let s = signal(3.0, |t| [0.0, 0.0, ring(t, 1.5, 0.5, 30.0)]);
        let (_, mut x) = run(&s);
        x.reset();
        assert!(!x.is_warm());
        assert_eq!(x.stats(), ExtractorStats::default());
        assert_eq!(x.params().trigger_g, SlapConfig::default().trigger_g);
    }

    #[test]
    fn mean_frequency_of_sine() {
        let fs = 800.0;
        let xs: Vec<f32> = (0..400)
            .map(|i| (2.0 * PI * 25.0 * i as f32 / fs).sin())
            .collect();
        let e: f32 = xs.iter().map(|v| v * v).sum();
        let d: f32 = xs.windows(2).map(|p| (p[1] - p[0]).powi(2)).sum();
        assert!((mean_frequency(d, e, fs) - 25.0).abs() < 0.5);
        assert_eq!(mean_frequency(1.0, 0.0, fs), 0.0);
    }
}
