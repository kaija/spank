//! Tauri commands for the settings UI and the onboarding wizard (design §10.3). Every command
//! returns `Result<_, String>` so errors reach the UI as messages. Changes are announced with
//! events: `config_changed` (`AppConfig`), `profiles_changed`, `library_changed` (no payload),
//! plus the engine's `engine_state`, `trigger_fired`, `calibration` and `accel_level`.

use crate::engine::EngineCmd;
use crate::state::{AppState, CmdResult, err};
use serde::Serialize;
use spank_core::{
    AppConfig, CalStep, Calibration, EventKind, FocusAuthorization, ImportOutcome, InputAccess, Language,
    LidConfig, MuteRules, PackImported, Profile, ProfileId, RuntimeState, SlapConfig, SoundEntry, SoundId,
    SoundSource,
};
use spank_pack::ExportMeta;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

pub const PROFILES_CHANGED: &str = "profiles_changed";
pub const LIBRARY_CHANGED: &str = "library_changed";

fn notify(app: &AppHandle, event: &str) {
    let _ = app.emit(event, ());
    crate::tray::refresh(app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    default_config: AppConfig,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        default_config: AppConfig::default(),
    }
}

// ---- state & config ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> RuntimeState {
    state.engine.state()
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.config()
}

#[tauri::command]
pub fn set_paused(app: AppHandle, state: State<'_, AppState>, paused: bool) -> CmdResult<AppConfig> {
    let c = state.update_config(|c| c.paused = paused)?;
    crate::tray::refresh(&app);
    Ok(c)
}

#[tauri::command]
pub fn set_mute_rules(state: State<'_, AppState>, rules: MuteRules) -> CmdResult<AppConfig> {
    rules.validate().map_err(err)?;
    state.update_config(|c| c.mute = rules)
}

#[tauri::command]
pub fn set_slap_enabled(state: State<'_, AppState>, enabled: bool) -> CmdResult<AppConfig> {
    state.update_config(|c| c.slap.enabled = enabled)
}

#[tauri::command]
pub fn set_lid_config(state: State<'_, AppState>, lid: LidConfig) -> CmdResult<AppConfig> {
    lid.validate().map_err(err)?;
    state.update_config(|c| c.lid = lid)
}

#[tauri::command]
pub fn set_slap_config(state: State<'_, AppState>, slap: SlapConfig) -> CmdResult<AppConfig> {
    slap.validate().map_err(err)?;
    state.update_config(|c| c.slap = slap)
}

#[tauri::command]
pub fn set_app_volume(state: State<'_, AppState>, volume: f32) -> CmdResult<AppConfig> {
    state.update_config(|c| c.app_volume = volume.clamp(0.0, 1.0))
}

#[tauri::command]
pub fn set_language(state: State<'_, AppState>, language: Language) -> CmdResult<AppConfig> {
    state.update_config(|c| c.language = language)
}

#[tauri::command]
pub fn set_auto_check_updates(state: State<'_, AppState>, enabled: bool) -> CmdResult<AppConfig> {
    state.update_config(|c| c.auto_check_updates = enabled)
}

/// Register / unregister the login item, then remember the choice.
pub fn apply_launch_at_login(app: &AppHandle, enabled: bool) -> CmdResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    if m.is_enabled().unwrap_or(!enabled) == enabled {
        return Ok(());
    }
    if enabled { m.enable() } else { m.disable() }.map_err(err)
}

