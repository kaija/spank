//! Updates (T41): a signed `latest.json` on GitHub Releases, checked once a day, installed only
//! after the user says yes.
//!
//! - Endpoint and public key live in `tauri.conf.json` (`plugins.updater`); `tools/release.sh`
//!   signs the `.app.tar.gz` and writes `latest.json` with the private key (docs/release.md).
//! - Release builds check [`FIRST_CHECK_DELAY`] after launch and then every [`CHECK_INTERVAL`],
//!   when `auto_check_updates` is on. A found update asks with a native dialog; "Later" is not
//!   asked again for the same version until the next launch. Dev builds never check by themselves.
//! - Settings › General can check and install on demand (`check_for_updates`, `install_update`);
//!   download progress goes out as `update_progress` (0–1).
//! - Only the version number leaves the Mac (the request for `latest.json`).

use crate::state::{AppState, CmdResult, err};
use crate::tray::{Lang, resolve_lang, system_language};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(120);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const UPDATE_PROGRESS: &str = "update_progress";

/// The last update found, kept so installing doesn't need a second check.
#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<Update>>,
    /// Version the user answered "Later" to in this run.
    declined: Mutex<Option<String>>,
    installing: Mutex<bool>,
}

/// Result of `check_for_updates`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    /// `None` when this is the newest version.
    pub available: Option<AvailableUpdate>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
}

fn info(app: &AppHandle, update: Option<&Update>) -> UpdateInfo {
    UpdateInfo {
        current: app.package_info().version.to_string(),
        available: update.map(|u| AvailableUpdate {
            version: u.version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        }),
    }
}

/// Ask the endpoint; remember what was found.
pub async fn check(app: &AppHandle) -> CmdResult<UpdateInfo> {
    let update = app.updater().map_err(err)?.check().await.map_err(err)?;
    let result = info(app, update.as_ref());
    if let Some(updates) = app.try_state::<Updates>()
        && let Ok(mut p) = updates.pending.lock()
    {
        *p = update;
    }
    Ok(result)
}

/// Download, verify and install the pending update, then restart.
pub async fn install(app: &AppHandle) -> CmdResult<()> {
    let updates = app.state::<Updates>();
    {
        let mut busy = updates.installing.lock().map_err(err)?;
        if *busy {
            return Err("an update is already being installed".into());
        }
        *busy = true;
    }
    let pending = updates.pending.lock().map_err(err)?.clone();
    let result = async {
        let update = match pending {
            Some(u) => u,
            None => app
                .updater()
                .map_err(err)?
                .check()
                .await
                .map_err(err)?
                .ok_or("already up to date")?,
        };
        let mut got = 0usize;
        let handle = app.clone();
        update
            .download_and_install(
                move |chunk, total| {
                    got += chunk;
                    if let Some(total) = total.filter(|t| *t > 0) {
                        let _ = handle.emit(UPDATE_PROGRESS, (got as f64 / total as f64).min(1.0));
                    }
                },
                || {},
            )
            .await
            .map_err(err)
    }
    .await;
    if let Ok(mut busy) = updates.installing.lock() {
        *busy = false;
    }
    result?;
    eprintln!("spank: update installed, restarting");
    if let Some(state) = app.try_state::<AppState>() {
        // Restore the accelerometer properties before the process is replaced.
        state.engine.shutdown();
    }
    app.restart()
}

/// Dialog strings (the native dialog is outside the web UI's i18n).
pub fn ask_text(lang: Lang, version: &str, current: &str) -> (String, String, &'static str, &'static str) {
    match lang {
        Lang::Zh => (
            "有新版本".into(),
            format!("Spank {version} 可以更新了（目前是 {current}）。要現在下載並重新啟動嗎？"),
            "更新",
            "稍後",
        ),
        Lang::En => (
            "Update available".into(),
            format!("Spank {version} is available (you have {current}). Download it and restart now?"),
            "Update",
            "Later",
        ),
    }
}

/// Ask about a found update (blocking: call off the main thread).
fn ask(app: &AppHandle, available: &AvailableUpdate, current: &str) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let lang = resolve_lang(state.config().language, system_language().as_deref());
    let (title, body, yes, no) = ask_text(lang, &available.version, current);
    let body = match &available.notes {
        Some(n) => format!("{body}\n\n{n}"),
        None => body,
    };
    app.dialog()
        .message(body)
        .title(title)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(yes.into(), no.into()))
        .blocking_show()
}

/// One scheduled check: ask if something new turned up and the user hasn't declined it.
fn scheduled_check(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if !state.config().auto_check_updates {
        return;
    }
    let found = match tauri::async_runtime::block_on(check(app)) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("spank: update check failed: {e}");
            return;
        }
    };
    let Some(available) = found.available else {
        return;
    };
    let updates = app.state::<Updates>();
    if updates
        .declined
        .lock()
        .is_ok_and(|d| d.as_deref() == Some(available.version.as_str()))
    {
        return;
    }
    if ask(app, &available, &found.current) {
        if let Err(e) = tauri::async_runtime::block_on(install(app)) {
            eprintln!("spank: update failed: {e}");
        }
    } else if let Ok(mut d) = updates.declined.lock() {
        *d = Some(available.version);
    }
}

/// Start the daily check (release builds only).
pub fn start_schedule(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("spank-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK_DELAY);
            loop {
                scheduled_check(&app);
                std::thread::sleep(CHECK_INTERVAL);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_text_names_both_versions() {
        let (_, body, yes, no) = ask_text(Lang::Zh, "0.2.0", "0.1.0");
        assert!(body.contains("0.2.0") && body.contains("0.1.0"));
        assert_eq!((yes, no), ("更新", "稍後"));
        let (title, body, ..) = ask_text(Lang::En, "0.2.0", "0.1.0");
        assert_eq!(title, "Update available");
        assert!(body.starts_with("Spank 0.2.0 is available"));
    }

    #[test]
    fn schedule_is_daily() {
        assert_eq!(CHECK_INTERVAL.as_secs(), 86_400);
        assert!(FIRST_CHECK_DELAY < CHECK_INTERVAL);
    }
}
