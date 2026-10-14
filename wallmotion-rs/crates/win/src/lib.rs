//! Windows render surface: WorkerW discovery and video canvas.
//!
//! Mirrors `wallmotion/win32.py` + the canvas part of
//! `wallmotion/video.py`. Pure selection math is unit-tested
//! everywhere; Win32 calls run on Windows only.

pub mod autopause;
pub mod canvas;
pub mod wallpaper;
pub mod workerw;

/// Render backend label for status lines.
pub fn backend_name() -> &'static str {
    #[cfg(windows)]
    {
        "windows WorkerW canvas + mpv"
    }
    #[cfg(not(windows))]
    {
        "stub (Windows only in this build)"
    }
}
