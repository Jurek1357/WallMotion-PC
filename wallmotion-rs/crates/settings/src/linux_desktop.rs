//! Linux desktop backends for Wayland wallpaper application.
//!
//! Wayland deliberately has no portable "set the wallpaper" protocol.
//! Rather than faking a canvas, we drive the desktop's own renderer:
//!
//! * GNOME: `gsettings` for images, Hanabi's schema for video.
//! * KDE Plasma: `plasma-apply-wallpaperimage` for images. Plasma has no
//!   reliable scriptable video backend, so video stays unsupported.
//! * wlroots compositors (sway, Hyprland, Wayfire, ...): `swww` for
//!   images and `mpvpaper` for video.
//!
//! X11 keeps the existing in-process canvas + mpv path and never enters
//! this module. Everything here is Linux-only.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

const SYSTEM_BIN_DIRS: [&str; 3] = ["/usr/bin", "/bin", "/usr/local/bin"];
const HANABI_UUID: &str = "hanabi-extension@jeffshee.github.io";
const HANABI_SCHEMA: &str = "io.github.jeffshee.hanabi-extension";
const MPV_IPC_SOCKET_NAME: &str = "wallmotion-mpv.sock";
const BACKGROUND_SCHEMA: &str = "org.gnome.desktop.background";

/// X11 vs Wayland, as reported by the session itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    X11,
    Wayland,
    Unknown,
}

impl SessionType {
    pub fn is_wayland(self) -> bool {
        self == Self::Wayland
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::X11 => "X11",
            Self::Wayland => "Wayland",
            Self::Unknown => "unknown",
        }
    }
}

/// Desktop family relevant for backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    Gnome,
    Kde,
    Wlroots,
    Other,
}

impl Desktop {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gnome => "GNOME",
            Self::Kde => "KDE Plasma",
            Self::Wlroots => "wlroots",
            Self::Other => "Wayland desktop",
        }
    }
}

/// Result of probing the environment. Kept free of process spawning so it
/// can be unit-tested and logged before any GUI work happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session: SessionType,
    pub desktop: Desktop,
    /// Raw desktop identifiers, for logs only.
    pub desktops: String,
}

impl SessionInfo {
    pub fn detect() -> Self {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    fn from_vars(get: impl Fn(&str) -> Option<String>) -> Self {
        let session_type = get("XDG_SESSION_TYPE").unwrap_or_default();
        let xdg_current = get("XDG_CURRENT_DESKTOP").unwrap_or_default();
        let desktop_session = get("DESKTOP_SESSION").unwrap_or_default();
        let session_desktop = get("XDG_SESSION_DESKTOP").unwrap_or_default();
        let desktops = [xdg_current, desktop_session, session_desktop]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(":");

        let session = if session_type.eq_ignore_ascii_case("wayland")
            || get("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        {
            SessionType::Wayland
        } else if session_type.eq_ignore_ascii_case("x11")
            || get("DISPLAY").is_some_and(|v| !v.is_empty())
        {
            SessionType::X11
        } else {
            SessionType::Unknown
        };

        let lower = desktops.to_ascii_lowercase();
        let wlroots_env = get("SWAYSOCK").is_some_and(|v| !v.is_empty())
            || get("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty())
            || get("WAYFIRE_SOCKET").is_some_and(|v| !v.is_empty())
            || get("NIRI_SOCKET").is_some_and(|v| !v.is_empty());
        let desktop = if lower.contains("gnome") || lower.contains("ubuntu:gnome") {
            Desktop::Gnome
        } else if lower.contains("kde") || lower.contains("plasma") {
            Desktop::Kde
        } else if wlroots_env
            || [
                "sway", "hyprland", "wlroots", "wayfire", "river", "labwc", "niri", "dwl",
            ]
            .iter()
            .any(|name| lower.contains(name))
        {
            Desktop::Wlroots
        } else {
            Desktop::Other
        };

        Self {
            session,
            desktop,
            desktops,
        }
    }

    pub fn backend_kind(&self) -> BackendKind {
        match self.session {
            SessionType::X11 => BackendKind::X11Canvas,
            SessionType::Wayland => match self.desktop {
                Desktop::Gnome => BackendKind::Gnome,
                Desktop::Kde => BackendKind::Kde,
                Desktop::Wlroots => BackendKind::Wlroots,
                Desktop::Other => BackendKind::Unsupported,
            },
            SessionType::Unknown => BackendKind::Unsupported,
        }
    }

    pub fn description(&self) -> String {
        format!("{} ({})", self.desktop.label(), self.session.label())
    }
}

/// Which implementation (if any) can apply wallpapers here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    X11Canvas,
    Gnome,
    Kde,
    Wlroots,
    Unsupported,
}

impl BackendKind {
    pub fn image_label(self) -> &'static str {
        match self {
            Self::X11Canvas => "X11 canvas",
            Self::Gnome => "GNOME gsettings",
            Self::Kde => "KDE Plasma",
            Self::Wlroots => "swww",
            Self::Unsupported => "no image backend",
        }
    }