#[tauri::command]
pub fn set_launch_at_login(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CmdResult<AppConfig> {
    apply_launch_at_login(&app, enabled)?;
    state.update_config(|c| c.launch_at_login = enabled)
}

// ---- onboarding & calibration (T36) -----------------------------------------------------------

#[tauri::command]
pub fn get_calibration(state: State<'_, AppState>) -> CmdResult<Option<Calibration>> {
    Ok(state.store.load_calibration().map_err(err)?.value)
}

#[tauri::command]
pub fn open_calibration(app: AppHandle) {
    crate::window::open_wizard(&app, crate::window::Wizard::Calibration);
}

#[tauri::command]
pub fn start_calibration(state: State<'_, AppState>) {
    state.engine.send(EngineCmd::StartCalibration);
}

#[tauri::command]
pub fn calibration_step(state: State<'_, AppState>, step: CalStep, reset: bool) {
    state.engine.send(EngineCmd::CalibrationStep { step, reset });
}

#[tauri::command]
pub fn cancel_calibration(state: State<'_, AppState>) {
    state.engine.send(EngineCmd::CancelCalibration);
}

/// End the wizard: save `calibration.json`, apply the new levels and turn slaps on.
#[tauri::command]
pub async fn finish_calibration(app: AppHandle) -> CmdResult<AppConfig> {
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<AppConfig> {
        let state = app.state::<AppState>();
        let (tx, rx) = std::sync::mpsc::channel();
        state.engine.send(EngineCmd::FinishCalibration(tx));
        let (calibration, slap) = rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| "the engine did not answer".to_owned())??;
        state.store.save_calibration(&calibration).map_err(err)?;
        state.update_config(|c| {
            c.slap.levels = slap.levels;
            c.slap.trigger_g = slap.trigger_g;
            c.slap.enabled = true;
        })
    })
    .await
    .map_err(err)?
}

/// The live acceleration waveform on the Sensors page (event `accel_level`).
#[tauri::command]
pub fn set_accel_monitor(state: State<'_, AppState>, on: bool) {
    state.engine.send(EngineCmd::Monitor(on));
}

/// Last onboarding step: remember it and close the wizard window.
#[tauri::command]
pub fn complete_onboarding(state: State<'_, AppState>, window: tauri::WebviewWindow) -> CmdResult<()> {
    if !state.config().onboarding_done {
        state.update_config(|c| c.onboarding_done = true)?;
    }
    window.close().map_err(err)
}

// ---- precise typing (T39) ---------------------------------------------------------------------

/// Ask for Input Monitoring (system prompt the first time). Resolves with the current state.
#[tauri::command]
pub async fn request_input_monitoring(app: AppHandle) -> CmdResult<InputAccess> {
    let access = tauri::async_runtime::spawn_blocking(|| {
        spank_sensors::request_input_access();
        spank_sensors::input_access()
    })
    .await
    .map_err(err)?;
    app.state::<AppState>().engine.send(EngineCmd::CheckInputAccess);
    Ok(access)
}

/// Open System Settings › Privacy & Security › Input Monitoring.
#[tauri::command]
pub fn open_input_monitoring_settings() -> CmdResult<()> {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
}

// ---- diagnostics & logs (T38) -----------------------------------------------------------------

fn diagnostics_text(app: &AppHandle, state: &AppState) -> String {
    let calibrated = state.store.load_calibration().is_ok_and(|c| c.value.is_some());
    crate::diag::report(
        &app.package_info().version.to_string(),
        &crate::diag::Machine::current(),
        &state.engine.state(),
        &state.config(),
        calibrated,
    )
}

#[tauri::command]
pub fn diagnostics(app: AppHandle, state: State<'_, AppState>) -> String {
    diagnostics_text(&app, &state)
}

/// Put the diagnostics text on the clipboard and return it.
#[tauri::command]
pub fn copy_diagnostics(app: AppHandle, state: State<'_, AppState>) -> CmdResult<String> {
    use std::io::Write;
    let text = diagnostics_text(&app, &state);
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(err)?;
    child
        .stdin
        .take()
        .ok_or("pbcopy has no stdin")?
        .write_all(text.as_bytes())
        .map_err(err)?;
    child.wait().map_err(err)?;
    Ok(text)
}

/// Show the log folder in Finder (created if missing).
#[tauri::command]
pub fn open_log_folder() -> CmdResult<()> {
    let dir = crate::diag::log_dir(crate::BUNDLE_ID).ok_or("no home directory")?;
    std::fs::create_dir_all(&dir).map_err(err)?;
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(err)
}

fn open_url(url: &str) -> CmdResult<()> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(err)
}

