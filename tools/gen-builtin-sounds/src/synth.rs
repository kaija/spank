//! Small procedural synthesizers for the built-in sounds. Everything is generated from code and a
//! fixed seed, so the output is our own work (CC0) and reproducible.

use std::f32::consts::PI;

pub const SR: f32 = 48_000.0;

pub fn len(secs: f32) -> usize {
    (secs * SR) as usize
}

/// RBJ biquad.
#[derive(Clone, Copy)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b: [b0 / a0, b1 / a0, b2 / a0],
            a: [a1 / a0, a2 / a0],
            z: [0.0; 2],
        }
    }

    pub fn bandpass(f: f32, q: f32) -> Self {
        let w = 2.0 * PI * f / SR;
        let alpha = w.sin() / (2.0 * q);
        Self::new(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * w.cos(), 1.0 - alpha)
    }

    pub fn lowpass(f: f32, q: f32) -> Self {
        let w = 2.0 * PI * f / SR;
        let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
        Self::new(
            (1.0 - c) / 2.0,
            1.0 - c,
            (1.0 - c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn highpass(f: f32, q: f32) -> Self {
        let w = 2.0 * PI * f / SR;
        let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
        Self::new(
            (1.0 + c) / 2.0,
            -(1.0 + c),
            (1.0 + c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

fn noise(rng: &mut fastrand::Rng) -> f32 {
    rng.f32() * 2.0 - 1.0
}

/// Exponential decay with time constant `tau` seconds.
fn decay(t: f32, tau: f32) -> f32 {
    (-t / tau).exp()
}

fn add(buf: &mut [f32], at: usize, src: &[f32]) {
    for (i, s) in src.iter().enumerate() {
        if let Some(b) = buf.get_mut(at + i) {
            *b += s;
        }
    }
}

/// A decaying sine partial.
fn partial(n: usize, f: f32, tau: f32, amp: f32, phase: f32) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            amp * decay(t, tau) * (2.0 * PI * f * t + phase).sin()
        })
        .collect()
}

/// Breaking glass: a bright noise burst, then shards (short high partials) scattered over time.
pub fn glass(rng: &mut fastrand::Rng, secs: f32, shards: usize, spread: f32) -> Vec<f32> {
    let n = len(secs);
    let mut out = vec![0.0; n];
    let mut hp = Biquad::highpass(2500.0, 0.7);
    for (i, o) in out.iter_mut().enumerate().take(len(0.08)) {
        let t = i as f32 / SR;
        *o += hp.run(noise(rng)) * decay(t, 0.018) * 0.9;
    }
    // Low crunch.
    let mut lp = Biquad::lowpass(400.0, 0.8);
    for (i, o) in out.iter_mut().enumerate().take(len(0.06)) {
        let t = i as f32 / SR;
        *o += lp.run(noise(rng)) * decay(t, 0.015) * 0.8;
    }
    for _ in 0..shards {
        let onset = (-(rng.f32().max(1e-4)).ln() * spread).min(secs * 0.7);
        let amp = 0.15 + rng.f32() * 0.35;
        let base = 2200.0 + rng.f32() * 6500.0;
        let tau = 0.02 + rng.f32() * 0.12;
        let m = len(tau * 6.0);
        let mut shard = vec![0.0; m];
        for k in 0..3 {
            let ratio = [1.0, 1.47 + rng.f32() * 0.2, 2.31 + rng.f32() * 0.3][k];
            add(
                &mut shard,
                0,
                &partial(
                    m,
                    base * ratio,
                    tau / (1.0 + k as f32),
                    amp / (1.0 + k as f32),
                    rng.f32() * 6.0,
                ),
            );
        }
        add(&mut out, len(onset), &shard);
    }
    out
}

/// Metal bar snapping: click plus inharmonic bar modes with long, beating decays.
pub fn metal_snap(rng: &mut fastrand::Rng, f0: f32, secs: f32) -> Vec<f32> {
    let n = len(secs);
    let mut out = vec![0.0; n];
    let mut hp = Biquad::highpass(1500.0, 0.7);
    for (i, o) in out.iter_mut().enumerate().take(len(0.01)) {
        *o += hp.run(noise(rng)) * decay(i as f32 / SR, 0.002);
    }
    for (k, ratio) in [1.0, 2.756, 5.404, 8.933, 13.34].iter().enumerate() {
        let tau = 0.55 / (1.0 + k as f32 * 0.6);
        let amp = 0.5 / (1.0 + k as f32 * 0.5);
        add(&mut out, 0, &partial(n, f0 * ratio, tau, amp, 0.0));
        // Slight detune → beating, like a real bar.
        add(&mut out, 0, &partial(n, f0 * ratio * 1.004, tau, amp * 0.6, 1.0));
    }
    out
}

/// Stick-slip friction: impulses at a varying rate, each exciting a resonator bank.
/// `rate(t)` gives impulses per second; `res` lists (frequency, Q, gain).
pub fn friction(
    rng: &mut fastrand::Rng,
    secs: f32,
    rate: impl Fn(f32) -> f32,
    env: impl Fn(f32) -> f32,
    res: &[(f32, f32, f32)],
) -> Vec<f32> {
    let n = len(secs);
    let mut bank: Vec<(Biquad, f32)> = res.iter().map(|&(f, q, g)| (Biquad::bandpass(f, q), g)).collect();
    let mut next = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let mut x = 0.0;
            if t >= next {
                x = 1.0 + noise(rng) * 0.3;
                let r = rate(t).max(1.0);
                next = t + (1.0 / r) * (0.85 + rng.f32() * 0.3);
            }
            x += noise(rng) * 0.02;
            let y: f32 = bank.iter_mut().map(|(b, g)| b.run(x) * *g).sum();
            y * env(t)
        })
        .collect()
}

/// Low thud: falling sine plus a short low-passed noise burst.
pub fn thud(rng: &mut fastrand::Rng, f_start: f32, f_end: f32, secs: f32) -> Vec<f32> {
    let n = len(secs);
    let mut lp = Biquad::lowpass(350.0, 0.7);
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            let f = f_end + (f_start - f_end) * decay(t, 0.05);
            phase += 2.0 * PI * f / SR;
            let attack = (t / 0.003).min(1.0);
            phase.sin() * decay(t, 0.14) * attack + lp.run(noise(rng)) * decay(t, 0.025) * 0.9
        })
        .collect()
}