    /// `None` means video wallpaper cannot be provided on this desktop.
    pub fn video_label(self) -> Option<&'static str> {
        match self {
            Self::X11Canvas => Some("mpv canvas"),
            Self::Gnome => Some("Hanabi"),
            Self::Wlroots => Some("mpvpaper"),
            Self::Kde | Self::Unsupported => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    MissingTools(Vec<&'static str>),
    NoBackend(String),
    ImageFailed(String),
    VideoFailed(String),
    HanabiMissing,
    HanabiQueued,
    VideoUnsupported(&'static str),
}

/// A Wayland wallpaper backend bound to the detected session.
pub struct DesktopBackend {
    info: SessionInfo,
    kind: BackendKind,
    child: Option<Child>,
    ipc_socket: Option<PathBuf>,
    gnome_saved: Option<Vec<(String, String)>>,
    wlroots_seen: bool,
    wlroots_saved: Option<String>,
    hanabi_active: bool,
    hanabi_paused_path: Option<String>,
}

impl DesktopBackend {
    pub fn detect() -> Self {
        Self::new(SessionInfo::detect())
    }

    pub fn new(info: SessionInfo) -> Self {
        let kind = info.backend_kind();
        crate::debug_log(&format!(
            "linux session: {} session={:?} desktop={:?} backend={kind:?}",
            info.desktops, info.session, info.desktop
        ));
        Self {
            info,
            kind,
            child: None,
            ipc_socket: None,
            gnome_saved: None,
            wlroots_seen: false,
            wlroots_saved: None,
            hanabi_active: false,
            hanabi_paused_path: None,
        }
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    pub fn kind(&self) -> BackendKind {
        self.kind
    }

    pub fn image_label(&self) -> &'static str {
        self.kind.image_label()
    }

    pub fn video_label(&self) -> Option<&'static str> {
        self.kind.video_label()
    }

    /// Apply a static image through the desktop's own tool.
    pub fn set_image(&mut self, path: &Path) -> Result<(), BackendError> {
        let path = absolute_path(path);
        self.stop_video();
        match self.kind {
            BackendKind::Gnome => self.gnome_set_image(&path),
            BackendKind::Kde => {
                let tool = session_tool("plasma-apply-wallpaperimage").ok_or_else(|| {
                    BackendError::MissingTools(vec!["plasma-apply-wallpaperimage"])
                })?;
                run_checked(Command::new(tool).arg(&path))
                    .map_err(BackendError::ImageFailed)
                    .map(|_| {
                        crate::debug_log(&format!("kde image applied: {}", path.display()));
                    })
            }
            BackendKind::Wlroots => self.wlroots_set_image(&path),
            BackendKind::X11Canvas => Err(BackendError::NoBackend(
                "X11 uses the in-process canvas backend".to_string(),
            )),
            BackendKind::Unsupported => Err(BackendError::NoBackend(self.info.description())),
        }
    }

    /// Start video wallpaper playback with the desktop's renderer.
    pub fn set_video(&mut self, path: &Path, muted: bool, volume: u8) -> Result<(), BackendError> {
        let path = absolute_path(path);
        match self.kind {
            BackendKind::Gnome => {
                self.stop_video();
                self.gnome_set_video(&path, muted, volume)
            }
            BackendKind::Wlroots => {
                self.stop_video();
                self.wlroots_set_video(&path, muted, volume)
            }
            BackendKind::Kde => Err(BackendError::VideoUnsupported(self.info.desktop.label())),
            BackendKind::X11Canvas => Err(BackendError::NoBackend(
                "X11 uses the in-process mpv backend".to_string(),
            )),
            BackendKind::Unsupported => Err(BackendError::NoBackend(self.info.description())),
        }
    }

    /// Stop video playback. Never restores the previous static image.
    pub fn stop_video(&mut self) {
        if let Some(mut child) = self.child.take() {
            crate::debug_log(&format!("stopping video renderer pid={}", child.id()));
            terminate_process_group(&mut child);
        }
        if let Some(socket) = self.ipc_socket.take() {
            let _ = std::fs::remove_file(socket);
        }
        if self.kind == BackendKind::Gnome
            && (self.hanabi_active || self.hanabi_paused_path.is_some())
        {
            let _ = hanabi_set("video-path", "");
            let _ = hanabi_disable();
        }
        self.hanabi_active = false;
        self.hanabi_paused_path = None;
    }

    /// Pause/resume playback. mpvpaper supports mpv IPC; Hanabi has no
    /// pause key, so it is disabled and its path is stashed.
    pub fn set_paused(&mut self, paused: bool) {
        match self.kind {
            BackendKind::Wlroots => {
                if let Some(socket) = &self.ipc_socket {
                    let _ = mpv_ipc_set(socket, "pause", serde_json::json!(paused));
                }
            }
            BackendKind::Gnome => {
                if paused {
                    if let Some(raw) = hanabi_get("video-path") {
                        let path = unquote_gvariant(&raw);
                        if !path.is_empty() {
                            self.hanabi_paused_path = Some(path);
                        }
                    }
                    let _ = hanabi_set("video-path", "");
                    if hanabi_state() == HanabiState::Enabled {
                        let _ = hanabi_disable();
                    }
                } else if let Some(path) = self.hanabi_paused_path.take() {
                    let _ = hanabi_set("video-path", &path);
                    let _ = hanabi_enable();
                }
            }
            _ => {}
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        match self.kind {
            BackendKind::Wlroots => {
                if let Some(socket) = &self.ipc_socket {
                    let _ = mpv_ipc_set(socket, "mute", serde_json::json!(muted));
                }
            }
            BackendKind::Gnome if self.hanabi_active => {
                let _ = hanabi_set("mute", if muted { "true" } else { "false" });
            }
            _ => {}
        }
    }

    pub fn set_volume(&mut self, volume: u8) {
        match self.kind {
            BackendKind::Wlroots => {
                if let Some(socket) = &self.ipc_socket {
                    let _ = mpv_ipc_set(
                        socket,
                        "volume",
                        serde_json::json!(f64::from(volume.min(100))),
                    );
                }
            }
            BackendKind::Gnome if self.hanabi_active => {
                let _ = hanabi_set("volume", &volume.min(100).to_string());
            }
            _ => {}
        }
    }

    /// Restore the desktop wallpaper that was active before we changed it.
    /// `Ok(false)` means this backend has nothing to restore.
    pub fn restore_image(&mut self) -> Result<bool, BackendError> {
        self.stop_video();
        match self.kind {
            BackendKind::Gnome => {
                let Some(saved) = self.gnome_saved.take() else {
                    crate::debug_log("restore(gnome): no captured wallpaper state");
                    return Ok(false);
                };
                let tool = session_tool("gsettings")
                    .ok_or_else(|| BackendError::MissingTools(vec!["gsettings"]))?;
                for (key, raw) in saved {
                    run_checked(
                        Command::new(&tool)
                            .arg("set")
                            .arg(BACKGROUND_SCHEMA)
                            .arg(&key)
                            .arg(&raw),
                    )
                    .map_err(|e| BackendError::ImageFailed(format!("restore {key}: {e}")))?;
                }
                crate::debug_log("restore(gnome): previous wallpaper restored");
                Ok(true)
            }
            BackendKind::Wlroots => {
                if !self.wlroots_seen {
                    crate::debug_log("restore(wlroots): no captured wallpaper state");
                    return Ok(false);
                }
                self.wlroots_seen = false;
                let Some(saved) = self.wlroots_saved.take() else {
                    return Ok(false);
                };
                let tool =
                    find_tool("swww").ok_or_else(|| BackendError::MissingTools(vec!["swww"]))?;
                run_checked(
                    Command::new(tool)
                        .args(["img", "--resize", "crop"])
                        .arg(&saved),
                )
                .map_err(BackendError::ImageFailed)
                .map(|_| {
                    crate::debug_log("restore(wlroots): previous wallpaper restored");
                    true
                })
            }
            _ => Ok(false),
        }
    }

    fn gnome_set_image(&mut self, path: &Path) -> Result<(), BackendError> {
        let tool = session_tool("gsettings")
            .ok_or_else(|| BackendError::MissingTools(vec!["gsettings"]))?;
        if self.gnome_saved.is_none() {
            let mut saved = Vec::new();
            for key in ["picture-uri", "picture-uri-dark"] {
                if let Ok(raw) =
                    run_checked(Command::new(&tool).args(["get", BACKGROUND_SCHEMA, key]))
                {
                    saved.push((key.to_string(), raw.trim().to_string()));
                }
            }
            self.gnome_saved = Some(saved);
        }
        let uri = file_uri(path);
        for key in ["picture-uri", "picture-uri-dark"] {
            // `picture-uri-dark` does not exist on very old GNOME releases;
            // if it was not snapshotted, it is not available here either.
            let known = self
                .gnome_saved
                .as_ref()
                .is_some_and(|saved| saved.iter().any(|(k, _)| k == key));
            if !known && key == "picture-uri-dark" {
                continue;
            }
            run_checked(
                Command::new(&tool)
                    .arg("set")
                    .arg(BACKGROUND_SCHEMA)
                    .arg(key)
                    .arg(&uri),
            )
            .map_err(BackendError::ImageFailed)?;
        }
        crate::debug_log(&format!("gnome image applied: {}", path.display()));
        Ok(())
    }

    fn wlroots_set_image(&mut self, path: &Path) -> Result<(), BackendError> {
        let tool = find_tool("swww").ok_or_else(|| BackendError::MissingTools(vec!["swww"]))?;
        self.ensure_swww_daemon()?;
        if !self.wlroots_seen {
            self.wlroots_seen = true;
            let query =
                run_checked(Command::new(&tool).arg("query")).map_err(BackendError::ImageFailed)?;
            self.wlroots_saved = parse_swww_image(&query);
        }
        run_checked(
            Command::new(&tool)
                .args(["img", "--resize", "crop"])
                .arg(path),
        )
        .map_err(BackendError::ImageFailed)?;
        crate::debug_log(&format!("swww image applied: {}", path.display()));
        Ok(())
    }

    fn ensure_swww_daemon(&self) -> Result<(), BackendError> {
        let tool = find_tool("swww").ok_or_else(|| BackendError::MissingTools(vec!["swww"]))?;
        if Command::new(&tool)
            .arg("query")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            return Ok(());
        }
        let daemon =
            find_tool("swww-daemon").ok_or_else(|| BackendError::MissingTools(vec!["swww"]))?;
        Command::new(daemon)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| BackendError::ImageFailed(format!("swww-daemon: {e}")))?;
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(200));
            if Command::new(&tool)
                .arg("query")
                .output()
                .is_ok_and(|o| o.status.success())
            {
                return Ok(());
            }
        }
        Err(BackendError::ImageFailed(
            "swww-daemon did not become ready".to_string(),
        ))
    }

    fn gnome_set_video(
        &mut self,
        path: &Path,
        muted: bool,
        volume: u8,
    ) -> Result<(), BackendError> {
        let state = hanabi_state();
        match state {
            HanabiState::Missing | HanabiState::Unknown => {
                crate::debug_log(&format!("hanabi state: {state:?}"));
                return Err(BackendError::HanabiMissing);
            }
            HanabiState::Queued => return Err(BackendError::HanabiQueued),
            HanabiState::Enabled | HanabiState::Installed => {}
        }
        if session_tool("gsettings").is_none() {
            return Err(BackendError::MissingTools(vec!["gsettings"]));
        }
        let Some(path_str) = path.to_str() else {
            return Err(BackendError::VideoFailed(
                "video path is not valid UTF-8".to_string(),
            ));
        };
        hanabi_set("mute", if muted { "true" } else { "false" })
            .map_err(BackendError::VideoFailed)?;
        hanabi_set("volume", &volume.min(100).to_string()).map_err(BackendError::VideoFailed)?;
        hanabi_set("video-path", path_str).map_err(BackendError::VideoFailed)?;
        if state == HanabiState::Installed {
            hanabi_enable().map_err(BackendError::VideoFailed)?;
        }
        self.hanabi_active = true;
        self.hanabi_paused_path = None;
        crate::debug_log(&format!("hanabi video started: {path_str}"));
        Ok(())
    }

    fn wlroots_set_video(
        &mut self,
        path: &Path,
        muted: bool,
        volume: u8,
    ) -> Result<(), BackendError> {
        let tool =
            find_tool("mpvpaper").ok_or_else(|| BackendError::MissingTools(vec!["mpvpaper"]))?;
        let socket = ipc_socket_path();
        let _ = std::fs::remove_file(&socket);
        let opts = mpvpaper_options(muted, volume, &socket);
        let mut child = Command::new(tool)
            .arg("-o")
            .arg(opts)
            .arg("*")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| BackendError::VideoFailed(format!("mpvpaper: {e}")))?;
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(400));
        if let Ok(Some(status)) = child.try_wait() {
            return Err(BackendError::VideoFailed(format!(
                "mpvpaper exited during startup ({status})"
            )));
        }
        self.child = Some(child);
        self.ipc_socket = Some(socket);
        crate::debug_log(&format!(
            "mpvpaper started pid={pid} path={}",
            path.display()
        ));
        Ok(())
    }
}

