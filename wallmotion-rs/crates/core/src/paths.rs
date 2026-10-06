//! Application directories: config, downloads, log.
//!
//! Mirrors `wallmotion/paths.py`: legacy locations on Windows,
//! XDG Base Directory locations on Linux/macOS (with env overrides).

use std::path::PathBuf;

/// Resolved locations for config file, downloads dir and log file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub config: PathBuf,
    pub downloads: PathBuf,
    pub log: PathBuf,
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Repository-root-anchored base dir for tests (the `wallmotion-rs/`
/// folder); production callers pass their own exe-adjacent dir.
pub fn app_dirs_with_home(home: &std::path::Path, windows: bool) -> AppDirs {
    if windows {
        let base = exe_dir();
        return AppDirs {
            config: home.join(".live_wallpaper_config.json"),
            downloads: base.join("downloads"),
            log: std::env::temp_dir().join("live_wallpaper_debug.log"),
        };
    }
    let cfg_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("share"));
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local").join("state"));
    AppDirs {
        config: cfg_home.join("wallmotion").join("config.json"),
        downloads: data_home.join("wallmotion").join("downloads"),
        log: state_home.join("wallmotion").join("debug.log"),
    }
}

/// Directories for this machine (Windows keeps legacy locations).
pub fn app_dirs() -> AppDirs {
    app_dirs_with_home(&home_dir(), cfg!(windows))
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const XDG_VARS: [&str; 3] = ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"];

    /// Run with XDG overrides removed (defaults under test).
    fn without_xdg(f: impl FnOnce()) {
        let saved: Vec<(String, Option<std::ffi::OsString>)> = XDG_VARS
            .iter()
            .map(|k| (k.to_string(), std::env::var_os(k)))
            .collect();
        for k in XDG_VARS {
            std::env::remove_var(k);
        }
        f();
        for (k, v) in saved {
            match v {
                Some(val) => std::env::set_var(&k, val),
                None => std::env::remove_var(&k),
            }
        }
    }

    #[test]
    fn windows_keeps_legacy_locations() {
        let dirs = app_dirs_with_home(Path::new("/home/test"), true);
        assert_eq!(
            dirs.config,
            Path::new("/home/test/.live_wallpaper_config.json")
        );
        assert!(dirs.downloads.ends_with("downloads"));
        assert!(dirs.log.ends_with("live_wallpaper_debug.log"));
    }

    #[test]
    fn linux_uses_xdg() {
        without_xdg(|| {
            let dirs = app_dirs_with_home(Path::new("/home/test"), false);
            assert_eq!(
                dirs.config,
                Path::new("/home/test/.config/wallmotion/config.json")
            );
            assert_eq!(
                dirs.downloads,
                Path::new("/home/test/.local/share/wallmotion/downloads")
            );
            assert_eq!(
                dirs.log,
                Path::new("/home/test/.local/state/wallmotion/debug.log")
            );
        });
    }
}
