fn main() {
    // Windows only: GUI binaries need an embedded manifest (DPI
    // awareness, Win10+ OS support). Without it, GetSystemMetrics
    // reports virtualized sizes and layered Progman children fail.
    #[cfg(windows)]
    {
        embed_manifest::embed_manifest_file("app.manifest")
            .expect("embed Windows application manifest");
    }
    println!("cargo:rerun-if-changed=app.manifest");
}
