//! Link the mpv import library.
//!
//! Local dev files live in `wallmotion-rs/deps/mpv-dev` (gitignored,
//! downloaded from the shinchiro mpv-dev package): the linker only needs
//! the import library there (`libmpv.dll.a` for GNU, `mpv.lib` for MSVC
//! — generated from the DLL, see CI). At runtime `libmpv-2.dll` must sit
//! next to the exe (or on PATH for dev runs).

fn main() {
    #[cfg(windows)]
    {
        let dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deps/mpv-dev");
        if dev.join("libmpv.dll.a").is_file() || dev.join("mpv.lib").is_file() {
            println!("cargo:rustc-link-search=native={}", dev.display());
        }
    }
}
