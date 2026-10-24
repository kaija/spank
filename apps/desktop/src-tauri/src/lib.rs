//! Spank menu bar app shell (ADR-0004).
//!
//! - Lives in the menu bar only: `ActivationPolicy::Accessory` (no Dock icon).
//! - The settings window is created on demand and destroyed on close, so no WebView
//!   stays alive while idle. The Dock icon shows only while that window is open.
//! - Closing the last window never quits the app; only the tray's Quit does.

mod tray;
mod window;

use serde::Serialize;
use spank_core::AppConfig;
use tauri::RunEvent;

/// Pass this argument (or launch a second instance) to open the settings window at startup.
pub const OPEN_SETTINGS_ARG: &str = "--open-settings";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    default_config: AppConfig,
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        default_config: AppConfig::default(),
    }
}

pub fn run() {
    let app = tauri::Builder::default()
        // A second launch (e.g. double-clicking the app again) opens settings in the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            window::open_settings(app);
        }))
        // Registered now; enabling "launch at login" is driven by config (T14/T38), never implicitly in dev.
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .invoke_handler(tauri::generate_handler![app_info])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            tray::create(app.handle())?;
            if std::env::args().any(|a| a == OPEN_SETTINGS_ARG) {
                window::open_settings(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        // Closing the last window must not quit: we live in the menu bar.
        RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
        RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } if label == window::SETTINGS_LABEL => {
            window::on_settings_destroyed(app);
        }
        _ => {}
    });
}
