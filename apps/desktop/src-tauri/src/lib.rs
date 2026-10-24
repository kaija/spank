//! Spank menu bar app shell (ADR-0004).
//!
//! - Lives in the menu bar only: `ActivationPolicy::Accessory` (no Dock icon).
//! - The settings window is created on demand and destroyed on close, so no WebView
//!   stays alive while idle. The Dock icon shows only while that window is open.
//! - Closing the last window never quits the app; only the tray's Quit does.
//! - [`engine`] runs the sensors, the orchestrator and playback on a core thread (T30);
//!   [`commands`] are the settings UI's backend.
//! - First launch opens the onboarding wizard (T36); closing it counts as done.
//! - Launched outside a terminal, stderr goes to `~/Library/Logs/<bundle id>/spank.log` ([`diag`]).

mod commands;
mod diag;
mod engine;
mod state;
mod tray;
mod window;

use engine::{Engine, EngineCmd, EngineEvent};
use spank_store::{Profiles, Store};
use state::AppState;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, RunEvent};

/// Pass this argument (or launch a second instance) to open the settings window at startup.
pub const OPEN_SETTINGS_ARG: &str = "--open-settings";
/// Store directory (design §9), named after the bundle id.
pub const BUNDLE_ID: &str = "dev.spank.app";

/// Turn accelerometer reporting off if we panic (ADR-0014: the Drop path may not run).
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        spank_sensors::reset_accel_properties();
        prev(info);
    }));
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    if let Some(dir) = diag::log_dir(BUNDLE_ID)
        && let Err(e) = diag::redirect_stderr(&dir)
    {
        eprintln!("spank: could not open the log file: {e}");
    }

    let root = Store::default_root(BUNDLE_ID).ok_or("no Application Support directory")?;
    let store = Store::open(root)?;
    let loaded = store.load_config()?;
    if let Some(r) = &loaded.recovered {
        eprintln!(
            "spank: config.json was unreadable ({}); saved as {}",
            r.reason,
            r.backup.display()
        );
    }
    let config = loaded.value;

    let builtin_dir = app.path().resource_dir()?.join("builtin");
    let (builtin, errors) = spank_pack::load_builtin(&builtin_dir);
    for e in errors {
        eprintln!("spank: built-in pack skipped: {e}");
    }
    let builtin = Arc::new(builtin);
    let profiles = Profiles::new(builtin.profiles.clone());
    let active = profiles.active(&store, &config);

    let handle = app.handle().clone();
    let engine = Engine::start(
        config.clone(),
        active,
        AppState::resolver(store.sounds_dir(), builtin.clone()),
        move |ev| match ev {
            EngineEvent::State(s) => {
                let _ = handle.emit("engine_state", &s);
                tray::refresh(&handle);
            }
            EngineEvent::Fired(f) => {
                let _ = handle.emit("trigger_fired", &f);
            }
            EngineEvent::Calibration(p) => {
                let _ = handle.emit("calibration", &p);
            }
            EngineEvent::AccelLevel(g) => {
                let _ = handle.emit("accel_level", g);
            }
        },
    );
    engine.send(EngineCmd::FocusAuthorization(commands::focus_auth_now()));

    // System signals: notifications arrive on the main queue (we're on the main thread here).
    let tx = engine.sender();
    let sysmon = spank_sysmon::SysMonitor::start(move |s| {
        let _ = tx.send(EngineCmd::Sys(s));
    });

    // Login item follows the config, but never from dev builds (CLAUDE.md: no implicit login items).
    if !cfg!(debug_assertions)
        && let Err(e) = commands::apply_launch_at_login(app.handle(), config.launch_at_login)
    {
        eprintln!("spank: could not update the login item: {e}");
    }
    let first_run = !config.onboarding_done;

    app.manage(AppState {
        app: app.handle().clone(),
        store,
        builtin,
        profiles,
        config: Mutex::new(config),
        engine,
        sysmon: Mutex::new(Some(sysmon)),
    });

    tray::create(app.handle())?;
    if first_run {
        window::open_wizard(app.handle(), window::Wizard::Onboarding);
    }
    if std::env::args().any(|a| a == OPEN_SETTINGS_ARG) {
        window::open_settings(app.handle());
    }
    Ok(())
}

pub fn run() {
    install_panic_hook();
    let app = tauri::Builder::default()
        // A second launch (e.g. double-clicking the app again) opens settings in the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::open_settings(app);
        }))
        // Registered now; enabling "launch at login" is driven by config (T38), never implicitly in dev.
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::get_state,
            commands::get_config,
            commands::set_paused,
            commands::set_mute_rules,
            commands::set_slap_enabled,
            commands::set_lid_config,
            commands::set_slap_config,
            commands::set_app_volume,
            commands::set_language,
            commands::set_auto_check_updates,
            commands::set_launch_at_login,
            commands::get_calibration,
            commands::open_calibration,
            commands::start_calibration,
            commands::calibration_step,
            commands::cancel_calibration,
            commands::finish_calibration,
            commands::set_accel_monitor,
            commands::complete_onboarding,
            commands::request_input_monitoring,
            commands::open_input_monitoring_settings,
            commands::diagnostics,
            commands::copy_diagnostics,
            commands::open_log_folder,
            commands::list_profiles,
            commands::create_profile,
            commands::duplicate_profile,
            commands::save_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::set_active_profile,
            commands::list_sounds,
            commands::import_sound,
            commands::rename_sound,
            commands::delete_sound,
            commands::sound_peaks,
            commands::preview_sound,
            commands::stop_preview,
            commands::preview_event,
            commands::reveal_sound,
            commands::import_pack,
            commands::export_pack,
            commands::request_focus_authorization,
            commands::open_focus_settings,
        ])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        // Closing the last window must not quit: we live in the menu bar.
        RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            if let Some(state) = app.try_state::<AppState>() {
                state.engine.shutdown();
                if let Ok(mut m) = state.sysmon.lock() {
                    m.take();
                }
            }
        }
        RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } => {
            if let Some(state) = app.try_state::<AppState>() {
                match label.as_str() {
                    // The page may not get to unmount: stop the live waveform here.
                    window::SETTINGS_LABEL => state.engine.send(EngineCmd::Monitor(false)),
                    // Closing onboarding half-way counts as done; Sensors then shows "not calibrated".
                    window::ONBOARDING_LABEL => {
                        state.engine.send(EngineCmd::CancelCalibration);
                        if !state.config().onboarding_done {
                            let _ = state.update_config(|c| c.onboarding_done = true);
                        }
                    }
                    _ => {}
                }
            }
            window::on_window_destroyed(app, &label);
        }
        _ => {}
    });
}