impl Drop for DesktopBackend {
    fn drop(&mut self) {
        self.stop_video();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HanabiState {
    Enabled,
    Installed,
    Queued,
    Missing,
    Unknown,
}

fn hanabi_dir() -> PathBuf {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            home.join(".local").join("share")
        });
    data_home
        .join("gnome-shell")
        .join("extensions")
        .join(HANABI_UUID)
}

fn hanabi_state() -> HanabiState {
    let Some(exe) = session_tool("gnome-extensions") else {
        return HanabiState::Unknown;
    };
    let Ok(list) = run_checked(Command::new(&exe).args(["list"])) else {
        return HanabiState::Unknown;
    };
    if !list.lines().any(|line| line.trim() == HANABI_UUID) {
        return if hanabi_dir().is_dir() {
            HanabiState::Queued
        } else {
            HanabiState::Missing
        };
    }
    if run_checked(Command::new(&exe).args(["list", "--enabled"]))
        .is_ok_and(|out| out.lines().any(|line| line.trim() == HANABI_UUID))
    {
        HanabiState::Enabled
    } else {
        HanabiState::Installed
    }
}

fn hanabi_disable() -> Result<(), String> {
    let exe =
        session_tool("gnome-extensions").ok_or_else(|| "gnome-extensions not found".to_string())?;
    run_checked(Command::new(exe).args(["disable", HANABI_UUID])).map(|_| ())
}

