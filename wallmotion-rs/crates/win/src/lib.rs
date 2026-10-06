//! Windows render surface: WorkerW discovery and video canvas.
//!
//! Mirrors `wallmotion/win32.py` + the canvas part of
//! `wallmotion/video.py`. Pure selection math is unit-tested
//! everywhere; Win32 calls run on Windows only.

pub mod canvas;
pub mod workerw;
