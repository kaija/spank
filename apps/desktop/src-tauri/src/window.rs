use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const SETTINGS_LABEL: &str = "settings";
/// First-run onboarding and the calibration wizard (design §11) share one window.
pub const ONBOARDING_LABEL: &str = "onboarding";

/// What the onboarding window shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wizard {
    /// The whole first-run flow (mockup/Onboarding.dc.html).
    Onboarding,
    /// Only the slap calibration steps (Settings › Sensors › Recalibrate).
    Calibration,
}

impl Wizard {
    /// Trailing-slash routes work in both modes: the Next dev server serves `calibrate/` but
    /// 404s on `calibrate/index.html`, and bundled assets resolve `calibrate/` to its index.html.
    fn route(self) -> &'static str {
        match self {
            Wizard::Onboarding => "onboarding/",
            Wizard::Calibration => "calibrate/",
        }
    }
}

fn focus_existing(app: &AppHandle, label: &str) -> bool {
    match app.get_webview_window(label) {
        Some(w) => {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            true
        }
        None => false,
    }
}

/// Show the settings window, creating it if needed, and show the Dock icon while it is open.
pub fn open_settings(app: &AppHandle) {
    if focus_existing(app, SETTINGS_LABEL) {
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    let builder = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
        .title(crate::edition::PRODUCT_NAME)
        .inner_size(760.0, 520.0)
        .min_inner_size(640.0, 440.0)
        .center();
    // Native traffic lights float over the sidebar (mockup/Settings Window.dc.html); the sidebar
    // top and page header are drag regions (`data-tauri-drag-region`).
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    match builder.build() {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("spank: failed to open settings window: {e}"),
    }
}

/// Show the onboarding / calibration window (560 × 440, mockup/Onboarding.dc.html). If it is
/// already open it is focused and keeps its current page.
pub fn open_wizard(app: &AppHandle, wizard: Wizard) {
    if focus_existing(app, ONBOARDING_LABEL) {
        return;
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);

    let builder = WebviewWindowBuilder::new(app, ONBOARDING_LABEL, WebviewUrl::App(wizard.route().into()))
        .title(crate::edition::PRODUCT_NAME)
        .inner_size(560.0, 440.0)
        .resizable(false)
        .maximizable(false)
        .center();
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    match builder.build() {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("spank: failed to open onboarding window: {e}"),
    }
}

/// A window is gone: back to menu-bar-only (no Dock icon) once none are left.
pub fn on_window_destroyed(app: &AppHandle, label: &str) {
    let others = app.webview_windows().into_keys().any(|l| l != label);
    #[cfg(target_os = "macos")]
    if !others {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, others);
}
