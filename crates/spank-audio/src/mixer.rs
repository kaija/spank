//! Single-voice mixer with priority preemption (ADR-0008). Pure and allocation-free while
//! rendering, so it can run inside the audio callback and be tested with a fake output.
//!
//! - One sound plays at a time.
//! - A request with strictly higher priority fades the current sound out over [`FADE_MS`] and then
//!   starts; equal or lower priority is dropped ([`MixOutcome::Busy`]).
//! - Clips keep their own rate and channel count; the mixer converts on the fly (linear
//!   interpolation, mono ↔ stereo mapping), so a device change never needs a reload.

use spank_core::Priority;
use std::sync::Arc;

pub const FADE_MS: u32 = 50;

/// Decoded audio, interleaved f32.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
}

impl Clip {
    pub fn new(samples: Vec<f32>, channels: u16, sample_rate: u32) -> Self {
        Self {
            samples,
            channels: channels.max(1),
            sample_rate: sample_rate.max(1),
        }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn duration_ms(&self) -> u64 {
        self.frames() as u64 * 1000 / self.sample_rate as u64
    }

    pub fn bytes(&self) -> usize {
        self.samples.len() * std::mem::size_of::<f32>()
    }

    /// Sample of `ch` at fractional frame `pos` (linear interpolation).
    fn sample(&self, ch: usize, pos: f64) -> f32 {
        let c = self.channels as usize;
        let i = pos as usize;
        let frac = (pos - i as f64) as f32;
        let at = |f: usize| self.samples.get(f * c + ch.min(c - 1)).copied().unwrap_or(0.0);
        at(i) + (at(i + 1) - at(i)) * frac
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixOutcome {
    Started,
    Preempted,
    Busy,
}

#[derive(Debug, Clone)]
struct Voice {
    clip: Arc<Clip>,
    pos: f64,
    step: f64,
    gain: f32,
    priority: Priority,
    /// Frames left in the fade-out, when fading.
    fade_left: Option<u32>,
}

impl Voice {
    fn done(&self) -> bool {
        self.pos >= self.clip.frames() as f64 || self.fade_left == Some(0)
    }
}

#[derive(Debug, Clone)]
pub struct Mixer {
    channels: usize,
    sample_rate: u32,
    fade_frames: u32,
    voice: Option<Voice>,
    next: Option<Voice>,
}

impl Mixer {
    pub fn new(channels: u16, sample_rate: u32) -> Self {
        Self {
            channels: channels.max(1) as usize,
            sample_rate: sample_rate.max(1),
            fade_frames: (sample_rate as u64 * FADE_MS as u64 / 1000).max(1) as u32,
            voice: None,
            next: None,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn is_active(&self) -> bool {
        self.voice.is_some() || self.next.is_some()
    }

    /// Priority of what is (or is about to be) playing.
    pub fn current_priority(&self) -> Option<Priority> {
        self.next.as_ref().or(self.voice.as_ref()).map(|v| v.priority)
    }

    pub fn request(&mut self, clip: Arc<Clip>, priority: Priority, gain: f32) -> MixOutcome {
        let voice = Voice {
            step: clip.sample_rate as f64 / self.sample_rate as f64,
            clip,
            pos: 0.0,
            gain: gain.clamp(0.0, 1.0),
            priority,
            fade_left: None,
        };
        match self.current_priority() {
            None => {
                self.voice = Some(voice);
                MixOutcome::Started
            }
            Some(cur) if priority.preempts(cur) => {
                if let Some(v) = &mut self.voice
                    && v.fade_left.is_none()
                {
                    v.fade_left = Some(self.fade_frames);
                }
                self.next = Some(voice);
                MixOutcome::Preempted
            }
            Some(_) => MixOutcome::Busy,
        }
    }

    /// Stop everything immediately.
    pub fn stop(&mut self) {
        self.voice = None;
        self.next = None;
    }

    /// Fill interleaved `out` (overwrites it).
    pub fn render(&mut self, out: &mut [f32]) {
        let ch = self.channels;
        for frame in out.chunks_mut(ch) {
            if self.voice.as_ref().is_none_or(Voice::done) {
                self.voice = self.next.take();
            }
            let Some(v) = self.voice.as_mut() else {
                frame.fill(0.0);
                continue;
            };
            let fade = match v.fade_left.as_mut() {
                Some(left) => {
                    let g = *left as f32 / self.fade_frames as f32;
                    *left = left.saturating_sub(1);
                    g
                }
                None => 1.0,
            };
            let gain = v.gain * fade;
            let src_ch = v.clip.channels as usize;
            for (c, s) in frame.iter_mut().enumerate() {
                *s = gain
                    * match (src_ch, ch) {
                        // Mono to every output channel.
                        (1, _) => v.clip.sample(0, v.pos),
                        // Stereo (or more) to mono: average.
                        (_, 1) => (0..src_ch).map(|k| v.clip.sample(k, v.pos)).sum::<f32>() / src_ch as f32,
                        // Map channel by channel; extra outputs stay silent.
                        _ if c < src_ch => v.clip.sample(c, v.pos),
                        _ => 0.0,
                    };
            }
            v.pos += v.step;
        }
        if self.voice.as_ref().is_some_and(Voice::done) {
            self.voice = self.next.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(frames: usize, value: f32, channels: u16, rate: u32) -> Arc<Clip> {
        Arc::new(Clip::new(vec![value; frames * channels as usize], channels, rate))
    }

    fn render(m: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![9.0; frames * m.channels()];
        m.render(&mut out);
        out
    }

    #[test]
    fn plays_once_then_silence() {
        let mut m = Mixer::new(2, 1000);
        assert!(!m.is_active());
        assert_eq!(
            m.request(clip(3, 0.5, 1, 1000), Priority(1), 1.0),
            MixOutcome::Started
        );
        assert!(m.is_active());
        let out = render(&mut m, 5);
        assert_eq!(out, vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0]);
        assert!(!m.is_active());
        assert_eq!(m.current_priority(), None);
    }

    #[test]
    fn gain_is_applied_and_clamped() {
        let mut m = Mixer::new(1, 1000);
        m.request(clip(2, 0.5, 1, 1000), Priority(1), 0.5);
        assert_eq!(render(&mut m, 1), vec![0.25]);
        m.stop();
        m.request(clip(2, 0.5, 1, 1000), Priority(1), 3.0);
        assert_eq!(render(&mut m, 1), vec![0.5]);
    }

    #[test]
    fn higher_priority_fades_out_current() {
        let mut m = Mixer::new(1, 1000); // fade = 50 frames
        m.request(clip(1000, 1.0, 1, 1000), Priority(1), 1.0);
        render(&mut m, 10);
        assert_eq!(
            m.request(clip(1000, 0.5, 1, 1000), Priority(1), 1.0),
            MixOutcome::Busy
        );
        assert_eq!(
            m.request(clip(1000, -1.0, 1, 1000), Priority(5), 1.0),
            MixOutcome::Preempted
        );
        assert_eq!(m.current_priority(), Some(Priority(5)));
        // Same priority as the pending sound is dropped too.
        assert_eq!(
            m.request(clip(10, 0.3, 1, 1000), Priority(5), 1.0),
            MixOutcome::Busy
        );
        let out = render(&mut m, 60);
        assert_eq!(out[0], 1.0);
        assert!(out[1] < 1.0 && out[1] > 0.9, "fading: {}", out[1]);
        assert!(out[49] > 0.0 && out[49] < 0.05);
        assert_eq!(out[50], -1.0, "new sound after 50 ms fade");
        // A higher one during the fade replaces the pending sound but doesn't restart the fade.
        let mut m = Mixer::new(1, 1000);
        m.request(clip(1000, 1.0, 1, 1000), Priority(1), 1.0);
        m.request(clip(1000, 0.2, 1, 1000), Priority(3), 1.0);
        render(&mut m, 20);
        m.request(clip(1000, 0.7, 1, 1000), Priority(7), 1.0);
        let out = render(&mut m, 40);
        assert_eq!(out[30], 0.7);
    }

    #[test]
    fn resamples_and_maps_channels() {
        // 500 Hz clip on a 1000 Hz device: each source frame lasts two output frames.
        let mut m = Mixer::new(1, 1000);
        m.request(
            Arc::new(Clip::new(vec![0.0, 1.0, 0.0, 1.0], 1, 500)),
            Priority(1),
            1.0,
        );
        let out = render(&mut m, 4);
        assert_eq!(out, vec![0.0, 0.5, 1.0, 0.5]);

        // Stereo → mono averages.
        let mut m = Mixer::new(1, 1000);
        m.request(
            Arc::new(Clip::new(vec![1.0, 0.0, 1.0, 0.0], 2, 1000)),
            Priority(1),
            1.0,
        );
        assert_eq!(render(&mut m, 1), vec![0.5]);

        // Stereo → 4 channels: L, R, silent, silent.
        let mut m = Mixer::new(4, 1000);
        m.request(Arc::new(Clip::new(vec![0.25, 0.75], 2, 1000)), Priority(1), 1.0);
        assert_eq!(render(&mut m, 1), vec![0.25, 0.75, 0.0, 0.0]);
    }

    #[test]
    fn clip_info() {
        let c = Clip::new(vec![0.0; 96_000], 2, 48_000);
        assert_eq!(c.frames(), 48_000);
        assert_eq!(c.duration_ms(), 1000);
        assert_eq!(c.bytes(), 384_000);
        let c = Clip::new(vec![], 0, 0);
        assert_eq!(c.channels, 1);
        assert_eq!(c.frames(), 0);
    }
}