fn hanabi_enable() -> Result<(), String> {
    if let Some(exe) = session_tool("gnome-extensions") {
        if run_checked(Command::new(exe).args(["enable", HANABI_UUID])).is_ok() {
            return Ok(());
        }
    }
    let exe = session_tool("gsettings").ok_or_else(|| "gsettings not found".to_string())?;
    let current =
        run_checked(Command::new(&exe).args(["get", "org.gnome.shell", "enabled-extensions"]))?;
    let Some(updated) = append_gvariant_string(&current, HANABI_UUID) else {
        return Err(format!(
            "could not parse enabled-extensions: {}",
            current.trim()
        ));
    };
    run_checked(
        Command::new(exe)
            .arg("set")
            .arg("org.gnome.shell")
            .arg("enabled-extensions")
            .arg(&updated),
    )
    .map(|_| ())
}

fn hanabi_set(key: &str, value: &str) -> Result<(), String> {
    let exe = session_tool("gsettings").ok_or_else(|| "gsettings not found".to_string())?;
    let mut cmd = Command::new(exe);
    cmd.env("GSETTINGS_SCHEMA_DIR", hanabi_dir().join("schemas"))
        .args(["set", HANABI_SCHEMA, key, value]);
    run_checked(&mut cmd).map(|_| ())
}

