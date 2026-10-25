//! Menu bar icon and menu (T31, design §10.1, mockup/Menu Bar.dc.html):
//!
//! ```text
//! Spank — 運作中 / 已靜音：開會中 / 已暫停
//! Profile：毀滅
//! ─────────────
//! 暫停 / 繼續
//! Profile ▸  ✓ 毀滅 / 哀哀叫 / 經典 / …自訂
//! 試聽 ▸     打開螢幕 / 闔上螢幕 / 開啟中 / 輕拍 / 中拍 / 重拍 / 連擊
//! ─────────────
//! 設定…   ⌘,
//! 結束    ⌘Q
//! ```
//!
//! The menu is rebuilt whenever the paused / mute / active-profile state or the profile list
//! changes.

use crate::engine::EngineCmd;
use crate::state::AppState;
use spank_core::{EventKind, Language, MuteReason, RuntimeState};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Result, Wry};

const TRAY_ID: &str = "main";
const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";
const ID_PAUSE: &str = "pause";
const PROFILE_PREFIX: &str = "profile:";
const PREVIEW_PREFIX: &str = "preview:";
const PRODUCT: &str = crate::edition::PRODUCT_NAME;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

/// The menu's own strings (the settings UI uses next-intl).
pub struct Text {
    lang: Lang,
}

impl Text {
    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        match self.lang {
            Lang::Zh => zh,
            Lang::En => en,
        }
    }

    pub fn reason(&self, r: MuteReason) -> &'static str {
        match r {
            MuteReason::Meeting => self.t("開會中", "in a meeting"),
            MuteReason::Focus => self.t("專注模式", "Focus is on"),
            MuteReason::QuietHours => self.t("安靜時段", "quiet hours"),
            MuteReason::SystemMuted => self.t("系統靜音", "system muted"),
        }
    }

    pub fn status(&self, s: &RuntimeState) -> String {
        if s.paused {
            format!("{PRODUCT} — {}", self.t("已暫停", "Paused"))
        } else if let Some(r) = s.mute.reasons.first() {
            format!("{PRODUCT} — {}{}", self.t("已靜音：", "Muted: "), self.reason(*r))
        } else {
            format!("{PRODUCT} — {}", self.t("運作中", "Running"))
        }
    }

    pub fn event(&self, k: EventKind) -> &'static str {
        match k {
            EventKind::LidOpened => self.t("打開螢幕", "Lid opened"),
            EventKind::LidClosing => self.t("闔上螢幕", "Lid closing"),
            EventKind::LidOverbent => self.t("開啟中", "Opening"),
            EventKind::SlapLight => self.t("輕拍", "Light slap"),
            EventKind::SlapMedium => self.t("中拍", "Medium slap"),
            EventKind::SlapHard => self.t("重拍", "Hard slap"),
            EventKind::SlapCombo => self.t("連擊", "Combo"),
        }
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFLocaleCopyPreferredLanguages() -> core_foundation::array::CFArrayRef;
}

/// First preferred macOS language, e.g. "zh-Hant-TW".
pub fn system_language() -> Option<String> {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    // SAFETY: Copy rule — we own the returned array.
    let arr: CFArray<CFString> = unsafe {
        let r = CFLocaleCopyPreferredLanguages();
        if r.is_null() {
            return None;
        }
        CFArray::wrap_under_create_rule(r)
    };
    arr.iter().next().map(|s| s.to_string())
}

/// Same rule as the UI's `resolveLocale`: any Chinese → zh-Hant, anything else → English.
pub fn resolve_lang(setting: Language, system: Option<&str>) -> Lang {
    match setting {
        Language::ZhHant => Lang::Zh,
        Language::En => Lang::En,
        Language::System => match system {
            Some(s) if s.to_ascii_lowercase().starts_with("zh") => Lang::Zh,
            _ => Lang::En,
        },
    }
}

/// What the menu shows; the menu is rebuilt only when this changes.
#[derive(Clone, PartialEq)]
struct MenuKey {
    lang: bool,
    paused: bool,
    mute: Vec<MuteReason>,
    active: String,
    profiles: Vec<(String, String)>,
}

static LAST: Mutex<Option<MenuKey>> = Mutex::new(None);

