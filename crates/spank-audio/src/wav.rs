use crate::import::ImportedSound;
use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Write as 16-bit PCM WAV, atomically (temp file + rename). Returns the hex sha256 of the file.
pub fn write_wav(sound: &ImportedSound, path: &Path) -> io::Result<String> {
    let spec = hound::WavSpec {
        channels: sound.channels,
        sample_rate: sound.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let tmp = path.with_extension("wav.tmp");
    let result = (|| -> io::Result<()> {
        let mut w = hound::WavWriter::create(&tmp, spec).map_err(to_io)?;
        for &s in &sound.samples {
            w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
                .map_err(to_io)?;
        }
        w.finalize().map_err(to_io)?;
        std::fs::File::open(&tmp)?.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    sha256_file(path)
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Read a stored WAV back into interleaved f32 (for playback preloading). Returns (samples, channels, rate).
pub fn read_wav(path: &Path) -> io::Result<(Vec<f32>, u16, u32)> {
    let mut r = hound::WavReader::open(path).map_err(to_io)?;
    let spec = r.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
                .map_err(to_io)?
        }
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(to_io)?,
    };
    Ok((samples, spec.channels, spec.sample_rate))
}

fn to_io(e: hound::Error) -> io::Error {
    match e {
        hound::Error::IoError(e) => e,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}
