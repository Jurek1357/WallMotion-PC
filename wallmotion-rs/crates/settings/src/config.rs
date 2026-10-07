//! App config: last file, mute and volume, persisted as JSON.
//!
//! Mirrors the matching keys of `%USERPROFILE%/.live_wallpaper_config.json`
//! (Windows) and `~/.config/wallmotion/config.json` (Linux) so a future
//! migration can read the Python app's file.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where the config file lives on this machine.
pub fn config_path() -> PathBuf {
    wallmotion_core::paths::app_dirs().config
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub last_path: String,
    #[serde(default = "default_muted")]
    pub muted: bool,
    #[serde(default = "default_volume")]
    pub volume: u8,
}

fn default_muted() -> bool {
    true
}

fn default_volume() -> u8 {
    30
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            last_path: String::new(),
            muted: default_muted(),
            volume: default_volume(),
        }
    }
}

impl AppConfig {
    /// Load from a file. Missing/corrupt file -> defaults.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Save to a file (creates parent dirs). Returns success.
    pub fn save_to(&self, path: &Path) -> bool {
        (|| -> Option<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok()?;
            }
            let text = serde_json::to_string_pretty(self).ok()?;
            std::fs::write(path, text).ok()?;
            Some(())
        })()
        .is_some()
    }

    /// Load from the standard location.
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    /// Save to the standard location.
    pub fn save(&self) -> bool {
        self.save_to(&config_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join("wallmotion-config-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        let cfg = AppConfig {
            last_path: "C:\\vids\\a.mp4".to_string(),
            muted: false,
            volume: 70,
        };
        assert!(cfg.save_to(&path));
        assert_eq!(AppConfig::load_from(&path), cfg);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_is_default() {
        let cfg = AppConfig::load_from(Path::new("/definitely/not/here.json"));
        assert_eq!(cfg, AppConfig::default());
        assert!(cfg.muted);
        assert_eq!(cfg.volume, 30);
    }

    #[test]
    fn corrupt_is_default() {
        let dir = std::env::temp_dir().join("wallmotion-config-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("broken.json");
        std::fs::write(&path, b"{oops").unwrap();
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
        let _ = std::fs::remove_file(&path);
    }
}