fn hanabi_get(key: &str) -> Option<String> {
    let exe = session_tool("gsettings")?;
    let mut cmd = Command::new(exe);
    cmd.env("GSETTINGS_SCHEMA_DIR", hanabi_dir().join("schemas"))
        .args(["get", HANABI_SCHEMA, key]);
    run_checked(&mut cmd).ok()
}

/// Resolve a tool that must talk to the real user session. Canonical
/// system dirs win over PATH: a Linuxbrew/Conda `gsettings` in PATH uses
/// the keyfile backend and silently writes to the wrong settings store.
fn session_tool(name: &str) -> Option<PathBuf> {
    for dir in SYSTEM_BIN_DIRS {
        let path = Path::new(dir).join(name);
        if is_executable(&path) {
            return Some(path);
        }
    }
    find_tool(name)
}

/// Resolve a renderer tool (swww, mpvpaper, ...) from PATH.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return is_executable(&path).then_some(path);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Absolute `file://` URI for gsettings. Percent-encodes everything except
/// unreserved path characters, so spaces and non-ASCII filenames work.
pub fn file_uri(path: &Path) -> String {
    let path = absolute_path(path);
    let text = path.to_string_lossy();
    let mut uri = String::from("file://");
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'.' | b'_' | b'~' => {
                uri.push(*byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

fn unquote_gvariant(raw: &str) -> String {
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("@s ")
        .or_else(|| raw.strip_prefix("@as "))
        .unwrap_or(raw);
    raw.strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(raw)
        .to_string()
}

fn append_gvariant_string(raw: &str, item: &str) -> Option<String> {
    let text = raw.trim();
    let text = text.strip_prefix("@as ").unwrap_or(text).trim();
    if !text.starts_with('[') || !text.ends_with(']') {
        return None;
    }
    let inner = text[1..text.len() - 1].trim();
    if inner.contains(&format!("'{item}'")) {
        return Some(text.to_string());
    }
    if inner.is_empty() {
        Some(format!("['{item}']"))
    } else {
        Some(format!("[{inner}, '{item}']"))
    }
}

fn parse_swww_image(query: &str) -> Option<String> {
    query.lines().find_map(|line| {
        line.split_once("image: ")
            .map(|(_, path)| path.trim().to_string())
    })
}

fn mpvpaper_options(muted: bool, volume: u8, socket: &Path) -> String {
    let volume = volume.min(100);
    let mute = if muted { "mute=yes" } else { "mute=no" };
    format!(
        "loop-file=inf {mute} volume={volume} input-ipc-server={}",
        socket.display()
    )
}

fn ipc_socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(std::env::temp_dir);
    base.join(MPV_IPC_SOCKET_NAME)
}

fn mpv_ipc_set(socket: &Path, prop: &str, value: serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let mut stream = std::os::unix::net::UnixStream::connect(socket)
        .map_err(|e| format!("mpv IPC connect: {e}"))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| format!("mpv IPC timeout: {e}"))?;
    let command = serde_json::json!({"command": ["set_property", prop, value]});
    writeln!(stream, "{command}").map_err(|e| format!("mpv IPC write: {e}"))
}

fn run_checked(cmd: &mut Command) -> Result<String, String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd.output().map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.is_empty() {
            Err(format!("{program} exited with {}", out.status))
        } else {
            Err(format!("{program} exited with {}: {stderr}", out.status))
        }
    }
}

