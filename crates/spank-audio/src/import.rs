//! Sound import pipeline (ADR-0008):
//! size check → decode → mono/stereo (>2 channels mixed to mono) → resample to 48 kHz → trim leading silence → cap length
//! → loudness-normalize to -16 LUFS with true peak ≤ -1 dBTP.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

pub const SAMPLE_RATE: u32 = 48_000;
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_DURATION_MS: u32 = 10_000;
pub const TARGET_LUFS: f64 = -16.0;
pub const MAX_TRUE_PEAK_DBTP: f64 = -1.0;
/// Samples quieter than this count as silence when trimming the start.
pub const SILENCE_DBFS: f32 = -50.0;
/// Stop decoding after this much audio, so a huge file can't eat memory.
const MAX_DECODE_SECS: u64 = 60;
/// EBU R128 integrated loudness needs at least one 400 ms block.
const MIN_MEASURE_FRAMES: usize = SAMPLE_RATE as usize * 400 / 1000;

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("file is {0} bytes; the limit is 5 MB")]
    TooLarge(u64),
    #[error("could not read file: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported or corrupt audio: {0}")]
    Unsupported(String),
    #[error("the file contains no audible sound")]
    Silent,
    #[error("audio processing failed: {0}")]
    Processing(String),
}

/// Processed, ready-to-store audio: interleaved f32 at 48 kHz, 1 or 2 channels.
#[derive(Debug, Clone)]
pub struct ImportedSound {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    /// Input sample rate before resampling.
    pub source_rate: u32,
    pub trimmed_leading_ms: u32,
    /// True if the sound was cut to [`MAX_DURATION_MS`].
    pub truncated: bool,
    /// Length after trimming leading silence, before the 10 s cap.
    pub original_ms: u32,
    pub gain_db: f64,
    /// Loudness after normalization. Below the target when true-peak limiting kicked in.
    pub loudness_lufs: f64,
    pub true_peak_dbtp: f64,
}

impl ImportedSound {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    pub fn duration_ms(&self) -> u32 {
        (self.frames() as u64 * 1000 / self.sample_rate as u64) as u32
    }
}

/// Run the whole import pipeline on a file.
pub fn import_file(path: &Path) -> Result<ImportedSound, ImportError> {
    let size = std::fs::metadata(path)?.len();
    if size > MAX_FILE_BYTES {
        return Err(ImportError::TooLarge(size));
    }
    let hint_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let (samples, channels, rate) = decode(File::open(path)?, hint_ext.as_deref())?;
    process(samples, channels, rate)
}

