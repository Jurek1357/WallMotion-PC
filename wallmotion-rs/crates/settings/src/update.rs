//! In-app updates from GitHub Releases (no manual downloads).
//!
//! Flow: Check (`releases/latest` JSON via curl, like the yt-dlp tool
//! installs — no new dependencies) → compare semver with our build →
//! Download the asset next to the exe → Install on restart. A running
//! exe cannot be overwritten on Windows, but it CAN be renamed, which
//! is what the install step relies on.
//!
//! All network/FS work runs on background threads; the UI polls events
//! and stays responsive. Windows-only for now (an AppImage cannot swap
//! itself the same way).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// Latest-release metadata (public, rate-limited: 60 req/h per IP —
/// plenty for daily checks).
pub const RELEASES_API: &str =
    "https://api.github.com/repos/Jurek1357/WallMotion-PC/releases/latest";
/// Release asset installed by the updater.
pub const ASSET_NAME: &str = "WallMotion-Settings.exe";
/// Silent auto-checks run at most this often (manual checks anytime).
pub const CHECK_INTERVAL_SECS: u64 = 24 * 3600;

/// Events from background update workers to the UI thread.
#[derive(Debug)]
pub enum UpdateEvent {
    /// Check done: newest tag + download URL (`None` = we are current).
    /// When up to date both carry our own version and no URL.
    Checked { latest: String, url: Option<String> },
    /// Download finished and staged (install runs right away).
    Downloaded,
    /// Any failure (network, parse, missing asset, read-only folder).
    Error(String),
}

/// Our build version (`wallmotion-core::VERSION`, workspace package).
pub fn current_version() -> &'static str {
    wallmotion_core::VERSION
}

/// `"v1.2.3"` / `"1.2"` → `(1, 2, 3)`. Missing parts are zero.
/// Anything else → `None` (never offer an update we cannot compare).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().strip_prefix(['v', 'V']).unwrap_or(s.trim());
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split('.');
    let num = |p: Option<&str>| -> Option<u64> {
        match p {
            None => Some(0),
            Some(t) if t.bytes().all(|b| b.is_ascii_digit()) && !t.is_empty() => {
                t.parse::<u64>().ok()
            }
            _ => None,
        }
    };
    let major = num(parts.next())?;
    let minor = num(parts.next())?;
    let patch = num(parts.next())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// True when `latest` is strictly newer than `current`.
/// Unparseable sides mean "don't touch anything" (`false`).
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// `(tag_name, asset_url)` from a `releases/latest` JSON body. `None`
/// on unexpected shape or when our asset is missing from the release.
pub fn parse_release(body: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let url = v
        .get("assets")?
        .as_array()?
        .iter()
        .filter_map(|a| {
            let name = a.get("name")?.as_str()?;
            let dl = a.get("browser_download_url")?.as_str()?;
            (name == ASSET_NAME).then(|| dl.to_string())
        })
        .next()?;
    Some((tag, url))
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("unknown exe path ({e})"))
}

/// Staged download: `<exe-dir>/<exe-name>.update` next to the binary.
pub fn stage_path() -> Result<PathBuf, String> {
    let exe = exe_path()?;
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ASSET_NAME.to_string());
    Ok(exe.with_file_name(format!("{name}.update")))
}

/// Installed backup of the previous build (removed on next startup).
fn backup_path() -> Result<PathBuf, String> {
    Ok(exe_path()?.with_extension("old.exe"))
}

/// Remove stale updater leftovers (previous backup / interrupted
/// download). Best effort, runs once at startup.
pub fn cleanup_leftovers() {
    if let Ok(old) = backup_path() {
        let _ = std::fs::remove_file(old);
    }
    if let Ok(stage) = stage_path() {
        let _ = std::fs::remove_file(stage);
    }
}

/// Check GitHub for a newer release on a background thread.
pub fn spawn_check(tx: Sender<UpdateEvent>) {
    std::thread::spawn(move || {
        let tmp = std::env::temp_dir().join("wallmotion-release.json");
        let _ = std::fs::remove_file(&tmp);
        if let Err(e) = crate::youtube::fetch_url(RELEASES_API, &tmp) {
            let _ = tx.send(UpdateEvent::Error(e));
            return;
        }
        let body = std::fs::read_to_string(&tmp).unwrap_or_default();
        let _ = std::fs::remove_file(&tmp);
        match parse_release(&body) {
            Some((tag, url)) if is_newer(current_version(), &tag) => {
                let _ = tx.send(UpdateEvent::Checked {
                    latest: tag,
                    url: Some(url),
                });
            }
            Some((tag, _)) => {
                let _ = tx.send(UpdateEvent::Checked {
                    latest: tag,
                    url: None,
                });
            }
            None => {
                let _ = tx.send(UpdateEvent::Error(
                    "could not read the release info".to_string(),
                ));
            }
        }
    });
}

