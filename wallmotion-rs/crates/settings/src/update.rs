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
/// Playback library shipped next to the exe since the libmpv switch
/// (v1.2.0): fetched only when missing — a loaded DLL cannot be swapped
/// while the app runs, and the client API is stable across our builds.
pub const DLL_ASSET_NAME: &str = "libmpv-2.dll";
pub const DLL_FILE_NAME: &str = "libmpv-2.dll";
/// Silent auto-checks run at most this often (manual checks anytime).
pub const CHECK_INTERVAL_SECS: u64 = 24 * 3600;

/// Events from background update workers to the UI thread.
#[derive(Debug)]
pub enum UpdateEvent {
    /// Check done: newest tag + download URL (`None` = we are current).
    /// When up to date both carry our own version and no URL.
    /// `dll_url` carries the playback library when the release has one.
    Checked {
        latest: String,
        url: Option<String>,
        dll_url: Option<String>,
    },
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

/// `(tag_name, exe_url, dll_url)` from a `releases/latest` JSON body.
/// `None` on unexpected shape or when our exe asset is missing. The DLL
/// URL is `None` for pre-libmpv releases (then nothing is fetched).
pub fn parse_release(body: &str) -> Option<(String, String, Option<String>)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let mut exe_url = None;
    let mut dll_url = None;
    for a in v.get("assets")?.as_array()? {
        let name = a.get("name")?.as_str()?;
        let dl = a.get("browser_download_url")?.as_str()?;
        if name == ASSET_NAME {
            exe_url = Some(dl.to_string());
        } else if name == DLL_ASSET_NAME {
            dll_url = Some(dl.to_string());
        }
    }
    Some((tag, exe_url?, dll_url))
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("unknown exe path ({e})"))
}

/// Shell-facing path: `cmd.exe`/`start` cannot handle verbatim (`\\?\`)
/// paths and report "cannot find '\\'". Filesystem APIs take the
/// verbatim form fine, so only convert at shell boundaries.
fn shell_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    s.into_owned()
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

/// Playback library next to the exe (load-time linked: present =
/// playable, missing = video shows an error).
pub fn dll_path() -> Result<PathBuf, String> {
    let exe = exe_path()?;
    let dir = exe
        .parent()
        .ok_or_else(|| "unknown exe folder".to_string())?;
    Ok(dir.join(DLL_FILE_NAME))
}

/// True when the playback library is absent and must be fetched.
pub fn dll_missing() -> bool {
    dll_path().map(|p| !p.is_file()).unwrap_or(true)
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
    if let Ok(exe) = exe_path() {
        if let Some(dir) = exe.parent() {
            let _ = std::fs::remove_file(dir.join(format!("{DLL_FILE_NAME}.download")));
        }
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
            Some((tag, url, dll_url)) if is_newer(current_version(), &tag) => {
                let _ = tx.send(UpdateEvent::Checked {
                    latest: tag,
                    url: Some(url),
                    dll_url,
                });
            }
            Some((tag, _, _)) => {
                let _ = tx.send(UpdateEvent::Checked {
                    latest: tag,
                    url: None,
                    dll_url: None,
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

/// Download `url` into the stage path on a background thread. When
/// `fetch_dll` the playback library comes along too (only ever needed
/// when missing: straight to its final name via a temp file, since no
/// locked file is being replaced).
pub fn spawn_download(
    tx: Sender<UpdateEvent>,
    url: String,
    dll_url: Option<String>,
    fetch_dll: bool,
) {
    std::thread::spawn(move || {
        let stage = match stage_path() {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(UpdateEvent::Error(e));
                return;
            }
        };
        if let Err(e) = crate::youtube::fetch_url(&url, &stage) {
            let _ = tx.send(UpdateEvent::Error(e));
            return;
        }
        if fetch_dll {
            let Some(dll) = dll_url else {
                let _ = tx.send(UpdateEvent::Error(
                    "the release has no playback library".to_string(),
                ));
                return;
            };
            let target = match dll_path() {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(UpdateEvent::Error(e));
                    return;
                }
            };
            let tmp = target.with_file_name(format!("{DLL_FILE_NAME}.download"));
            if let Err(e) = crate::youtube::fetch_url(&dll, &tmp) {
                let _ = tx.send(UpdateEvent::Error(e));
                return;
            }
            if let Err(e) = std::fs::rename(&tmp, &target) {
                let _ = tx.send(UpdateEvent::Error(format!(
                    "cannot stage the playback library ({e})"
                )));
                return;
            }
        }
        let _ = tx.send(UpdateEvent::Downloaded);
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
/// caller quits. The exe runs quoted directly (no `start`: its title
/// parsing chokes on some paths). No console window pops up (GUI
/// subsystem + hidden cmd, which lingers invisibly until the app quits).
pub fn spawn_restart() -> Result<(), String> {
    let exe = shell_path(&exe_path()?);
    crate::debug_log(&format!("restart cmd: timeout 3 & \"{exe}\""));
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "timeout", "/t", "3", "/nobreak", ">nul", "&", &exe])
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
            Some(("v1.1.5".to_string(), "https://x/w.exe".to_string(), None))
        );
        let with_dll = serde_json::json!({
            "tag_name": "v1.2.0",
            "assets": [
                {"name": "WallMotion-Settings.exe", "browser_download_url": "https://x/w.exe"},
                {"name": "libmpv-2.dll", "browser_download_url": "https://x/m.dll"},
            ]
        })
        .to_string();
        assert_eq!(
            parse_release(&with_dll),
            Some((
                "v1.2.0".to_string(),
                "https://x/w.exe".to_string(),
                Some("https://x/m.dll".to_string())
            ))
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

    #[test]
    fn shell_path_strips_verbatim() {
        assert_eq!(
            shell_path(Path::new(r"\\?\C:\app\w.exe")),
            r"C:\app\w.exe".to_string()
        );
        assert_eq!(
            shell_path(Path::new(r"\\?\UNC\server\share\w.exe")),
            r"\\server\share\w.exe".to_string()
        );
        assert_eq!(
            shell_path(Path::new(r"C:\app\w.exe")),
            r"C:\app\w.exe".to_string()
        );
    }
}
