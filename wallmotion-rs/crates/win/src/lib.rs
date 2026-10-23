//! Desktop render surface: WorkerW discovery and video canvas on
//! Windows, an override-redirect X11 desktop window on Linux.
//!
//! Mirrors `wallmotion/win32.py` + the canvas part of
//! `wallmotion/video.py`. Pure selection math is unit-tested
//! everywhere; native calls run on their platform only.

pub mod autopause;
pub mod canvas;
pub mod wallpaper;
pub mod workerw;

#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

/// Render backend label for status lines.
pub fn backend_name() -> &'static str {
    #[cfg(windows)]
    {
        "windows WorkerW canvas + mpv"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "x11 desktop canvas + mpv"
    }
    #[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
    {
        "stub (Windows/X11 only in this build)"
    }
}
