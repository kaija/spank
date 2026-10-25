//! Build flavour (ADR-0017).
//!
//! - `direct` (default): the notarized DMG "Spank" with every feature and the self-updater.
//! - `appstore` (`--no-default-features --features appstore`): the Mac App Store edition "Creaky".
//!   It runs in the App Sandbox, where the accelerometer cannot be switched on (property writes on
//!   `AppleSPUHIDDriver` are refused), so it plays lid sounds only: no slaps, calibration, precise
//!   typing or updater, and launch at login goes through `SMAppService` ([`crate::login_item`]).

use spank_core::AppConfig;

#[cfg(all(feature = "appstore", feature = "updater"))]
compile_error!(
    "the Mac App Store edition must not include the updater: build with --no-default-features --features appstore"
);

/// This build is the Mac App Store edition.
pub const APP_STORE: bool = cfg!(feature = "appstore");

/// `"appstore"` or `"direct"`, as the UI sees it (`AppInfo.edition`).
pub const NAME: &str = if APP_STORE { "appstore" } else { "direct" };

/// Name in menus and window titles. Matches `productName` in the Tauri config for this flavour.
pub const PRODUCT_NAME: &str = if APP_STORE { "Creaky" } else { "Spank" };

/// Error for commands that exist only in the DMG edition.
pub fn unavailable() -> String {
    "not available in the Mac App Store edition".to_owned()
}

/// Turn off what this edition can't do, so a config from anywhere (disk, UI) stays consistent.
pub fn restrict(cfg: &mut AppConfig) {
    if APP_STORE {
        cfg.slap.enabled = false;
        cfg.slap.precise_typing = false;
        cfg.auto_check_updates = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restrict_matches_the_edition() {
        let mut c = AppConfig::default();
        c.slap.enabled = true;
        c.slap.precise_typing = true;
        restrict(&mut c);
        assert_eq!(c.slap.enabled, !APP_STORE);
        assert_eq!(c.slap.precise_typing, !APP_STORE);
        assert_eq!(PRODUCT_NAME, if APP_STORE { "Creaky" } else { "Spank" });
    }
}
