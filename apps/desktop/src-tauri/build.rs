fn main() {
    // `generate_context!` embeds the icons (the dev Dock icon comes from icons/icon.icns), but
    // tauri-build doesn't tell cargo to watch them, so an icon-only change would keep the old one.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
