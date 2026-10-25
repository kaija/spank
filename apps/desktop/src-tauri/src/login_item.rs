//! Launch at login for the Mac App Store edition (ADR-0017): the app registers itself as a login
//! item with `SMAppService.mainApp`, which works inside the App Sandbox. (The DMG edition keeps the
//! autostart plugin's Launch Agent, which writes outside the sandbox container.)

use smappservice_rs::{AppService, ServiceStatus, ServiceType};

/// Is the app registered to launch at login? `RequiresApproval` counts: it is registered and only
/// waits for the user in System Settings › Login Items.
pub fn is_enabled() -> bool {
    matches!(
        AppService::new(ServiceType::MainApp).status(),
        ServiceStatus::Enabled | ServiceStatus::RequiresApproval
    )
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if is_enabled() == enabled {
        return Ok(());
    }
    let service = AppService::new(ServiceType::MainApp);
    if enabled {
        service.register()
    } else {
        service.unregister()
    }
    .map_err(|e| e.to_string())
}