/// Decode to interleaved f32. Returns (samples, channels, sample_rate).
fn decode(file: File, ext: Option<&str>) -> Result<(Vec<f32>, usize, u32), ImportError> {
    let unsupported = |e: SymError| ImportError::Unsupported(e.to_string());
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(unsupported)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| ImportError::Unsupported("no audio track".into()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| ImportError::Unsupported("no audio codec parameters".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(unsupported)?;

    let mut out: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    let mut spec: Option<(usize, u32)> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // Truncated files often end with an I/O error; keep what we decoded.
            Err(SymError::IoError(_)) if !out.is_empty() => break,
            Err(e) => return Err(unsupported(e)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue, // skip a damaged packet
            Err(e) => return Err(unsupported(e)),
        };
        let (ch, rate) = (buf.spec().channels().count(), buf.spec().rate());
        match spec {
            None => spec = Some((ch, rate)),
            Some(s) if s != (ch, rate) => {
                return Err(ImportError::Unsupported("audio format changes mid-stream".into()));
            }
            _ => {}
        }
        buf.copy_to_vec_interleaved(&mut chunk);
        out.extend_from_slice(&chunk);
        if out.len() as u64 >= MAX_DECODE_SECS * rate as u64 * ch as u64 {
            break;
        }
    }
    let (channels, rate) = spec.ok_or_else(|| ImportError::Unsupported("no audio decoded".into()))?;
    if channels == 0 || rate == 0 {
        return Err(ImportError::Unsupported(
            "invalid channel count or sample rate".into(),
        ));
    }
    Ok((out, channels, rate))
}

/// Steps after decoding. Public for tests and for callers that already have PCM.
pub fn process(samples: Vec<f32>, channels: usize, rate: u32) -> Result<ImportedSound, ImportError> {
    if channels == 0 || rate == 0 {
        return Err(ImportError::Processing(
            "invalid channel count or sample rate".into(),
        ));
    }
    let (mut samples, channels) = to_stereo_or_mono(samples, channels);
    if rate != SAMPLE_RATE {
        samples = resample(&samples, channels, rate, SAMPLE_RATE)?;
    }

    // Trim leading silence.
    let threshold = 10f32.powf(SILENCE_DBFS / 20.0);
    let first_loud = samples
        .iter()
        .position(|s| s.abs() > threshold)
        .ok_or(ImportError::Silent)?;
    let lead_frames = first_loud / channels;
    samples.drain(..lead_frames * channels);
    let trimmed_leading_ms = (lead_frames as u64 * 1000 / SAMPLE_RATE as u64) as u32;

    // Cap the length.
    let max_samples = (MAX_DURATION_MS as usize * SAMPLE_RATE as usize / 1000) * channels;
    let truncated = samples.len() > max_samples;
    let original_ms = ((samples.len() / channels) as u64 * 1000 / SAMPLE_RATE as u64) as u32;
    samples.truncate(max_samples);

    // Loudness normalization with a true-peak ceiling.
    let (loudness, peak_dbtp) = measure(&samples, channels)?;
    if !loudness.is_finite() {
        return Err(ImportError::Silent);
    }
    let mut gain_db = TARGET_LUFS - loudness;
    if peak_dbtp + gain_db > MAX_TRUE_PEAK_DBTP {
        gain_db = MAX_TRUE_PEAK_DBTP - peak_dbtp;
    }
    let gain = 10f32.powf(gain_db as f32 / 20.0);
    for s in &mut samples {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }

    Ok(ImportedSound {
        samples,
        channels: channels as u16,
        sample_rate: SAMPLE_RATE,
        source_rate: rate,
        trimmed_leading_ms,
        truncated,
        original_ms,
        gain_db,
        loudness_lufs: loudness + gain_db,
        true_peak_dbtp: peak_dbtp + gain_db,
    })
}

/// Keep mono and stereo as they are. Anything with more channels is mixed down to mono (average of
/// all channels): channel layouts vary, and keeping only front left/right would drop sounds that
/// live in the center channel. Loudness normalization restores the level afterwards.
fn to_stereo_or_mono(samples: Vec<f32>, channels: usize) -> (Vec<f32>, usize) {
    if channels <= 2 {
        return (samples, channels);
    }
    let out = samples
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect();
    (out, 1)
}

fn resample(samples: &[f32], channels: usize, from: u32, to: u32) -> Result<Vec<f32>, ImportError> {
    let err = |e: &dyn std::fmt::Display| ImportError::Processing(format!("resample: {e}"));
    let frames = samples.len() / channels;
    if frames == 0 {
        return Ok(Vec::new());
    }
    let mut r =
        Fft::<f32>::new(from as usize, to as usize, 1024, channels, FixedSync::Both).map_err(|e| err(&e))?;
    let input = InterleavedSlice::new(samples, channels, frames).map_err(|e| err(&e))?;
    let out = r.process_all(&input, frames, None).map_err(|e| err(&e))?;
    Ok(out.take_data())
}

/// (integrated loudness LUFS, max true peak dBTP). Clips shorter than one 400 ms gating block
/// are looped for measurement, so a 150 ms "ow" is measured at its real level.
fn measure(samples: &[f32], channels: usize) -> Result<(f64, f64), ImportError> {
    let err = |e: ebur128::Error| ImportError::Processing(format!("loudness: {e:?}"));
    let mut m = ebur128::EbuR128::new(
        channels as u32,
        SAMPLE_RATE,
        ebur128::Mode::I | ebur128::Mode::TRUE_PEAK,
    )
    .map_err(err)?;
    let frames = samples.len() / channels;
    if frames == 0 {
        return Err(ImportError::Silent);
    }
    let mut fed = 0;
    while fed < MIN_MEASURE_FRAMES.max(frames) {
        m.add_frames_f32(samples).map_err(err)?;
        fed += frames;
    }
    let loudness = m.loudness_global().map_err(err)?;
    let mut peak: f64 = 0.0;
    for ch in 0..channels as u32 {
        peak = peak.max(m.true_peak(ch).map_err(err)?);
    }
    Ok((loudness, 20.0 * peak.max(1e-9).log10()))
}
