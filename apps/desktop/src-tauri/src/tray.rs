use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Result};

const ID_SETTINGS: &str = "settings";
const ID_QUIT: &str = "quit";

/// Menu bar icon with the scaffold menu (design §10.1; full menu comes in T31).
pub fn create(app: &AppHandle) -> Result<()> {
    let status = MenuItemBuilder::with_id("status", "Spank — 運作中")
        .enabled(false)
        .build(app)?;
    let settings = MenuItemBuilder::with_id(ID_SETTINGS, "設定…")
        .accelerator("Cmd+,")
        .build(app)?;
    let quit = MenuItemBuilder::with_id(ID_QUIT, "結束")
        .accelerator("Cmd+Q")
        .build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&settings)
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(include_bytes!("../icons/tray-template.png"))?)
        .icon_as_template(true)
        .tooltip("Spank")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            ID_SETTINGS => crate::window::open_settings(app),
            ID_QUIT => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