/// Download `url` into the stage path on a background thread.
pub fn spawn_download(tx: Sender<UpdateEvent>, url: String) {
    std::thread::spawn(move || {
        let stage = match stage_path() {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(UpdateEvent::Error(e));
                return;
            }
        };
        match crate::youtube::fetch_url(&url, &stage) {
            Ok(()) => {
                let _ = tx.send(UpdateEvent::Downloaded);
            }
            Err(e) => {
                let _ = tx.send(UpdateEvent::Error(e));
            }
        }
    });
}

/// Swap the staged build into place: rename the running exe aside (the
/// one thing Windows allows on a running binary), move the download in.
/// The exe folder must be writable; caller restarts afterwards.
pub fn install_update() -> Result<(), String> {
    let exe = exe_path()?;
    let staged = stage_path()?;
    if !staged.is_file() {
        return Err("update file is missing, download it again".to_string());
    }
    let old = backup_path()?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old)
        .map_err(|e| format!("cannot replace the running app ({e}); close other copies"))?;
    std::fs::rename(&staged, &exe).map_err(|e| format!("cannot install the update ({e})"))?;
    Ok(())
}

/// Relaunch the freshly installed exe after a short delay (this process
/// needs a moment to exit and free the single-instance port), then the
/// caller quits. No console window pops up.
pub fn spawn_restart() -> Result<(), String> {
    let exe = exe_path()?;
    let exe = exe.to_string_lossy().into_owned();
    let mut cmd = std::process::Command::new("cmd");
    cmd.args([
        "/C", "timeout", "/t", "3", "/nobreak", ">nul", "&", "start", "\"\"", &exe,
    ])
    .stdin(std::process::Stdio::null());
    crate::youtube::hide_console(&mut cmd);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot relaunch ({e})"))
}

/// Seconds since the Unix epoch (config `update_checked_at` bookkeeping).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Drop a no-longer-needed staged download (user skipped the restart).
#[allow(dead_code)]
pub fn drop_staged() {
    if let Ok(stage) = stage_path() {
        let _ = std::fs::remove_file(stage);
    }
}

/// Parent folder must be writable or install/restart cannot work.
#[allow(dead_code)]
pub fn exe_dir_writable(path: &Path) -> bool {
    path.parent()
        .map(|dir| {
            let probe = dir.join("wallmotion-write-probe.tmp");
            let ok = std::fs::write(&probe, b"x").is_ok();
            let _ = std::fs::remove_file(&probe);
            ok
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("v1"), Some((1, 0, 0)));
        assert_eq!(parse_version("V10.0.1"), Some((10, 0, 1)));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version("1.2.x"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn newer_comparison() {
        assert!(is_newer("1.1.4", "v1.1.5"));
        assert!(is_newer("1.1.4", "v1.2.0"));
        assert!(is_newer("1.1.4", "v2.0.0"));
        assert!(!is_newer("1.1.4", "v1.1.4"));
        assert!(!is_newer("1.1.5", "v1.1.4"));
        assert!(!is_newer("1.1.4", "garbage"));
        assert!(!is_newer("garbage", "v9.9.9"));
    }

    #[test]
    fn release_parsing() {
        let body = serde_json::json!({
            "tag_name": "v1.1.5",
            "assets": [
                {"name": "WallMotion-x86_64.AppImage", "browser_download_url": "https://x/app"},
                {"name": "WallMotion-Settings.exe", "browser_download_url": "https://x/w.exe"},
            ]
        })
        .to_string();
        assert_eq!(
            parse_release(&body),
            Some(("v1.1.5".to_string(), "https://x/w.exe".to_string()))
        );
        let missing = serde_json::json!({
            "tag_name": "v1.1.5",
            "assets": [{"name": "other.zip", "browser_download_url": "https://x/o"}]
        })
        .to_string();
        assert_eq!(parse_release(&missing), None);
        assert_eq!(parse_release("not json"), None);
        assert_eq!(parse_release("{}"), None);
    }

    #[test]
    fn current_version_is_semver() {
        assert!(parse_version(current_version()).is_some());
    }
}