// ---- profiles ---------------------------------------------------------------------------------

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> CmdResult<Vec<Profile>> {
    state.profiles.list(&state.store).map_err(err)
}

#[tauri::command]
pub fn create_profile(app: AppHandle, state: State<'_, AppState>, name: String) -> CmdResult<Profile> {
    let p = state.profiles.create(&state.store, &name).map_err(err)?;
    notify(&app, PROFILES_CHANGED);
    Ok(p)
}

#[tauri::command]
pub fn duplicate_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ProfileId,
    name: String,
) -> CmdResult<Profile> {
    let p = state.profiles.duplicate(&state.store, &id, &name).map_err(err)?;
    notify(&app, PROFILES_CHANGED);
    Ok(p)
}

#[tauri::command]
pub fn save_profile(app: AppHandle, state: State<'_, AppState>, profile: Profile) -> CmdResult<()> {
    state.profiles.save(&state.store, &profile).map_err(err)?;
    if state.config().active_profile == profile.id {
        state.refresh_active();
    }
    notify(&app, PROFILES_CHANGED);
    Ok(())
}

#[tauri::command]
pub fn rename_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    id: ProfileId,
    name: String,
) -> CmdResult<Profile> {
    let p = state.profiles.rename(&state.store, &id, &name).map_err(err)?;
    notify(&app, PROFILES_CHANGED);
    Ok(p)
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, state: State<'_, AppState>, id: ProfileId) -> CmdResult<()> {
    let mut cfg = state.config();
    let was_active = cfg.active_profile == id;
    state.profiles.delete(&state.store, &mut cfg, &id).map_err(err)?;
    if was_active {
        state.update_config(|c| c.active_profile = cfg.active_profile.clone())?;
        state.refresh_active();
    }
    notify(&app, PROFILES_CHANGED);
    Ok(())
}

#[tauri::command]
pub fn set_active_profile(app: AppHandle, state: State<'_, AppState>, id: ProfileId) -> CmdResult<()> {
    let p = state.profiles.get(&state.store, &id).map_err(err)?;
    state.update_config(|c| c.active_profile = id)?;
    state.engine.send(EngineCmd::Profile(p));
    notify(&app, PROFILES_CHANGED);
    Ok(())
}

// ---- sound library ----------------------------------------------------------------------------

#[tauri::command]
pub fn list_sounds(state: State<'_, AppState>) -> CmdResult<Vec<SoundEntry>> {
    state.sounds()
}

static SOUND_SEQ: AtomicU32 = AtomicU32::new(0);

fn new_sound_id() -> SoundId {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    SoundId(format!("user-{ms}-{}", SOUND_SEQ.fetch_add(1, Ordering::Relaxed)))
}

fn import_one(state: &AppState, path: &std::path::Path) -> ImportOutcome {
    use spank_audio::ImportError;
    let fail = |reason: &str, message: String| ImportOutcome::Failed {
        reason: reason.into(),
        message,
    };
    let sound = match spank_audio::import_file(path) {
        Ok(s) => s,
        Err(e) => {
            let reason = match e {
                ImportError::TooLarge(_) => "too_large",
                ImportError::Unsupported(_) => "unsupported",
                ImportError::Silent => "silent",
                _ => "other",
            };
            return fail(reason, e.to_string());
        }
    };
    let id = new_sound_id();
    let dest = match state.store.sound_file(&id) {
        Ok(p) => p,
        Err(e) => return fail("other", e.to_string()),
    };
    let sha256 = match spank_audio::write_wav(&sound, &dest) {
        Ok(h) => h,
        Err(e) => return fail("other", e.to_string()),
    };
    let original_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| id.0.clone());
    let entry = SoundEntry {
        id,
        name,
        source: SoundSource::User { original_name },
        duration_ms: sound.duration_ms(),
        sha256,
        license: None,
        added_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    if let Err(e) = state.store.add_sound(entry.clone()) {
        let _ = std::fs::remove_file(&dest);
        return fail("other", e.to_string());
    }
    ImportOutcome::Added {
        truncated: sound.truncated,
        original_ms: sound.original_ms,
        sound: entry,
    }
}

