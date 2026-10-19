//! In-process video playback via libmpv (`wid` embed).
//!
//! Spike phase: mirrors what the mpv sidecar does today (render into our
//! canvas HWND, pause/mute/volume, end-of-file signal) but without child
//! processes or IPC pipes. The canvas/WorkerW code is untouched — libmpv
//! takes the same window handle the sidecar got via `--wid`.
//!
//! Link time needs the mpv import library (see build.rs); runtime needs
//! `libmpv-2.dll` next to the exe.

use libmpv2::Mpv;
use std::path::Path;

/// Our 0.0-1.0 volume mapped to mpv 0-100 (same as the sidecar path).
pub fn volume_to_mpv(volume: f32) -> i64 {
    ((volume.clamp(0.0, 1.0) * 100.0).round() as i64).clamp(0, 100)
}

/// Subset of mpv events the app cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibEvent {
    /// First frame is on screen — unblock a frozen start here.
    /// (Replaces the brightness probe + sync thread.)
    PlaybackRestart,
    /// File metadata is ready.
    FileLoaded,
    /// Video ended on its own — advance the rotation here.
    /// (Replaces polling for the exiting mpv process.)
    EndOfFile,
    /// File failed (decode error, missing file) — skip it.
    FileError,
    /// mpv is shutting down.
    Shutdown,
}

/// One in-process player bound to a canvas window.
pub struct Player {
    mpv: Mpv,
}

impl Player {
    /// Create a player rendering into `wid` (canvas HWND).
    /// Starts frozen: call [`Player::unfreeze`] on [`LibEvent::PlaybackRestart`]
    /// so every monitor starts on the same frame.
    pub fn open(wid: isize) -> Result<Self, String> {
        let mpv = Mpv::with_initializer(|init| {
            init.set_property("wid", wid as i64)?;
            init.set_property("vo", "gpu")?;
            init.set_property("hwdec", "auto-safe")?;
            // Keep the last frame on screen after EOF: the rotation
            // switch then swaps from a frozen picture, never from a
            // black/transparent canvas (original wallpaper flash).
            init.set_property("keep-open", "yes")?;
            init.set_property("pause", true)?;
            init.set_property("osc", false)?;
            init.set_property("input-default-bindings", false)?;
            Ok(())
        })
        .map_err(|e| format!("mpv init: {e:?}"))?;
        Ok(Self { mpv })
    }

    /// Replace the current file (mirror of `--loop-file=no` rotation mode:
    /// the file plays once, [`LibEvent::EndOfFile`] fires at the end).
    pub fn load_file(&self, file: &Path, loop_file: bool) -> Result<(), String> {
        // Loop mode first: it must be in place before the file starts,
        // otherwise a short clip can reach EOF unlooped (single video
        // then freezes on its last frame instead of looping).
        self.mpv
            .set_property("loop-file", if loop_file { "inf" } else { "no" })
            .map_err(|e| format!("loop-file: {e:?}"))?;
        let path = file.to_string_lossy().into_owned();
        self.mpv
            .command("loadfile", &[path.as_str(), "replace"])
            .map_err(|e| format!("loadfile: {e:?}"))?;
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        self.mpv
            .set_property("pause", paused)
            .map_err(|e| format!("pause: {e:?}"))
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), String> {
        self.mpv
            .set_property("mute", muted)
            .map_err(|e| format!("mute: {e:?}"))
    }

    pub fn set_volume(&self, volume: f32) -> Result<(), String> {
        self.mpv
            .set_property("volume", volume_to_mpv(volume))
            .map_err(|e| format!("volume: {e:?}"))
    }

    /// Read a boolean / string property (diagnostics, `None` when unavailable).
    pub fn prop_bool(&self, name: &str) -> Option<bool> {
        self.mpv.get_property(name).ok()
    }

    pub fn prop_str(&self, name: &str) -> Option<String> {
        self.mpv.get_property(name).ok()
    }

    pub fn prop_f64(&self, name: &str) -> Option<f64> {
        self.mpv.get_property(name).ok()
    }

    /// Stop playback and kill the core. Mandatory before drop:
    /// `Mpv`'s Drop only detaches (`mpv_destroy`) and a detached core
    /// keeps rendering + playing audio as a ghost (two renderers then
    /// fight over one HWND: old picture stays, both audios play).
    pub fn terminate(&self) {
        use libmpv2::events::Event;
        use std::time::{Duration, Instant};
        // Best effort: even if quit errors (already dead), Drop below
        // still runs.
        let _ = self.mpv.command("quit", &[]);
        // Bounded wait for the core to die; then Drop is safe.
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(500) {
            if matches!(self.mpv.wait_event(0.05), Some(Ok(Event::Shutdown))) {
                return;
            }
        }
    }

    /// Wait up to `timeout_ms` for the next interesting event.
    /// Returns `None` on timeout or on mpv chatter we don't care about.
    pub fn wait_event(&self, timeout_ms: u32) -> Option<LibEvent> {
        use libmpv2::events::Event;
        match self.mpv.wait_event(timeout_ms as f64 / 1000.0) {
            Some(Ok(Event::PlaybackRestart)) => Some(LibEvent::PlaybackRestart),
            Some(Ok(Event::FileLoaded)) => Some(LibEvent::FileLoaded),
            Some(Ok(Event::EndFile(reason))) => Some(match reason {
                libmpv2::mpv_end_file_reason::Eof => LibEvent::EndOfFile,
                _ => LibEvent::FileError,
            }),
            Some(Ok(Event::Shutdown)) => Some(LibEvent::Shutdown),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_mapping_matches_sidecar() {
        assert_eq!(volume_to_mpv(0.6), 60);
        assert_eq!(volume_to_mpv(5.0), 100);
        assert_eq!(volume_to_mpv(-1.0), 0);
    }

    #[test]
    fn terminate_kills_idle_core() {
        // Headless-safe: no file is ever loaded, so no VO/display
        // is needed — this only proves quit + Shutdown round-trips.
        let player = Player::open(0).expect("mpv open");
        player.terminate();
    }
}
