//! Diagnostics for bug reports and the log file (T38, Settings › General).
//!
//! The diagnostics text only holds the app version, the Mac model and macOS version, sensor and
//! permission states, and a few switches — nothing personal (no file names, sounds or profiles).
//!
//! Logging: the app writes with `eprintln!`. When it is not attached to a terminal (launched from
//! Finder or at login), stderr goes to `~/Library/Logs/<bundle id>/spank.log`, rotated to
//! `spank.log.1` past [`LOG_MAX_BYTES`].

use spank_core::{AppConfig, Capability, RuntimeState};
use std::ffi::CString;
use std::path::PathBuf;

pub const LOG_MAX_BYTES: u64 = 1024 * 1024;
const LOG_FILE: &str = "spank.log";

/// A string `sysctl` value such as `hw.model`.
pub fn sysctl(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    // SAFETY: the first call only asks for the size; the second writes at most `len` bytes.
    unsafe {
        if libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        ) != 0
            || len == 0
        {
            return None;
        }
        let mut buf = vec![0u8; len];
        if libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        buf.truncate(len);
        let s = String::from_utf8_lossy(&buf)
            .trim_end_matches('\0')
            .trim()
            .to_owned();
        (!s.is_empty()).then_some(s)
    }
}

/// What the machine is, for the diagnostics text.
pub struct Machine {
    pub model: Option<String>,
    pub chip: Option<String>,
    pub os: Option<String>,
}

impl Machine {
    pub fn current() -> Self {
        Self {
            model: sysctl("hw.model"),
            chip: sysctl("machdep.cpu.brand_string"),
            os: sysctl("kern.osproductversion"),
        }
    }
}

fn cap(c: &Capability) -> String {
    match c {
        Capability::Unknown => "unknown".into(),
        Capability::Available => "available".into(),
        Capability::Unsupported { reason } => format!("unsupported ({reason})"),
    }
}

fn on(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// The text copied by "Copy diagnostics".
pub fn report(version: &str, m: &Machine, s: &RuntimeState, c: &AppConfig, calibrated: bool) -> String {
    let unknown = || "unknown".to_owned();
    let mute: Vec<String> = s.mute.reasons.iter().map(|r| format!("{r:?}")).collect();
    [
        format!("Spank {version}"),
        format!("macOS {}", m.os.clone().unwrap_or_else(unknown)),
        format!(
            "Mac: {} ({})",
            m.model.clone().unwrap_or_else(unknown),
            m.chip.clone().unwrap_or_else(unknown)
        ),
        format!("Lid angle sensor: {}", cap(&s.caps.lid)),
        format!("Accelerometer: {}", cap(&s.caps.accel)),
        format!("Input Monitoring: {:?}", s.input_access),
        format!("Focus status: {:?}", s.focus_authorization),
        format!("Audio output: {}", if s.audio_ok { "ok" } else { "unavailable" }),
        format!(
            "Running: paused={}, lid={}, accel={}, input={}",
            s.paused, s.lid_running, s.accel_running, s.input_running
        ),
        format!(
            "Muted by: {}",
            if mute.is_empty() {
                "nothing".into()
            } else {
                mute.join(", ")
            }
        ),
        format!(
            "Slaps: {}, calibrated: {}, precise typing: {}, levels: {:.2}/{:.2}/{:.2} g",
            on(c.slap.enabled),
            if calibrated { "yes" } else { "no" },
            on(c.slap.precise_typing),
            c.slap.levels.light_g,
            c.slap.levels.medium_g,
            c.slap.levels.hard_g
        ),
        format!(
            "Lid thresholds: open {}°, close {}°, overbend {}°",
            c.lid.open_min_deg, c.lid.close_threshold_deg, c.lid.overbend_threshold_deg
        ),
        format!("Language: {:?}", c.language),
    ]
    .join("\n")
}

/// `~/Library/Logs/<bundle id>`.
pub fn log_dir(bundle_id: &str) -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Logs").join(bundle_id))
}

/// Send stderr to the log file unless we run in a terminal. Returns the log file path.
pub fn redirect_stderr(dir: &std::path::Path) -> std::io::Result<Option<PathBuf>> {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    // SAFETY: isatty only inspects the descriptor.
    if unsafe { libc::isatty(libc::STDERR_FILENO) } == 1 {
        return Ok(None);
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(LOG_FILE);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
        let _ = std::fs::rename(&path, dir.join(format!("{LOG_FILE}.1")));
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    writeln!(file, "---- spank started (unix {secs}) ----")?;
    // SAFETY: both descriptors are valid; dup2 atomically replaces stderr.
    if unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_has_no_surprises() {
        let m = Machine {
            model: Some("Mac16,12".into()),
            chip: Some("Apple M4".into()),
            os: None,
        };
        let mut s = RuntimeState::default();
        s.caps.accel = Capability::Unsupported {
            reason: "no driver".into(),
        };
        let r = report("0.1.0", &m, &s, &AppConfig::default(), false);
        assert!(
            r.starts_with("Spank 0.1.0\nmacOS unknown\nMac: Mac16,12 (Apple M4)"),
            "{r}"
        );
        assert!(r.contains("Accelerometer: unsupported (no driver)"));
        assert!(r.contains("Slaps: off, calibrated: no, precise typing: off, levels: 0.15/0.30/0.50 g"));
        assert!(r.contains("Muted by: nothing"));
    }

    #[test]
    fn sysctl_reads_strings() {
        assert!(sysctl("kern.ostype").is_some_and(|s| s == "Darwin"));
        assert_eq!(sysctl("no.such.key"), None);
    }

    #[test]
    fn log_dir_is_under_library_logs() {
        let d = log_dir("dev.spank.app").unwrap();
        assert!(d.ends_with("Library/Logs/dev.spank.app"));
    }
}