fn terminate_process_group(child: &mut Child) {
    let pid = child.id() as i32;
    kill_group(pid, 15);
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
    kill_group(pid, 9);
    let _ = child.wait();
}

fn kill_group(pid: i32, signal: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // Negative PID targets the process group created by `process_group(0)`.
    unsafe {
        kill(-pid, signal);
    }
}

/// Show a startup error with the first desktop dialog tool available.
pub fn show_error_dialog(message: &str) {
    let title = "WallMotion";
    if let Some(tool) = find_tool("zenity") {
        let _ = Command::new(tool)
            .args(["--error", "--title", title, "--text", message])
            .status();
        return;
    }
    if let Some(tool) = find_tool("kdialog") {
        let _ = Command::new(tool)
            .args(["--title", title, "--error", message])
            .status();
        return;
    }
    if let Some(tool) = find_tool("xmessage") {
        let _ = Command::new(tool).args(["-center", message]).status();
        return;
    }
    eprintln!("{title}: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(items: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let items: Vec<(String, String)> = items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| {
            items
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn detects_wayland_gnome() {
        let info = SessionInfo::from_vars(vars(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
        ]));
        assert_eq!(info.session, SessionType::Wayland);
        assert_eq!(info.desktop, Desktop::Gnome);
        assert_eq!(info.backend_kind(), BackendKind::Gnome);
    }

    #[test]
    fn detects_wayland_wlroots_from_compositor_env() {
        let info = SessionInfo::from_vars(vars(&[
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc"),
        ]));
        assert_eq!(info.session, SessionType::Wayland);
        assert_eq!(info.desktop, Desktop::Wlroots);
        assert_eq!(info.backend_kind(), BackendKind::Wlroots);
    }

    #[test]
    fn x11_keeps_canvas_backend() {
        let info = SessionInfo::from_vars(vars(&[
            ("XDG_SESSION_TYPE", "x11"),
            ("DISPLAY", ":0"),
            ("XDG_CURRENT_DESKTOP", "GNOME"),
        ]));
        assert_eq!(info.session, SessionType::X11);
        assert_eq!(info.backend_kind(), BackendKind::X11Canvas);
    }

    #[test]
    fn kde_wayland_has_images_but_no_video() {
        let info = SessionInfo::from_vars(vars(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("XDG_CURRENT_DESKTOP", "KDE"),
        ]));
        assert_eq!(info.backend_kind(), BackendKind::Kde);
        assert!(info.backend_kind().video_label().is_none());
    }

    #[test]
    fn file_uri_encodes_spaces_and_unicode() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b/žluť.png")),
            "file:///tmp/a%20b/%C5%BElu%C5%A5.png"
        );
    }

    #[test]
    fn appends_hanabi_uuid_to_gvariant_list() {
        assert_eq!(
            append_gvariant_string("@as []", HANABI_UUID),
            Some(format!("['{HANABI_UUID}']"))
        );
        assert_eq!(
            append_gvariant_string("['a@b']", HANABI_UUID),
            Some(format!("['a@b', '{HANABI_UUID}']"))
        );
        assert_eq!(
            append_gvariant_string(&format!("['{HANABI_UUID}']"), HANABI_UUID),
            Some(format!("['{HANABI_UUID}']"))
        );
    }

    #[test]
    fn parses_swww_query_output() {
        let out = "eDP-1: 1920x1080, scale: 1, currently displaying: image: /p/x.png\n";
        assert_eq!(parse_swww_image(out), Some("/p/x.png".to_string()));
        assert_eq!(parse_swww_image("no image"), None);
    }

    #[test]
    fn mpvpaper_options_keep_audio_stream_unmutable() {
        let socket = Path::new("/run/user/1000/wallmotion-mpv.sock");
        let muted = mpvpaper_options(true, 30, socket);
        assert!(muted.contains("mute=yes"));
        assert!(muted.contains("volume=30"));
        assert!(muted.contains("input-ipc-server=/run/user/1000/wallmotion-mpv.sock"));
    }
}
