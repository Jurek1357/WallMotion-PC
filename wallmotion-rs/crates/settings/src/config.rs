//! App config: last file, mute and volume, persisted as JSON.
//!
//! Mirrors the matching keys of `%USERPROFILE%/.live_wallpaper_config.json`
//! (Windows) and `~/.config/wallmotion/config.json` (Linux) so a future
//! migration can read the Python app's file.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Monitor device name (`\\.\DISPLAY1`) or empty for all monitors.
    #[serde(default)]
    pub monitor: String,
    /// Pause video when a fullscreen app is active (games).
    #[serde(default = "default_pause_on_fullscreen")]
    pub pause_on_fullscreen: bool,
    /// Pause video when running on battery.
    #[serde(default)]
    pub pause_on_battery: bool,
    /// Rotation playlist. Same shape as the Python app's `rotation` object
    /// (`files/index/shuffle/repeat/enabled/interval`) — both apps share
    /// the config file.
    #[serde(default)]
    pub rotation: RotationConfig,
    /// Per-wallpaper volume memory (`{path: {volume, muted}}`).
    /// Same shape as Python `volumes` (see `wallmotion/volumememory.py`).
    #[serde(default)]
    pub volumes: HashMap<String, VolumeEntry>,
}

/// Rotation playlist state for serde. Validation (interval whitelist)
/// happens when the queue is built, so corrupt files fall back safely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RotationConfig {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default = "default_repeat")]
    pub repeat: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            files: vec![],
            index: 0,
            shuffle: false,
            repeat: true,
            enabled: false,
            interval: default_interval(),
        }
    }
}

fn default_repeat() -> bool {
    true
}

fn default_interval() -> u64 {
    wallmotion_core::rotation::DEFAULT_INTERVAL
}

fn default_pause_on_fullscreen() -> bool {
    true
}

fn default_muted() -> bool {
    true
}

/// One remembered per-file entry. Field defaults keep old hand-edited
/// configs loadable (missing keys fall back to the global defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeEntry {
    #[serde(default = "default_volume")]
    pub volume: u8,
    #[serde(default = "default_muted")]
    pub muted: bool,
}

impl From<VolumeEntry> for wallmotion_core::volumememory::VolumeSetting {
    fn from(e: VolumeEntry) -> Self {
        Self::new(e.volume.min(100), e.muted)
    }
}

impl From<wallmotion_core::volumememory::VolumeSetting> for VolumeEntry {
    fn from(s: wallmotion_core::volumememory::VolumeSetting) -> Self {
        Self {
            volume: s.volume.min(100),
            muted: s.muted,
        }
    }
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
            monitor: String::new(),
            pause_on_fullscreen: default_pause_on_fullscreen(),
            pause_on_battery: false,
            rotation: RotationConfig::default(),
            volumes: HashMap::new(),
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
            monitor: String::new(),
            pause_on_fullscreen: true,
            pause_on_battery: false,
            rotation: RotationConfig::default(),
            volumes: HashMap::new(),
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
