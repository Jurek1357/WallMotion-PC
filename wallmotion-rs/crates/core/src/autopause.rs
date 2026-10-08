//! Auto-pause rules: pause video wallpaper during fullscreen apps / on battery.
//!
//! Port of `wallmotion/autopause.py` (same idea as Lively): a fullscreen
//! game gets the GPU back, a laptop on battery saves power. Decision logic
//! is pure and unit-tested; OS sensors live in `wallmotion-win` (Windows)
//! and in [`is_on_battery_sysfs`] (Linux sysfs, no dependencies).

use std::path::Path;

/// Poll interval and clean polls required before resuming (no alt-tab flapping).
pub const POLL_INTERVAL_MS: u64 = 2000;
pub const RESUME_AFTER_CLEAN_POLLS: u32 = 2;

/// Pure check: does the window cover its whole monitor?
pub fn is_fullscreen_rect(
    window_rect: (i32, i32, i32, i32),
    monitor_rect: (i32, i32, i32, i32),
) -> bool {
    window_rect == monitor_rect
}

/// Pure decision: pause when any enabled rule fires.
pub fn should_pause(
    fullscreen_active: bool,
    on_battery: bool,
    pause_on_fullscreen: bool,
    pause_on_battery: bool,
) -> bool {
    (pause_on_fullscreen && fullscreen_active) || (pause_on_battery && on_battery)
}

/// True when the `ACLineStatus` byte means "running on battery".
/// Mirrors `parse_ac_line_status()`: only 0 is battery (1=AC, 255=unknown).
pub fn parse_ac_line_status(ac_line_status: u8) -> bool {
    ac_line_status == 0
}

/// Hysteresis state machine: pause at once, resume only after
/// [`RESUME_AFTER_CLEAN_POLLS`] clean polls. Mirrors `VideoWallpaperWindow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoPause {
    pub pause_on_fullscreen: bool,
    pub pause_on_battery: bool,
    pub autopaused: bool,
    clean_polls: u32,
}

impl AutoPause {
    pub fn new(pause_on_fullscreen: bool, pause_on_battery: bool) -> Self {
        Self {
            pause_on_fullscreen,
            pause_on_battery,
            autopaused: false,
            clean_polls: 0,
        }
    }

    pub fn set_rules(&mut self, fullscreen: bool, battery: bool) {
        self.pause_on_fullscreen = fullscreen;
        self.pause_on_battery = battery;
        if !fullscreen && !battery {
            self.autopaused = false;
            self.clean_polls = 0;
        }
    }

    pub fn reset(&mut self) {
        self.autopaused = false;
        self.clean_polls = 0;
    }

    /// One poll tick. `user_paused` wins (manual Pause is never overridden).
    /// Returns `Some(true)` = should pause now, `Some(false)` = should resume
    /// now, `None` = no change.
    pub fn tick(
        &mut self,
        fullscreen_active: bool,
        on_battery: bool,
        user_paused: bool,
    ) -> Option<bool> {
        if !(self.pause_on_fullscreen || self.pause_on_battery) {
            return None;
        }
        if user_paused {
            return None;
        }
        let want_pause = should_pause(
            fullscreen_active,
            on_battery,
            self.pause_on_fullscreen,
            self.pause_on_battery,
        );
        if want_pause {
            if self.autopaused {
                return None;
            }
            self.autopaused = true;
            self.clean_polls = 0;
            return Some(true);
        }
        if self.autopaused {
            self.clean_polls += 1;
            if self.clean_polls >= RESUME_AFTER_CLEAN_POLLS {
                self.autopaused = false;
                self.clean_polls = 0;
                return Some(false);
            }
        }
        None
    }
}

/// Linux battery sensor over sysfs: true when any `type==battery` supply
/// reports `status==discharging`. Never panics; false when unreadable.
pub fn is_on_battery_sysfs(sysfs_root: &Path) -> bool {
    let entries = match std::fs::read_dir(sysfs_root) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let base = entry.path();
        let is_battery = std::fs::read_to_string(base.join("type"))
            .map(|s| s.trim().eq_ignore_ascii_case("battery"))
            .unwrap_or(false);
        if !is_battery {
            continue;
        }
        if std::fs::read_to_string(base.join("status"))
            .map(|s| s.trim().eq_ignore_ascii_case("discharging"))
            .unwrap_or(false)
        {
            return true;
        }
    }
    false
}

/// Production Linux sensor (`/sys/class/power_supply`). Always false on
/// desktops / unknown (no battery entries).
pub fn is_on_battery_linux() -> bool {
    is_on_battery_sysfs(Path::new("/sys/class/power_supply"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_rect_exact_match() {
        assert!(is_fullscreen_rect((0, 0, 1920, 1080), (0, 0, 1920, 1080)));
        assert!(!is_fullscreen_rect((0, 0, 1919, 1080), (0, 0, 1920, 1080)));
        assert!(!is_fullscreen_rect(
            (0, 0, 1920, 1080),
            (1920, 0, 3840, 1080)
        ));
    }

    #[test]
    fn decision_matrix() {
        assert!(should_pause(true, false, true, false));
        assert!(!should_pause(true, false, false, false));
        assert!(!should_pause(true, false, false, true));
        assert!(should_pause(false, true, false, true));
        assert!(!should_pause(false, false, true, true));
    }

    #[test]
    fn ac_status_only_zero_is_battery() {
        assert!(parse_ac_line_status(0));
        assert!(!parse_ac_line_status(1));
        assert!(!parse_ac_line_status(255));
    }

    #[test]
    fn tick_pauses_at_once_resumes_after_two_clean() {
        let mut ap = AutoPause::new(true, false);
        assert_eq!(ap.tick(true, false, false), Some(true));
        // Still fullscreen: no repeat command.
        assert_eq!(ap.tick(true, false, false), None);
        // First clean poll: not yet.
        assert_eq!(ap.tick(false, false, false), None);
        assert!(ap.autopaused);
        // Second clean poll: resume.
        assert_eq!(ap.tick(false, false, false), Some(false));
        assert!(!ap.autopaused);
    }

    #[test]
    fn tick_respects_user_pause() {
        let mut ap = AutoPause::new(true, false);
        assert_eq!(ap.tick(true, false, true), None);
        assert!(!ap.autopaused);
    }

    #[test]
    fn tick_disabled_rules_never_fire() {
        let mut ap = AutoPause::new(false, false);
        assert_eq!(ap.tick(true, true, false), None);
    }

    #[test]
    fn sysfs_missing_is_false() {
        assert!(!is_on_battery_sysfs(Path::new("/definitely/not/here")));
    }

    #[test]
    fn sysfs_discharging_is_true() {
        let dir = std::env::temp_dir().join("wallmotion-batt-test");
        let bat = dir.join("BAT0");
        let _ = std::fs::create_dir_all(&bat);
        std::fs::write(bat.join("type"), b"Battery\n").unwrap();
        std::fs::write(bat.join("status"), b"Discharging\n").unwrap();
        assert!(is_on_battery_sysfs(&dir));
        std::fs::write(bat.join("status"), b"Charging\n").unwrap();
        assert!(!is_on_battery_sysfs(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