/// Vowel formants (F1, F2, F3) in Hz.
pub const A: [f32; 3] = [800.0, 1200.0, 2600.0];
pub const O: [f32; 3] = [480.0, 820.0, 2600.0];
pub const U: [f32; 3] = [340.0, 700.0, 2400.0];
pub const I: [f32; 3] = [300.0, 2250.0, 3000.0];
pub const E: [f32; 3] = [550.0, 1800.0, 2600.0];
pub const M: [f32; 3] = [250.0, 1100.0, 2500.0];

/// A cartoon voice: additive harmonics shaped by moving formants.
/// `pitch(t)` in Hz, `vowel(t)` returns formants, `env(t)` the loudness.
pub fn voice(
    rng: &mut fastrand::Rng,
    secs: f32,
    pitch: impl Fn(f32) -> f32,
    vowel: impl Fn(f32) -> [f32; 3],
    env: impl Fn(f32) -> f32,
) -> Vec<f32> {
    let n = len(secs);
    let mut phases = [0.0f32; 40];
    let mut breath = Biquad::bandpass(2500.0, 1.0);
    let mut jitter = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SR;
            jitter = jitter * 0.999 + noise(rng) * 0.001;
            let f0 = pitch(t) * (1.0 + 0.012 * (2.0 * PI * 5.5 * t).sin() + jitter);
            let fm = vowel(t);
            let mut y = 0.0;
            for (k, ph) in phases.iter_mut().enumerate() {
                let f = f0 * (k + 1) as f32;
                if f > 5000.0 {
                    break;
                }
                let shape: f32 = fm
                    .iter()
                    .zip([1.0, 0.7, 0.35])
                    .map(|(&fc, g)| {
                        let bw = 60.0 + fc * 0.08;
                        g / (1.0 + ((f - fc) / bw).powi(2))
                    })
                    .sum();
                *ph += 2.0 * PI * f / SR;
                y += ph.sin() * shape / (k + 1) as f32;
            }
            (y + breath.run(noise(rng)) * 0.04) * env(t)
        })
        .collect()
}

/// Linear interpolation between vowels over `[t0, t1]`.
pub fn glide(a: [f32; 3], b: [f32; 3], t: f32, t0: f32, t1: f32) -> [f32; 3] {
    let x = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * x)
}

/// Attack / hold / release envelope.
pub fn ahr(t: f32, attack: f32, hold_until: f32, release: f32) -> f32 {
    if t < attack {
        t / attack
    } else if t < hold_until {
        1.0
    } else {
        decay(t - hold_until, release)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generators_are_finite_and_audible() {
        let mut rng = fastrand::Rng::with_seed(1);
        let all = [
            glass(&mut rng, 0.8, 30, 0.1),
            metal_snap(&mut rng, 300.0, 1.0),
            friction(&mut rng, 1.0, |_| 30.0, |_| 1.0, &[(900.0, 12.0, 1.0)]),
            thud(&mut rng, 90.0, 45.0, 0.5),
            voice(&mut rng, 0.5, |_| 200.0, |_| A, |t| ahr(t, 0.02, 0.3, 0.05)),
        ];
        for s in all {
            assert!(s.iter().all(|v| v.is_finite()));
            assert!(s.iter().any(|v| v.abs() > 0.01));
        }
        assert_eq!(glide(A, O, 0.5, 0.0, 1.0)[0], 640.0);
        assert_eq!(ahr(0.0, 0.1, 0.2, 0.1), 0.0);
    }
}
