//! Trace format v1 (`fixtures/traces/README.md`, ADR-0010).
//!
//! `<scenario>.ndjson`: a header line, then one sample per line sorted by time:
//! `{"t":0.50012,"a":[x,y,z]}` (accelerometer, g) or `{"t":0.50210,"lid":108}` (degrees).
//! `<scenario>.labels.json`: `{"labels":[{"label":"slap_hard","start":25.0,"end":33.0}]}`.
//! Times are seconds since the session start; in memory they become milliseconds.

use serde::{Deserialize, Serialize};
use spank_core::{AccelSample, LidSample};
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "spank-trace";
pub const VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("i/o error on {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("{path}:{line}: {msg}")]
    Parse { path: PathBuf, line: usize, msg: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceHeader {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub t0_unix_ms: f64,
    #[serde(default)]
    pub accel_interval_us: u32,
    #[serde(default)]
    pub lid_poll_hz: f32,
    #[serde(default)]
    pub accel_dropped: u64,
    /// Fields added by newer recorders are kept as-is.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl TraceHeader {
    pub fn new(scenario: impl Into<String>) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            scenario: scenario.into(),
            model: String::new(),
            os: String::new(),
            t0_unix_ms: 0.0,
            accel_interval_us: crate::ACCEL_INTERVAL_US,
            lid_poll_hz: 30.0,
            accel_dropped: 0,
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub label: String,
    /// Seconds since session start.
    pub start: f64,
    pub end: f64,
}

impl Label {
    pub fn contains_ms(&self, at_ms: u64) -> bool {
        let t = at_ms as f64 / 1000.0;
        self.start <= t && t < self.end
    }

    /// `slap_*` and `playback_slap` segments contain slaps (README, "Format v1").
    pub fn is_slap(&self) -> bool {
        self.label.starts_with("slap_") || self.label == "playback_slap"
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct LabelFile {
    labels: Vec<Label>,
}

#[derive(Deserialize)]
struct Line {
    t: f64,
    #[serde(default)]
    a: Option<[f32; 3]>,
    #[serde(default)]
    lid: Option<f32>,
}

fn ms(t: f64) -> u64 {
    (t * 1000.0).round().max(0.0) as u64
}

/// A whole recorded trace in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub header: TraceHeader,
    pub accel: Vec<AccelSample>,
    pub lid: Vec<LidSample>,
    pub labels: Vec<Label>,
}

impl Trace {
    /// Read `<name>.ndjson` and, if present, the sibling `<name>.labels.json`.
    pub fn read(path: &Path) -> Result<Self, TraceError> {
        let io_err = |source| TraceError::Io {
            path: path.to_owned(),
            source,
        };
        let file = std::fs::File::open(path).map_err(io_err)?;
        let mut trace = Self::parse(BufReader::new(file), path)?;
        let labels = labels_path(path);
        match std::fs::read(&labels) {
            Ok(bytes) => {
                let f: LabelFile = serde_json::from_slice(&bytes).map_err(|e| TraceError::Parse {
                    path: labels.clone(),
                    line: e.line(),
                    msg: e.to_string(),
                })?;
                trace.labels = f.labels;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(TraceError::Io { path: labels, source }),
        }
        Ok(trace)
    }

    /// Parse the NDJSON body (`path` is only used in error messages).
    pub fn parse(r: impl BufRead, path: &Path) -> Result<Self, TraceError> {
        let err = |line: usize, msg: String| TraceError::Parse {
            path: path.to_owned(),
            line,
            msg,
        };
        let mut lines = r.lines();
        let first = lines
            .next()
            .ok_or_else(|| err(1, "empty trace".into()))?
            .map_err(|source| TraceError::Io {
                path: path.to_owned(),
                source,
            })?;
        let header: TraceHeader = serde_json::from_str(&first).map_err(|e| err(1, e.to_string()))?;
        if header.format != FORMAT || header.version != VERSION {
            return Err(err(
                1,
                format!(
                    "unsupported trace {:?} v{} (expected {FORMAT} v{VERSION})",
                    header.format, header.version
                ),
            ));
        }
        let mut accel = Vec::new();
        let mut lid = Vec::new();
        for (i, line) in lines.enumerate() {
            let n = i + 2;
            let line = line.map_err(|source| TraceError::Io {
                path: path.to_owned(),
                source,
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let l: Line = serde_json::from_str(&line).map_err(|e| err(n, e.to_string()))?;
            match (l.a, l.lid) {
                (Some([x, y, z]), None) => accel.push(AccelSample {
                    at_ms: ms(l.t),
                    x,
                    y,
                    z,
                }),
                (None, Some(angle_deg)) => lid.push(LidSample {
                    at_ms: ms(l.t),
                    angle_deg,
                }),
                _ => return Err(err(n, "a sample needs exactly one of \"a\" or \"lid\"".into())),
            }
        }
        Ok(Self {
            header,
            accel,
            lid,
            labels: Vec::new(),
        })
    }

    /// Measured accelerometer rate (samples / span), or `None` with fewer than two samples.
    pub fn accel_rate_hz(&self) -> Option<f32> {
        let (first, last) = (self.accel.first()?, self.accel.last()?);
        let span = last.at_ms.checked_sub(first.at_ms).filter(|&s| s > 0)?;
        Some((self.accel.len() - 1) as f32 * 1000.0 / span as f32)
    }

    pub fn label_at(&self, at_ms: u64) -> Option<&Label> {
        self.labels.iter().find(|l| l.contains_ms(at_ms))
    }

    /// Write this trace as `<dir>/<scenario>.ndjson` (+ labels).
    pub fn write(&self, dir: &Path) -> io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.ndjson", self.header.scenario));
        let mut w = TraceWriter::new(io::BufWriter::new(std::fs::File::create(&path)?), &self.header)?;
        // Merge both streams by time, accelerometer first on ties (same as trace-recorder).
        let (mut i, mut j) = (0, 0);
        while i < self.accel.len() || j < self.lid.len() {
            if j >= self.lid.len() || (i < self.accel.len() && self.accel[i].at_ms <= self.lid[j].at_ms) {
                w.accel(&self.accel[i])?;
                i += 1;
            } else {
                w.lid(&self.lid[j])?;
                j += 1;
            }
        }
        w.finish()?;
        if !self.labels.is_empty() {
            write_labels(&labels_path(&path), &self.labels)?;
        }
        Ok(path)
    }
}

/// `x/baseline.ndjson` → `x/baseline.labels.json`.
pub fn labels_path(trace: &Path) -> PathBuf {
    let stem = trace
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    trace.with_file_name(format!("{stem}.labels.json"))
}

pub fn write_labels(path: &Path, labels: &[Label]) -> io::Result<()> {
    let body = serde_json::to_vec_pretty(&LabelFile {
        labels: labels.to_vec(),
    })
    .map_err(io::Error::other)?;
    std::fs::write(path, body)
}

/// Streaming NDJSON writer.
pub struct TraceWriter<W: Write> {
    w: W,
}

impl<W: Write> TraceWriter<W> {
    pub fn new(mut w: W, header: &TraceHeader) -> io::Result<Self> {
        serde_json::to_writer(&mut w, header).map_err(io::Error::other)?;
        w.write_all(b"\n")?;
        Ok(Self { w })
    }

    pub fn accel(&mut self, s: &AccelSample) -> io::Result<()> {
        writeln!(
            self.w,
            "{{\"t\":{:.3},\"a\":[{:.5},{:.5},{:.5}]}}",
            s.at_ms as f64 / 1000.0,
            s.x,
            s.y,
            s.z
        )
    }

    pub fn lid(&mut self, s: &LidSample) -> io::Result<()> {
        writeln!(
            self.w,
            "{{\"t\":{:.3},\"lid\":{}}}",
            s.at_ms as f64 / 1000.0,
            s.angle_deg
        )
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.w.flush()?;
        Ok(self.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"{"format":"spank-trace","version":1,"scenario":"t","model":"Mac16,13","os":"26","t0_unix_ms":1.0,"accel_interval_us":1000,"lid_poll_hz":30,"accel_dropped":0,"future":"x"}
{"t":0.5,"a":[0.01,0.02,-0.99]}
{"t":0.5004,"lid":108}

{"t":0.502,"a":[0.0,0.0,-1.0]}
"#;

    fn parse(s: &str) -> Result<Trace, TraceError> {
        Trace::parse(s.as_bytes(), Path::new("t.ndjson"))
    }

    #[test]
    fn parses_header_and_samples() {
        let t = parse(BODY).unwrap();
        assert_eq!(t.header.model, "Mac16,13");
        assert_eq!(t.header.extra["future"], "x");
        assert_eq!(t.accel.len(), 2);
        assert_eq!(t.accel[1].at_ms, 502);
        assert_eq!(
            t.lid,
            vec![LidSample {
                at_ms: 500,
                angle_deg: 108.0
            }]
        );
        assert_eq!(t.accel_rate_hz(), Some(500.0));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("").is_err());
        assert!(parse(r#"{"format":"other","version":1}"#).is_err());
        assert!(parse(r#"{"format":"spank-trace","version":2}"#).is_err());
        let bad = format!("{}\n{{\"t\":1}}\n", BODY.lines().next().unwrap());
        match parse(&bad) {
            Err(TraceError::Parse { line, .. }) => assert_eq!(line, 2),
            other => panic!("{other:?}"),
        }
        let both = format!(
            "{}\n{{\"t\":1,\"a\":[0,0,0],\"lid\":3}}\n",
            BODY.lines().next().unwrap()
        );
        assert!(parse(&both).is_err());
        assert!(Trace::read(Path::new("/nonexistent/x.ndjson")).is_err());
    }

    #[test]
    fn write_read_round_trip_with_labels() {
        let mut t = parse(BODY).unwrap();
        t.labels = vec![Label {
            label: "slap_light".into(),
            start: 0.4,
            end: 0.6,
        }];
        let dir = tempfile::tempdir().unwrap();
        let path = t.write(dir.path()).unwrap();
        assert!(path.ends_with("t.ndjson"));
        let back = Trace::read(&path).unwrap();
        assert_eq!(back.accel, t.accel);
        assert_eq!(back.lid, t.lid);
        assert_eq!(back.labels, t.labels);
        assert_eq!(back.header.scenario, "t");
        assert!(back.label_at(500).unwrap().is_slap());
        assert!(back.label_at(700).is_none());
        assert_eq!(TraceHeader::new("x").version, VERSION);
    }

    #[test]
    fn labels_path_uses_stem() {
        assert_eq!(
            labels_path(Path::new("a/baseline.ndjson")),
            PathBuf::from("a/baseline.labels.json")
        );
    }
}