/// Import one audio file into the library (decode, resample, trim, normalize).
#[tauri::command]
pub async fn import_sound(app: AppHandle, path: String) -> CmdResult<ImportOutcome> {
    let a = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let state = a.state::<AppState>();
        import_one(&state, std::path::Path::new(&path))
    })
    .await
    .map_err(err)?;
    if matches!(outcome, ImportOutcome::Added { .. }) {
        notify(&app, LIBRARY_CHANGED);
    }
    Ok(outcome)
}

#[tauri::command]
pub fn rename_sound(app: AppHandle, state: State<'_, AppState>, id: SoundId, name: String) -> CmdResult<()> {
    if state.is_builtin_sound(&id) {
        return Err("built-in sounds are read-only".into());
    }
    state.store.rename_sound(&id, &name).map_err(err)?;
    notify(&app, LIBRARY_CHANGED);
    Ok(())
}

/// Delete a user sound and remove it from every user profile. Returns the profiles changed.
#[tauri::command]
pub fn delete_sound(app: AppHandle, state: State<'_, AppState>, id: SoundId) -> CmdResult<Vec<ProfileId>> {
    if state.is_builtin_sound(&id) {
        return Err("built-in sounds are read-only".into());
    }
    state.store.remove_sound(&id).map_err(err)?;
    let changed = state.unlink_sound(&id).map_err(err)?;
    state.refresh_active();
    notify(&app, LIBRARY_CHANGED);
    notify(&app, PROFILES_CHANGED);
    Ok(changed)
}

/// Peak envelope for the waveform thumbnail: `buckets` values in 0..=1.
pub fn peaks(samples: &[f32], channels: u16, buckets: usize) -> Vec<f32> {
    let frames = samples.len() / channels.max(1) as usize;
    if frames == 0 || buckets == 0 {
        return vec![0.0; buckets];
    }
    let mut out: Vec<f32> = (0..buckets)
        .map(|b| {
            let (lo, hi) = (
                b * frames / buckets,
                ((b + 1) * frames / buckets).max(b * frames / buckets + 1),
            );
            samples[lo * channels as usize..(hi * channels as usize).min(samples.len())]
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs()))
        })
        .collect();
    let max = out.iter().copied().fold(0.0f32, f32::max);
    if max > 0.0 {
        out.iter_mut().for_each(|v| *v /= max);
    }
    out
}