fn build_menu(app: &AppHandle) -> Result<(Menu<Wry>, MenuKey)> {
    // The engine can report before setup has registered the state.
    let state = app
        .try_state::<AppState>()
        .ok_or(tauri::Error::InvalidWindowHandle)?;
    let cfg = state.config();
    let rt = state.engine.state();
    let text = Text {
        lang: resolve_lang(cfg.language, system_language().as_deref()),
    };
    let profiles = state.profiles.list(&state.store).unwrap_or_default();
    let key = MenuKey {
        lang: text.lang == Lang::Zh,
        paused: rt.paused,
        mute: rt.mute.reasons.clone(),
        active: cfg.active_profile.0.clone(),
        profiles: profiles
            .iter()
            .map(|p| (p.id.0.clone(), p.name.clone()))
            .collect(),
    };

    let status = MenuItemBuilder::with_id("status", text.status(&rt))
        .enabled(false)
        .build(app)?;
    let active_name = profiles
        .iter()
        .find(|p| p.id == cfg.active_profile)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let current = MenuItemBuilder::with_id("current", format!("Profile：{active_name}"))
        .enabled(false)
        .build(app)?;
    let pause = MenuItemBuilder::with_id(
        ID_PAUSE,
        if rt.paused {
            text.t("繼續", "Resume")
        } else {
            text.t("暫停", "Pause")
        },
    )
    .build(app)?;

    let mut profile_menu = SubmenuBuilder::new(app, "Profile");
    let mut last_builtin = None;
    for p in &profiles {
        if last_builtin == Some(true) && !p.builtin {
            profile_menu = profile_menu.separator();
        }
        last_builtin = Some(p.builtin);
        let item = CheckMenuItemBuilder::with_id(format!("{PROFILE_PREFIX}{}", p.id.0), &p.name)
            .checked(p.id == cfg.active_profile)
            .build(app)?;
        profile_menu = profile_menu.item(&item);
    }
    let profile_menu = profile_menu.build()?;

    let mut preview = SubmenuBuilder::new(app, text.t("試聽", "Preview"));
    // The App Store edition has no slaps (ADR-0017).
    for k in EventKind::ALL
        .into_iter()
        .filter(|k| !(crate::edition::APP_STORE && k.is_slap()))
    {
        let id = format!(
            "{PREVIEW_PREFIX}{}",
            serde_json::to_value(k)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        );
        preview = preview.item(&MenuItemBuilder::with_id(id, text.event(k)).build(app)?);
    }
    let preview = preview.build()?;

    let settings = MenuItemBuilder::with_id(ID_SETTINGS, text.t("設定…", "Settings…"))
        .accelerator("Cmd+,")
        .build(app)?;
    let quit = MenuItemBuilder::with_id(ID_QUIT, text.t("結束", "Quit"))
        .accelerator("Cmd+Q")
        .build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .item(&current)
        .separator()
        .item(&pause)
        .item(&profile_menu)
        .item(&preview)
        .separator()
        .item(&settings)
        .separator()
        .item(&quit)
        .build()?;
    Ok((menu, key))
}

fn on_menu(app: &AppHandle, id: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    match id {
        ID_SETTINGS => crate::window::open_settings(app),
        ID_QUIT => {
            state.engine.shutdown();
            app.exit(0);
        }
        ID_PAUSE => {
            let paused = !state.config().paused;
            let _ = crate::commands::set_paused(app.clone(), state, paused);
        }
        _ => {
            if let Some(pid) = id.strip_prefix(PROFILE_PREFIX) {
                let _ = crate::commands::set_active_profile(app.clone(), state, pid.into());
            } else if let Some(kind) = id.strip_prefix(PREVIEW_PREFIX)
                && let Ok(k) = serde_json::from_value::<EventKind>(serde_json::Value::String(kind.into()))
            {
                state.engine.send(EngineCmd::PreviewEvent(k));
            }
        }
    }
    refresh(app);
}

pub fn create(app: &AppHandle) -> Result<()> {
    let (menu, key) = build_menu(app)?;
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!("../icons/tray-template.png"))?)
        .icon_as_template(true)
        .tooltip(PRODUCT)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

/// Rebuild the menu if what it shows changed. Safe to call from any thread.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Ok((menu, key)) = build_menu(&handle) else {
            return;
        };
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(&key) {
            return;
        }
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_menu(Some(menu));
            let tip = Text {
                lang: if key.lang { Lang::Zh } else { Lang::En },
            }
            .status(&handle.state::<AppState>().engine.state());
            // (try_state succeeded in build_menu, so state() is safe here.)
            let _ = tray.set_tooltip(Some(tip));
        }
        *last = Some(key);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use spank_core::MuteStatus;

    #[test]
    fn language_resolution() {
        assert!(resolve_lang(Language::System, Some("zh-Hant-TW")) == Lang::Zh);
        assert!(resolve_lang(Language::System, Some("en-US")) == Lang::En);
        assert!(resolve_lang(Language::System, None) == Lang::En);
        assert!(resolve_lang(Language::ZhHant, Some("en")) == Lang::Zh);
        assert!(resolve_lang(Language::En, Some("zh")) == Lang::En);
    }

    #[test]
    fn status_lines() {
        let zh = Text { lang: Lang::Zh };
        let en = Text { lang: Lang::En };
        let mut s = RuntimeState::default();
        assert_eq!(zh.status(&s), format!("{PRODUCT} — 運作中"));
        s.mute = MuteStatus {
            reasons: vec![MuteReason::Meeting],
            focus_unavailable: false,
        };
        assert_eq!(zh.status(&s), format!("{PRODUCT} — 已靜音：開會中"));
        assert_eq!(en.status(&s), format!("{PRODUCT} — Muted: in a meeting"));
        s.paused = true;
        assert_eq!(en.status(&s), format!("{PRODUCT} — Paused"));
        assert_eq!(zh.event(EventKind::SlapCombo), "連擊");
        assert_eq!(en.reason(MuteReason::QuietHours), "quiet hours");
    }
}
