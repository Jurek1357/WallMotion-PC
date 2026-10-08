fn main() {
    // Windows only: GUI binaries need an embedded manifest (DPI
    // awareness, Win10+ OS support).
    #[cfg(windows)]
    {
        embed_manifest::embed_manifest_file("app.manifest")
            .expect("embed Windows application manifest");
    }
    // Windows only: exe file icon (Explorer, taskbar, Alt-Tab).
    // The window title-bar + tray icons are set at runtime from
    // `assets/icon.png` (see `app_icon_rgba` in main.rs).
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        embed_resource::compile("app.rc", embed_resource::NONE);
    }
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=icon.ico");
}