#[tauri::command]
pub async fn sound_peaks(app: AppHandle, id: SoundId) -> CmdResult<Vec<f32>> {
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<Vec<f32>> {
        let state = app.state::<AppState>();
        let (samples, channels, _) = match state.builtin.audio.get(&id) {
            Some(b) => spank_audio::decode_wav(b).map_err(err)?,
            None => spank_audio::read_wav(&state.store.sound_file(&id).map_err(err)?).map_err(err)?,
        };
        Ok(peaks(&samples, channels, 32))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn preview_sound(state: State<'_, AppState>, id: SoundId) {
    state.engine.send(EngineCmd::PreviewSound(id));
}

#[tauri::command]
pub fn stop_preview(state: State<'_, AppState>) {
    state.engine.send(EngineCmd::StopPreview);
}

#[tauri::command]
pub fn preview_event(state: State<'_, AppState>, kind: EventKind) {
    state.engine.send(EngineCmd::PreviewEvent(kind));
}

/// Show a user sound's WAV file in Finder.
#[tauri::command]
pub fn reveal_sound(state: State<'_, AppState>, id: SoundId) -> CmdResult<()> {
    let path = state.store.sound_file(&id).map_err(err)?;
    if !path.exists() {
        return Err("built-in sounds live inside the app".into());
    }
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(err)?;
    Ok(())
}

// ---- sound packs ------------------------------------------------------------------------------

#[tauri::command]
pub async fn import_pack(app: AppHandle, path: String) -> CmdResult<PackImported> {
    let a = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> CmdResult<PackImported> {
        let state = a.state::<AppState>();
        let pack = spank_pack::read_pack_file(&PathBuf::from(&path)).map_err(err)?;
        let r = spank_pack::import_pack(&state.store, &state.profiles, &pack).map_err(err)?;
        Ok(PackImported {
            sounds: r.sounds.len() as u32,
            profile: r.profile,
        })
    })
    .await
    .map_err(err)??;
    notify(&app, LIBRARY_CHANGED);
    notify(&app, PROFILES_CHANGED);
    Ok(result)
}

fn slug(s: &str) -> String {
    let v: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let v = v.trim_matches('-').to_owned();
    if v.is_empty() { "pack".into() } else { v }
}

/// Export a profile and its sounds as a `.spankpack`.
#[tauri::command]
pub async fn export_pack(app: AppHandle, profile_id: ProfileId, path: String) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<()> {
        let state = app.state::<AppState>();
        let profile = state.profiles.get(&state.store, &profile_id).map_err(err)?;
        let sounds = state.sounds()?;
        let builtin = state.builtin.clone();
        let meta = ExportMeta {
            id: format!("dev.spank.user.{}", slug(&profile.id.0)),
            name: BTreeMap::from([
                ("zh-Hant".into(), profile.name.clone()),
                ("en".into(), profile.name.clone()),
            ]),
            version: "1.0.0".into(),
            author: None,
            license: "NOASSERTION".into(),
        };
        let pack = spank_pack::export_pack_with(&profile, meta, |id| {
            let entry = sounds.iter().find(|s| &s.id == id)?.clone();
            let bytes = match builtin.audio.get(id) {
                Some(b) => b.as_ref().to_vec(),
                None => std::fs::read(state.store.sound_file(id).ok()?).ok()?,
            };
            Some((entry, bytes))
        })
        .map_err(err)?;
        let file = std::fs::File::create(&path).map_err(err)?;
        spank_pack::write_pack(file, &pack).map_err(err)
    })
    .await
    .map_err(err)?
}

// ---- Focus permission -------------------------------------------------------------------------

pub fn focus_auth_now() -> FocusAuthorization {
    match spank_sysmon::focus_authorization() {
        spank_sysmon::FocusAuthorization::Authorized => FocusAuthorization::Authorized,
        spank_sysmon::FocusAuthorization::Denied => FocusAuthorization::Denied,
        spank_sysmon::FocusAuthorization::Restricted => FocusAuthorization::Restricted,
        spank_sysmon::FocusAuthorization::NotDetermined => FocusAuthorization::NotDetermined,
    }
}

/// Show the system prompt for Focus status (once); resolves with the answer.
#[tauri::command]
pub async fn request_focus_authorization(app: AppHandle) -> CmdResult<FocusAuthorization> {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    spank_sysmon::request_focus_authorization(move |_| {
        if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
            let _ = tx.send(());
        }
    });
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(120)))
        .await
        .map_err(err)?
        .ok();
    let a = focus_auth_now();
    app.state::<AppState>()
        .engine
        .send(EngineCmd::FocusAuthorization(a));
    Ok(a)
}

/// Open System Settings › Privacy & Security › Focus.
#[tauri::command]
pub fn open_focus_settings() -> CmdResult<()> {
    open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Focus")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_envelope() {
        let p = peaks(&[0.0, 0.5, 0.0, 1.0, 0.25, 0.0], 1, 3);
        assert_eq!(p, vec![1.0 * 0.5, 1.0, 0.25]);
        assert_eq!(peaks(&[], 2, 4), vec![0.0; 4]);
        assert_eq!(peaks(&[0.2, 0.8, 0.1, 0.4], 2, 2), vec![1.0, 0.5]);
    }

    #[test]
    fn slugs_and_ids() {
        assert_eq!(slug("user-123-0"), "user-123-0");
        assert_eq!(slug("毀滅"), "pack");
        assert_eq!(slug("My Pack!"), "my-pack");
        let a = new_sound_id();
        let b = new_sound_id();
        assert_ne!(a, b);
        assert!(spank_store::validate_id(&a.0).is_ok());
    }
}
