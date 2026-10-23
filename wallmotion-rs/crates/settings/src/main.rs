//! Native settings window over the Rust core (video set/stop for now).
//!
//! eframe UI driving `wallmotion-win` canvas setup and the
//! `wallmotion-mpv` in-process playback. Windows only for playback;
//! elsewhere the window opens with an explanatory status.

//! No console window: this is a GUI app (also in debug builds, so a
//! plain double-click never leaves a terminal behind).
#![windows_subsystem = "windows"]

use std::path::PathBuf;

mod autostart;
mod cli;
mod config;
mod hotkeys;
mod i18n;
mod instance;
mod library;
mod remote;
mod theme;
mod update;
mod youtube;

use config::AppConfig;

const TRAY_SHOW_ID: &str = "show";
const TRAY_QUIT_ID: &str = "quit";

/// Set by the tray thread on Quit; the UI thread performs the close
/// (with video cleanup) on its next frame.
static TRAY_QUIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Append one line to the debug log in the temp dir. Best effort, never panics.
fn debug_log(msg: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("wallmotion-settings-debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{} pid={} {msg}", chrono_stamp(), std::process::id());
    }
}

fn chrono_stamp() -> String {
    // No chrono dep: seconds since epoch is enough for a debug log.
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("{}", d.as_secs()),
        Err(_) => "?".to_string(),
    }
}

/// Heartbeat (max one line per minute): proves the UI loop is alive,
/// even with the window hidden to the tray. Diagnoses stalled rotation.
fn heartbeat() {
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now.saturating_sub(LAST.load(std::sync::atomic::Ordering::SeqCst)) >= 60 {
        LAST.store(now, std::sync::atomic::Ordering::SeqCst);
        debug_log("ui heartbeat (loop alive)");
    }
}

/// Playback poll (max one line per 30 s): proves the UI loop still
/// pumps player events while the window is hidden. Includes the mpv
/// `pause` flag + `time-pos` so a frozen picture is diagnosable
/// (paused=true vs. stuck-while-playing vs. EOF missed).
fn playback_poll_log(file: &str, paused: Option<bool>, pos: Option<f64>, eof_set: Option<bool>) {
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now.saturating_sub(LAST.load(std::sync::atomic::Ordering::SeqCst)) >= 30 {
        LAST.store(now, std::sync::atomic::Ordering::SeqCst);
        let paused = paused.map_or("?".to_string(), |p| p.to_string());
        let pos = pos.map_or("?".to_string(), |p| format!("{p:.1}"));
        let eof = eof_set.map_or("?".to_string(), |e| e.to_string());
        debug_log(&format!(
            "playback poll file={file} paused={paused} pos={pos} eof={eof}"
        ));
    }
}

/// Show the main window: un-minimize, un-hide, repaint.
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    #[cfg(windows)]
    reveal_app_window();
    ctx.request_repaint();
}

/// Win32 handle of our window, for taskbar-free hiding (X hides the
/// window from the taskbar while the tray icon keeps running).
/// The window is NEVER really hidden (SW_HIDE stops the UI loop dead,
/// which stalls rotation): it moves far off-screen and loses its
/// taskbar button instead, so frames keep pumping while hidden.
#[cfg(windows)]
static APP_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Window rect saved before off-screen hiding (restored on show).
#[cfg(windows)]
static SAVED_RECT: std::sync::Mutex<(i32, i32, i32, i32)> =
    std::sync::Mutex::new((100, 100, 940, 680));

/// Message-only window owning our main window while hidden: owned
/// windows never get a taskbar button (unlike TOOLWINDOW, which the
/// shell sometimes keeps showing). Wallpaper-Engine style: window
/// visible off-screen, button gone, tray icon stays.
#[cfg(windows)]
fn owner_window() -> isize {
    static OWNER: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
    let raw = OWNER.load(std::sync::atomic::Ordering::SeqCst);
    if raw != 0 {
        return raw;
    }
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::*;
        let h = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::w!("STATIC"),
            windows::core::w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
        .map(|h| h.0 as isize)
        .unwrap_or(0);
        if h != 0 {
            OWNER.store(h, std::sync::atomic::Ordering::SeqCst);
        }
        h
    }
}

#[cfg(windows)]
fn find_app_hwnd() -> isize {
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    let mut title: Vec<u16> = "WallMotion (native)".encode_utf16().collect();
    title.push(0);
    unsafe {
        FindWindowW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR(title.as_ptr()),
        )
        .map(|h| h.0 as isize)
        .unwrap_or(0)
    }
}

/// Hide the window incl. its taskbar button (tray icon stays).
/// Off-screen + toolwindow style: winit still sees a visible window,
/// so the event loop (rotation, tray) keeps running.
#[cfg(windows)]
fn hide_app_window() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::*;
    let raw = APP_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            // Remember where to come back (never the off-screen spot).
            let mut r = windows::Win32::Foundation::RECT::default();
            if GetWindowRect(hwnd, &mut r).is_ok() && r.left > -10000 {
                if let Ok(mut saved) = SAVED_RECT.lock() {
                    *saved = (r.left, r.top, r.right - r.left, r.bottom - r.top);
                }
            }
            // Drop the taskbar button, keep the window "visible".
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0) as i32);
            let ex2 = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            // Owned windows never get a taskbar button (bulletproof).
            let owner = owner_window();
            if owner != 0 {
                let _ = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner);
            }
            let _ = SetWindowPos(
                hwnd,
                None,
                -32000,
                -32000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            debug_log(&format!(
                "hide to tray (off-screen) ex={ex:08x}->{ex2:08x} owner={owner}"
            ));
        }
    }
}

/// Re-show a window hidden with [`hide_app_window`].
#[cfg(windows)]
fn reveal_app_window() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::*;
    let raw = APP_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if raw != 0 {
        unsafe {
            let hwnd = HWND(raw as *mut std::ffi::c_void);
            // Taskbar button back: clear owner first, then the style.
            let _ = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, 0);
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_TOOLWINDOW.0) as i32);
            // Back to the saved spot (or default size).
            let (x, y, w, h) = SAVED_RECT
                .lock()
                .map(|r| *r)
                .unwrap_or((100, 100, 940, 680));
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                x,
                y,
                w.max(200),
                h.max(200),
                SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            let _ = SetForegroundWindow(hwnd);
            debug_log("revealed from tray");
        }
    }
}

/// Polling tray loop: menu events + left-click-to-show. Polling (not
/// blocking) so one thread serves both channels without starving either.
/// Instant response even when the UI thread is throttled while hidden.
fn tray_menu_thread(ctx: egui::Context) {
    std::thread::spawn(move || {
        use std::sync::atomic::Ordering;
        debug_log("tray thread started");
        loop {
            let mut idle = true;
            while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                idle = false;
                debug_log(&format!("menu event: {}", event.id.0.as_str()));
                match event.id.0.as_str() {
                    TRAY_SHOW_ID => show_window(&ctx),
                    TRAY_QUIT_ID => {
                        TRAY_QUIT.store(true, Ordering::SeqCst);
                        ctx.request_repaint();
                    }
                    _ => {}
                }
            }
            while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
                idle = false;
                if let tray_icon::TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = event
                {
                    debug_log("tray left-click: show");
                    show_window(&ctx);
                }
            }
            if idle {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    });
}

struct RunningVideo {
    player: wallmotion_mpv::Player,
    canvas: isize,
    /// Monitor geometry the canvas was built for (stale on monitor
    /// change → next switch rebuilds the canvas).
    #[cfg(windows)]
    rect: (i32, i32, i32, i32),
    /// Only the first runner plays audio; the rest stay muted (one
    /// video on every monitor must not echo).
    audible: bool,
    /// End-of-file signal from the player event queue: None = playing,
    /// Some(true) = clean end, Some(false) = broken file.
    eof: Option<bool>,
    /// First frame painted (PlaybackRestart seen). The sync release
    /// waits for every runner.
    painted: bool,
}

struct App {
    file: String,
    status: String,
    muted: bool,
    volume: u8,
    running: Option<RunningVideo>,
    /// Extra runners for the other monitors (same video everywhere,
    /// muted except the first). Empty unless multi-monitor is active.
    extra: Vec<RunningVideo>,
    /// Prerolled successors on hidden canvases (double-buffered
    /// switch: the new video warms up while the old one keeps
    /// playing, then they swap on first frame — no gap).
    /// Drained by `commit_preroll`, reaped by `stop_video`.
    pending: Vec<RunningVideo>,
    /// Swap deadline for `pending` (first-frame sync, 3 s like
    /// `arm_sync`): swap even if some canvas never painted.
    pending_deadline: Option<std::time::Instant>,
    /// First-frame sync: runners start frozen, release together once
    /// every canvas painted (or the deadline passes).
    sync_waiting: bool,
    sync_deadline: std::time::Instant,
    sync_resume: bool,
    quit_requested: bool,
    paused: bool,
    monitor: String,
    monitors: Vec<MonitorChoice>,
    pause_on_fullscreen: bool,
    pause_on_battery: bool,
    auto: wallmotion_core::autopause::AutoPause,
    auto_paused: bool,
    last_poll: std::time::Instant,
    last_zcheck: std::time::Instant,
    remote_rx: Option<std::sync::mpsc::Receiver<String>>,
    hotkeys: hotkeys::Hotkeys,
    hotkeys_attached: bool,
    show_hotkeys: bool,
    pending_start: bool,
    rotation: wallmotion_core::rotation::RotationQueue,
    rotation_enabled: bool,
    rotation_interval: u64,
    last_rotation: std::time::Instant,
    /// Consecutive rotation files whose mpv exited with an error (broken
    /// files are skipped; a full cycle of them switches rotation off).
    rot_quick_skips: u32,
    library: Vec<library::MediaItem>,
    thumbs: std::collections::HashMap<String, egui::TextureHandle>,
    lib_scanned: bool,
    volumes: std::collections::HashMap<String, wallmotion_core::volumememory::VolumeSetting>,
    // YouTube download (yt-dlp sidecar on a background thread).
    yt_url: String,
    yt_status: String,
    yt_progress: f32,
    yt_busy: bool,
    yt_rx: Option<std::sync::mpsc::Receiver<youtube::YtEvent>>,
    yt_child: youtube::ChildSlot,
    yt_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    yt_playlist: Vec<wallmotion_core::youtube::PlaylistEntry>,
    yt_picked: Vec<bool>,
    yt_queue: std::collections::VecDeque<String>,
    yt_queue_total: usize,
    yt_tools_scanned: bool,
    ytdlp_path: Option<std::path::PathBuf>,
    ytdlp_ver: Option<String>,
    ffmpeg_path: Option<std::path::PathBuf>,
    /// AV1 decoder in the provisioned ffmpeg (`None` = unknown:
    /// ffmpeg missing or won't run). Decided in `rescan_tools`.
    ffmpeg_av1: Option<bool>,
    /// YouTube download codec preference (persisted as `yt_codec`).
    yt_codec: wallmotion_core::youtube::VideoCodec,
    // Background tool auto-install (own channel so it never blocks
    // downloads; started once on launch when a sidecar is missing).
    tool_rx: Option<std::sync::mpsc::Receiver<youtube::YtEvent>>,
    tool_busy: bool,
    tools_auto_started: bool,
    // In-app updater (own channel; Windows only for install).
    upd_rx: Option<std::sync::mpsc::Receiver<update::UpdateEvent>>,
    upd_busy: bool,
    /// Silent auto-check (no status noise) vs. manual button.
    upd_quiet: bool,
    upd_status: String,
    upd_latest: Option<String>,
    upd_url: Option<String>,
    upd_dll_url: Option<String>,
    upd_ready: bool,
    // Windows-only updater bookkeeping (read by windows-gated code).
    #[allow(dead_code)]
    upd_auto_done: bool,
    upd_checked_at: u64,
    upd_skipped: String,
    // Library gallery (Lively-style tab).
    tab: Tab,
    favorites: std::collections::HashSet<String>,
    lib_search: String,
    lib_fav_only: bool,
    lib_selected: Option<String>,
    lib_sort: LibSort,
    lib_type: LibType,
    /// Set when the downloads folder was just created: one-time notice
    /// with its path (session-only, dismissed with OK).
    lib_folder_notice: Option<PathBuf>,
    // Language + theme (header toggles, same keys as the Python app).
    lang: i18n::Lang,
    theme: theme::AppTheme,
    follow_system: bool,
    sys_theme: Option<theme::AppTheme>,
    applied_style: Option<bool>,
    last_theme_poll: std::time::Instant,
    logo: Option<egui::TextureHandle>,
    // Settings preview of the current wallpaper (rebuilt only when
    // `file` changes; videos use the ffmpeg frame cache).
    preview_path: String,
    preview_tex: Option<egui::TextureHandle>,
    // Windows autostart (Run key) + hidden logon start (`--minimized`).
    // Read by windows-gated UI only.
    #[allow(dead_code)]
    autostart: bool,
    start_hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Settings,
    Library,
}

/// Library gallery actions (grid clicks + detail buttons), applied by
/// `library_tab` after both columns render.
enum LibAction {
    Select(String),
    Set(String),
    AddRot(String),
    Delete(String),
    ToggleFav(String),
}

/// Library sort order (session-only, toolbar combo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LibSort {
    #[default]
    Name,
    Newest,
    Oldest,
}

impl LibSort {
    fn key(self) -> &'static str {
        match self {
            LibSort::Name => "sort_name",
            LibSort::Newest => "sort_newest",
            LibSort::Oldest => "sort_oldest",
        }
    }
}

/// Library type filter (session-only, toolbar combo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LibType {
    #[default]
    All,
    Video,
    Image,
}

impl LibType {
    fn key(self) -> &'static str {
        match self {
            LibType::All => "type_all",
            LibType::Video => "type_video",
            LibType::Image => "type_image",
        }
    }
}

#[derive(Debug, Clone)]
struct MonitorChoice {
    name: String,
    label: String,
    // Geometry for canvas placement (windows-gated readers only).
    #[allow(dead_code)]
    rect: Option<(i32, i32, i32, i32)>,
}

#[cfg(windows)]
fn monitor_label(
    lang: i18n::Lang,
    name: &str,
    rect: (i32, i32, i32, i32),
    primary: bool,
) -> String {
    let (l, t, r, b) = rect;
    let tag = if primary {
        i18n::tr(lang, "mon_primary")
    } else {
        String::new()
    };
    format!(
        "{} {name} ({}x{}){tag}",
        i18n::tr(lang, "mon_name"),
        r - l,
        b - t
    )
}

fn discover_monitors(lang: i18n::Lang) -> Vec<MonitorChoice> {
    // Non-Windows builds list no monitors; keep the param used.
    let _ = lang;
    #[cfg(windows)]
    {
        wallmotion_win::canvas::sys::list_monitors()
            .into_iter()
            .map(|m| {
                let label = monitor_label(lang, &m.name, m.rect, m.primary);
                MonitorChoice {
                    name: m.name,
                    label,
                    rect: Some(m.rect),
                }
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

fn poll_fullscreen() -> bool {
    #[cfg(windows)]
    {
        wallmotion_win::autopause::is_fullscreen_app_active()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn poll_battery() -> bool {
    #[cfg(windows)]
    {
        wallmotion_win::autopause::is_on_battery()
    }
    #[cfg(not(windows))]
    {
        wallmotion_core::autopause::is_on_battery_linux()
    }
}

/// Reveal a folder in the system file manager. Best effort.
fn open_folder(dir: &std::path::Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

impl Default for App {
    fn default() -> Self {
        let saved = AppConfig::load();
        let interval = if wallmotion_core::rotation::INTERVALS.contains(&saved.rotation.interval) {
            saved.rotation.interval
        } else {
            wallmotion_core::rotation::DEFAULT_INTERVAL
        };
        let mut rotation = wallmotion_core::rotation::RotationQueue::new(
            saved.rotation.files.clone(),
            saved.rotation.index,
            saved.rotation.shuffle,
            saved.rotation.repeat,
        );
        rotation.prune_missing();
        let lang = i18n::Lang::from_code(&saved.lang);
        let theme = theme::AppTheme::from_code(&saved.theme);
        Self {
            file: saved.last_path,
            status: i18n::tr(lang, "status_pick"),
            muted: saved.muted,
            volume: saved.volume,
            running: None,
            extra: vec![],
            sync_waiting: false,
            sync_deadline: std::time::Instant::now(),
            sync_resume: false,
            quit_requested: false,
            paused: false,
            monitor: saved.monitor,
            monitors: discover_monitors(lang),
            auto: wallmotion_core::autopause::AutoPause::new(
                saved.pause_on_fullscreen,
                saved.pause_on_battery,
            ),
            auto_paused: false,
            pause_on_fullscreen: saved.pause_on_fullscreen,
            pause_on_battery: saved.pause_on_battery,
            last_poll: std::time::Instant::now(),
            last_zcheck: std::time::Instant::now(),
            pending: vec![],
            pending_deadline: None,
            remote_rx: None,
            hotkeys: hotkeys::Hotkeys::start(),
            hotkeys_attached: false,
            show_hotkeys: false,
            pending_start: false,
            rotation_enabled: saved.rotation.enabled,
            rotation_interval: interval,
            last_rotation: std::time::Instant::now(),
            rot_quick_skips: 0,
            rotation,
            library: vec![],
            thumbs: Default::default(),
            lib_scanned: false,
            volumes: saved
                .volumes
                .into_iter()
                .map(|(k, v)| (k, wallmotion_core::volumememory::VolumeSetting::from(v)))
                .collect(),
            yt_url: String::new(),
            yt_status: String::new(),
            yt_progress: 0.0,
            yt_busy: false,
            yt_rx: None,
            yt_child: youtube::new_child_slot(),
            yt_cancel: youtube::new_cancel_flag(),
            yt_playlist: vec![],
            yt_picked: vec![],
            yt_queue: Default::default(),
            yt_queue_total: 0,
            yt_tools_scanned: false,
            ytdlp_path: None,
            ytdlp_ver: None,
            ffmpeg_path: None,
            ffmpeg_av1: None,
            yt_codec: wallmotion_core::youtube::VideoCodec::from_code(&saved.yt_codec),
            tool_rx: None,
            tool_busy: false,
            tools_auto_started: false,
            upd_rx: None,
            upd_busy: false,
            upd_quiet: true,
            upd_status: String::new(),
            upd_latest: None,
            upd_url: None,
            upd_dll_url: None,
            upd_ready: false,
            upd_auto_done: false,
            upd_checked_at: saved.update_checked_at,
            upd_skipped: saved.update_skipped.clone(),
            tab: Tab::Settings,
            favorites: saved.favorites.into_iter().collect(),
            lib_search: String::new(),
            lib_fav_only: false,
            lib_selected: None,
            lib_sort: LibSort::default(),
            lib_type: LibType::default(),
            lib_folder_notice: None,
            lang,
            theme,
            follow_system: saved.theme_follow_system,
            sys_theme: theme::read_system_theme(),
            applied_style: None,
            last_theme_poll: std::time::Instant::now(),
            logo: None,
            preview_path: String::new(),
            preview_tex: None,
            autostart: autostart::is_enabled(),
            start_hidden: false,
        }
    }
}

impl App {
    /// Apply a startup CLI command to this fresh instance. Mirrors the
    /// `startup_cmd` path in Python `main()`: preset file/mute/volume and
    /// auto-play `--set` on the first frame.
    fn apply_launch_command(&mut self, cmd: &serde_json::Value) {
        if let Some(path) = cmd.get("set").and_then(|v| v.as_str()) {
            if PathBuf::from(path).is_file() {
                self.file = path.to_string();
                self.pending_start = true;
            }
        }
        if let Some(muted) = cmd.get("muted").and_then(|v| v.as_bool()) {
            self.muted = muted;
        }
        if let Some(volume) = cmd.get("volume").and_then(|v| v.as_u64()) {
            self.volume = volume.min(100) as u8;
        }
        if cmd.get("set").is_some() || cmd.get("muted").is_some() || cmd.get("volume").is_some() {
            self.persist();
        }
    }

    /// Apply one remote CLI command (JSON line) to the running app.
    /// Order mirrors Python `handle_remote_command`: set, stop, mute, volume.
    fn apply_remote_command(&mut self, ctx: &egui::Context, line: &str) {
        let cmd: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return,
        };
        if !cmd.is_object() {
            return;
        }
        if let Some(path) = cmd.get("set").and_then(|v| v.as_str()) {
            if PathBuf::from(path).is_file() {
                self.file = path.to_string();
                self.set_wallpaper();
            }
        }
        if cmd.get("stop").and_then(|v| v.as_bool()).unwrap_or(false) {
            // Stopping halts the rotation too (no dead "enabled but idle").
            self.rotation_enabled = false;
            self.stop_video();
        }
        if let Some(muted) = cmd.get("muted").and_then(|v| v.as_bool()) {
            self.muted = muted;
            self.apply_mute_volume();
        }
        if let Some(volume) = cmd.get("volume").and_then(|v| v.as_u64()) {
            self.volume = volume.min(100) as u8;
            self.apply_mute_volume();
        }
        self.persist();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.request_repaint();
    }

    fn persist(&mut self) {
        // Remember current slider/mute for the current file first
        // (mirrors Python `_save_config` + `volumememory.remember`).
        if !self.file.trim().is_empty() {
            wallmotion_core::volumememory::remember(
                &mut self.volumes,
                self.file.trim(),
                self.volume,
                self.muted,
                wallmotion_core::volumememory::MAX_ENTRIES,
            );
        }
        AppConfig {
            last_path: self.file.clone(),
            muted: self.muted,
            volume: self.volume,
            monitor: self.monitor.clone(),
            pause_on_fullscreen: self.pause_on_fullscreen,
            pause_on_battery: self.pause_on_battery,
            rotation: config::RotationConfig {
                files: self.rotation.files().to_vec(),
                index: self.rotation.index(),
                shuffle: self.rotation.shuffle(),
                repeat: self.rotation.repeat(),
                enabled: self.rotation_enabled,
                interval: self.rotation_interval,
            },
            volumes: self
                .volumes
                .iter()
                .map(|(k, v)| (k.clone(), config::VolumeEntry::from(*v)))
                .collect(),
            favorites: {
                let mut favs: Vec<String> = self.favorites.iter().cloned().collect();
                favs.sort();
                favs
            },
            lang: self.lang.code().to_string(),
            theme: self.theme.code().to_string(),
            theme_follow_system: self.follow_system,
            update_checked_at: self.upd_checked_at,
            update_skipped: self.upd_skipped.clone(),
            yt_codec: self.yt_codec.code().to_string(),
        }
        .save();
    }

    /// Restore remembered volume for the current file, if any.
    /// Only the volume is per-file: mute stays exactly as the user set
    /// it (global). Recalling a stored mute re-muted videos behind the
    /// user's back on every switch, which is worse than no memory.
    /// The muted flag is still recorded (Python shares the config).
    fn apply_volume_memory(&mut self) {
        let path = self.file.trim().to_string();
        if path.is_empty() {
            return;
        }
        if let Some(entry) = wallmotion_core::volumememory::lookup(Some(&self.volumes), &path) {
            self.volume = entry.volume;
        }
    }

    /// Play a rotation item now: set file, apply, note the start time
    /// (image dwell + spawn-rate debounce measure from here).
    fn play_rotation_path(&mut self, path: String) {
        self.file = path;
        self.set_wallpaper();
        self.persist();
        self.last_rotation = std::time::Instant::now();
    }

    /// Move to the next rotation item. Same path back means the list
    /// ended (non-repeating) or holds a single file: rotation switches
    /// off and the file replays looped; `None` means every file vanished.
    fn advance_rotation(&mut self) {
        match self.rotation.next_file() {
            Some(path) if path != self.file => {
                debug_log(&format!("rotation advance -> {path}"));
                self.play_rotation_path(path);
            }
            Some(same) => {
                self.rotation_enabled = false;
                self.play_rotation_path(same);
            }
            None => {
                self.rotation_enabled = false;
                self.persist();
            }
        }
    }

    /// (Re)detect the yt-dlp/ffmpeg sidecars. Cheap fs checks; called
    /// once at startup and after every provision/update event.
    fn rescan_tools(&mut self) {
        self.ytdlp_path = youtube::find_ytdlp();
        self.ytdlp_ver = self.ytdlp_path.as_deref().and_then(youtube::ytdlp_version);
        self.ffmpeg_path = youtube::find_ffmpeg();
        // AV1 decode check is one `ffmpeg -decoders` run (~50 ms);
        // cached until the next rescan (install/update changes it).
        self.ffmpeg_av1 = self
            .ffmpeg_path
            .as_deref()
            .and_then(youtube::ffmpeg_supports_av1);
        self.yt_tools_scanned = true;
    }

    /// Start one download (or a playlist fetch) for the URL in the box.
    fn start_yt_url(&mut self, ctx: &egui::Context) {
        use wallmotion_core::youtube as yt;
        let url = self.yt_url.trim().to_string();
        if url.is_empty() {
            self.yt_status = i18n::tr(self.lang, "yt_url_first");
            return;
        }
        if !yt::is_valid_youtube_url(&url) {
            self.yt_status = i18n::tr(self.lang, "yt_invalid_url");
            return;
        }
        let Some(exe) = self.ytdlp_path.clone() else {
            // Auto-install still running? Tell the user to wait a moment.
            if self.tool_busy {
                self.yt_status = i18n::tr(self.lang, "yt_auto_setup");
            } else {
                self.yt_status = i18n::tr(self.lang, "yt_need_tool");
            }
            return;
        };
        if self.yt_busy {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.yt_rx = Some(rx);
        self.yt_busy = true;
        self.yt_progress = 0.0;
        self.yt_playlist.clear();
        self.yt_picked.clear();
        self.yt_cancel = youtube::new_cancel_flag();
        if yt::is_playlist_url(&url) {
            self.yt_status = i18n::tr(self.lang, "yt_playlist_loading");
            youtube::spawn_playlist_fetch(exe, url, self.yt_cancel.clone(), tx);
        } else {
            self.yt_queue.clear();
            self.yt_queue_total = 1;
            self.yt_status = self.yt_progress_line(0.0);
            youtube::spawn_download(
                exe,
                url,
                library::media_dir(),
                self.ffmpeg_path.clone(),
                self.yt_codec,
                self.yt_child.clone(),
                self.yt_cancel.clone(),
                tx,
            );
        }
        ctx.request_repaint();
    }

    /// One download status line (queue-aware, localized).
    fn yt_progress_line(&self, pct: f32) -> String {
        let p = format!("{pct:.0}%");
        if self.yt_queue_total > 1 {
            let done = self.yt_queue_total - self.yt_queue.len();
            format!(
                "{} {}",
                i18n::trf(
                    self.lang,
                    "yt_queue_progress",
                    &[
                        ("i", &done.to_string()),
                        ("n", &self.yt_queue_total.to_string())
                    ]
                ),
                i18n::trf(self.lang, "yt_downloading", &[("p", &p)])
            )
        } else {
            i18n::trf(self.lang, "yt_downloading", &[("p", &p)])
        }
    }

    /// Start the next queued playlist item, if any.
    fn start_next_queued(&mut self, ctx: &egui::Context) {
        let Some(url) = self.yt_queue.pop_front() else {
            self.yt_queue_total = 0;
            return;
        };
        let Some(exe) = self.ytdlp_path.clone() else {
            self.yt_status = i18n::tr(self.lang, "yt_need_tool");
            self.yt_busy = false;
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.yt_rx = Some(rx);
        self.yt_busy = true;
        self.yt_progress = 0.0;
        self.yt_cancel = youtube::new_cancel_flag();
        self.yt_status = self.yt_progress_line(0.0);
        youtube::spawn_download(
            exe,
            url,
            library::media_dir(),
            self.ffmpeg_path.clone(),
            self.yt_codec,
            self.yt_child.clone(),
            self.yt_cancel.clone(),
            tx,
        );
        ctx.request_repaint();
    }

    /// Drain YouTube worker events (progress, playlist, done, errors)
    /// plus background tool auto-install events. Missing sidecars are
    /// installed automatically once on launch so the user never has to
    /// click Get yt-dlp / Get ffmpeg by hand.
    fn poll_yt_events(&mut self, ctx: &egui::Context) {
        if !self.yt_tools_scanned {
            self.rescan_tools();
        }
        if !self.tools_auto_started {
            self.tools_auto_started = true;
            self.auto_install_missing_tools(ctx);
        }
        let events: Vec<youtube::YtEvent> = self
            .yt_rx
            .as_ref()
            .map(|rx| {
                let mut out = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    out.push(ev);
                }
                out
            })
            .unwrap_or_default();
        for ev in events {
            match ev {
                youtube::YtEvent::Progress(pct) => {
                    self.yt_progress = pct;
                    self.yt_status = self.yt_progress_line(pct);
                }
                youtube::YtEvent::Playlist(entries) => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_picked = vec![true; entries.len()];
                    self.yt_playlist = entries;
                    self.yt_status = i18n::tr(self.lang, "yt_pick");
                }
                youtube::YtEvent::Finished(path) => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_progress = 100.0;
                    self.yt_status = i18n::tr(self.lang, "yt_done");
                    self.file = path;
                    self.set_wallpaper();
                    self.persist();
                    self.rescan_library(ctx);
                    // Playlist queue: continue with the next item.
                    if !self.yt_queue.is_empty() {
                        self.start_next_queued(ctx);
                    } else {
                        self.yt_queue_total = 0;
                    }
                }
                youtube::YtEvent::Error(code) => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_status = youtube::error_text(self.lang, &code);
                    // Playlist queue: skip the broken video, keep going.
                    if !self.yt_queue.is_empty() {
                        self.start_next_queued(ctx);
                    } else {
                        self.yt_queue_total = 0;
                    }
                }
                youtube::YtEvent::Tool { key, arg } => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_status = i18n::trf(self.lang, key, &[("v", &arg)]);
                    self.rescan_tools();
                }
            }
        }
        // Background tool installs share one channel (both workers can
        // send to clones of the same tx); drain them here so yt_busy for
        // real downloads is never disturbed.
        let tool_events: Vec<youtube::YtEvent> = self
            .tool_rx
            .as_ref()
            .map(|rx| {
                let mut out = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    out.push(ev);
                }
                out
            })
            .unwrap_or_default();
        for ev in tool_events {
            match ev {
                youtube::YtEvent::Tool { key, arg } => {
                    self.rescan_tools();
                    self.yt_status = i18n::trf(self.lang, key, &[("v", &arg)]);
                    if self.ytdlp_path.is_some() && self.ffmpeg_path.is_some() {
                        self.tool_busy = false;
                        self.tool_rx = None;
                    } else if key == "yt_ready" || key == "yt_updated" {
                        // yt-dlp done, ffmpeg may still be running.
                        if self.ytdlp_path.is_some() && self.tool_rx.is_some() {
                            self.yt_status = i18n::tr(self.lang, "yt_dling_ffmpeg");
                        }
                    }
                    if self.ytdlp_path.is_some() && self.ffmpeg_path.is_some() {
                        // Both ready — leave the success line visible.
                    }
                }
                youtube::YtEvent::Error(code) => {
                    self.yt_status = youtube::error_text(self.lang, &code);
                    self.tool_busy = false;
                    self.tool_rx = None;
                }
                // Downloads never run on the tool channel; ignore the rest.
                _ => {}
            }
        }
        if self.tool_rx.is_some() {
            ctx.request_repaint();
        }
    }

    /// Start background install of every missing sidecar (yt-dlp, ffmpeg).
    /// Called once on launch; manual Get buttons reuse the same channel.
    fn auto_install_missing_tools(&mut self, ctx: &egui::Context) {
        let need_ytdlp = self.ytdlp_path.is_none();
        let need_ffmpeg = self.ffmpeg_path.is_none();
        if !need_ytdlp && !need_ffmpeg {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.tool_rx = Some(rx);
        self.tool_busy = true;
        if need_ytdlp {
            youtube::spawn_provision_ytdlp(tx.clone());
        }
        if need_ffmpeg {
            youtube::spawn_provision_ffmpeg(tx);
        }
        // Only overwrite an empty status — never hide a real message.
        if self.yt_status.is_empty() {
            self.yt_status = i18n::tr(self.lang, "yt_auto_setup");
        }
        ctx.request_repaint();
    }

    /// Manual (or re-try) install of one sidecar on the tool channel.
    fn start_tool_install(&mut self, ctx: &egui::Context, tool: &str) {
        let (tx, rx) = if self.tool_rx.is_some() {
            // Already installing — reuse is impossible (rx taken), so
            // just ignore extra clicks while busy.
            return;
        } else {
            std::sync::mpsc::channel()
        };
        self.tool_rx = Some(rx);
        self.tool_busy = true;
        if tool == "ffmpeg" {
            self.yt_status = i18n::tr(self.lang, "yt_dling_ffmpeg");
            youtube::spawn_provision_ffmpeg(tx);
        } else {
            self.yt_status = i18n::tr(self.lang, "yt_dling_ytdlp");
            youtube::spawn_provision_ytdlp(tx);
        }
        ctx.request_repaint();
    }

    /// Rescan the downloads folder and (re)build thumbnails. Images load
    /// directly; videos need ffmpeg on PATH (else a text badge is shown).
    fn rescan_library(&mut self, ctx: &egui::Context) {
        let dir = library::media_dir();
        // Fresh install: the folder may not exist yet — create it so the
        // user (and "Open videos folder") always has somewhere to drop files.
        let fresh = !dir.is_dir();
        let _ = std::fs::create_dir_all(&dir);
        if fresh && dir.is_dir() {
            self.lib_folder_notice = Some(dir.clone());
        }
        self.library = library::list_media(&dir);
        self.thumbs.clear();
        let cache = library::thumbs_dir();
        for item in &self.library {
            let full = dir.join(&item.name);
            let source = match item.kind {
                library::MediaKind::Image => Some(full),
                library::MediaKind::Video => library::ensure_video_thumb(&full, &cache),
            };
            if let Some(path) = source {
                if let Some((rgba, w, h)) = library::load_thumb_rgba(&path) {
                    let img =
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                    self.thumbs.insert(
                        item.name.clone(),
                        ctx.load_texture(&item.name, img, egui::TextureOptions::LINEAR),
                    );
                }
            }
        }
        self.lib_scanned = true;
    }

    /// Explorer drag & drop: files dropped onto the window are copied
    /// into the library folder (media only, never overwriting), then the
    /// gallery refreshes and the Library tab opens so the result shows.
    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let sources: Vec<PathBuf> = ctx
            .input(|i| i.raw.dropped_files.clone())
            .into_iter()
            .filter_map(|f| f.path)
            .collect();
        if sources.is_empty() {
            return;
        }
        let report = library::import_files(&library::media_dir(), &sources);
        if report.added > 0 {
            self.tab = Tab::Library;
            self.lib_selected = None;
            self.rescan_library(ctx);
            self.status = i18n::trf(
                self.lang,
                "lib_drop_added",
                &[("n", &report.added.to_string())],
            );
        } else if report.failed > 0 {
            self.status = i18n::tr(self.lang, "lib_drop_failed");
        } else {
            self.status = i18n::tr(self.lang, "lib_drop_skipped");
        }
    }

    /// Full-window hint while a file hovers over the window (drag & drop).
    fn draw_drop_hint(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("wallmotion-drop-hint"),
        ));
        let rect = ctx.screen_rect();
        painter.rect_filled(
            rect,
            egui::CornerRadius::ZERO,
            egui::Color32::from_black_alpha(150),
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            i18n::tr(self.lang, "lib_drop_hint"),
            egui::FontId::proportional(20.0),
            egui::Color32::WHITE,
        );
    }

    /// Floating cheat-sheet with the global shortcuts, opened from the
    /// header button. Escape or the X closes it.
    fn hotkeys_window(&mut self, ctx: &egui::Context) {
        if !self.show_hotkeys {
            return;
        }
        let mut open = true;
        egui::Window::new(i18n::tr(self.lang, "hotkeys_title"))
            .id(egui::Id::new("wallmotion-hotkeys-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let rows = [
                    ("Ctrl + Alt + Right", "hotkeys_action_next"),
                    ("Ctrl + Alt + Left", "hotkeys_action_prev"),
                    ("Ctrl + Alt + Space", "hotkeys_action_pause"),
                    ("Ctrl + Alt + M", "hotkeys_action_mute"),
                ];
                ui.add_space(2.0);
                egui::Grid::new("wallmotion-hotkeys-grid")
                    .num_columns(2)
                    .spacing([18.0, 8.0])
                    .show(ui, |ui| {
                        for (combo, key) in rows {
                            ui.label(egui::RichText::new(combo).strong().monospace());
                            ui.label(i18n::tr(self.lang, key));
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr(self.lang, "hotkeys_works_hidden")).weak());
                if self.hotkeys.registered() == 0 {
                    ui.label(
                        egui::RichText::new(i18n::tr(self.lang, "hotkeys_failed"))
                            .color(egui::Color32::from_rgb(0xE0, 0x7A, 0x5F)),
                    );
                }
                ui.add_space(8.0);
                if ui.button(i18n::tr(self.lang, "hotkeys_close")).clicked() {
                    self.show_hotkeys = false;
                }
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_hotkeys = false;
        }
    }

    /// One rotation tick. Timer-driven (videos loop, so unlike Python
    /// there is no follow-video-end mode).
    /// Advance the rotation: videos switch when the file finishes
    /// playing (rotation mode spawns mpv without `--loop`, so its exit
    /// *is* the end-of-file signal); images dwell on the saved interval.
    /// Broken files are skipped, a full cycle of them switches rotation
    /// off. At most one advance per 500 ms (no spawn machine-gun).
    /// Drain the in-process player event queues (non-blocking, every
    /// UI tick): first-frame marks for the sync release, end-of-file
    /// marks for rotation. Then release frozen starters together once
    /// every runner painted (or the 3 s deadline): frozen starts can't
    /// skew, the event itself is the signal — no brightness probe.
    fn poll_player_events(&mut self) {
        use wallmotion_mpv::LibEvent;
        for run in self
            .running
            .iter_mut()
            .chain(self.extra.iter_mut())
            .chain(self.pending.iter_mut())
        {
            loop {
                match run.player.wait_event(0) {
                    Some(LibEvent::PlaybackRestart) => run.painted = true,
                    Some(LibEvent::FileLoaded) => {}
                    Some(LibEvent::EndOfFile) => {
                        if run.eof.is_none() {
                            run.eof = Some(true);
                            debug_log("mpv event: EndOfFile");
                        }
                    }
                    Some(LibEvent::FileError) => {
                        if run.eof.is_none() {
                            run.eof = Some(false);
                            debug_log("mpv event: FileError");
                        }
                    }
                    Some(LibEvent::Shutdown) => {}
                    None => break,
                }
            }
        }
        // Event fallback: with keep-open the player idles on the last
        // frame after EOF; if the EndFile event got lost, `eof-reached`
        // stays true while `eof` is still None and rotation would freeze
        // forever. Property is ground truth — adopt it (live runners
        // only, never the hidden warmups).
        for run in self.running.iter_mut().chain(self.extra.iter_mut()) {
            if run.eof.is_none() && run.player.prop_bool("eof-reached") == Some(true) {
                run.eof = Some(true);
                debug_log("mpv eof-reached property fallback");
            }
        }
        if self.sync_waiting {
            let all_painted = self
                .running
                .iter()
                .chain(self.extra.iter())
                .all(|run| run.painted);
            if all_painted || std::time::Instant::now() >= self.sync_deadline {
                self.sync_waiting = false;
                if self.sync_resume {
                    if let Some(run) = &self.running {
                        let _ = run.player.set_paused(false);
                    }
                    for run in &self.extra {
                        let _ = run.player.set_paused(false);
                    }
                }
            }
        }
    }

    fn rotation_tick(&mut self) {
        if !self.rotation_enabled || self.rotation.is_empty() {
            return;
        }
        if is_image_path(&self.file) {
            if self.last_rotation.elapsed()
                < std::time::Duration::from_secs(self.rotation_interval.max(1))
            {
                return;
            }
            self.advance_rotation();
            return;
        }
        // Video: an EndOfFile mark on ANY monitor = clean end (advance)
        // or broken file (skip). Still playing/paused or never
        // spawned: wait.
        let exit_ok: Option<bool> = self
            .running
            .as_ref()
            .and_then(|run| run.eof)
            .or_else(|| self.extra.iter().find_map(|run| run.eof));
        let ok = match exit_ok {
            Some(ok) => ok,
            None => return,
        };
        debug_log(&format!("rotation eof ok={ok} file={}", self.file));
        if self.last_rotation.elapsed() < std::time::Duration::from_millis(500) {
            return;
        }
        if ok {
            self.rot_quick_skips = 0;
            self.advance_rotation();
        } else if self.rot_quick_skips >= self.rotation.len().max(1) as u32 {
            self.rotation_enabled = false;
            self.persist();
        } else {
            self.rot_quick_skips += 1;
            self.advance_rotation();
        }
    }

    /// (Re)build the settings preview texture when the file changes.
    /// Images load directly; videos use the ffmpeg frame cache (same as
    /// the library grid). Cheap guard: returns immediately otherwise.
    fn ensure_preview(&mut self, ctx: &egui::Context) {
        let path = self.file.trim().to_string();
        if self.preview_path == path {
            return;
        }
        self.preview_path = path.clone();
        self.preview_tex = None;
        if path.is_empty() {
            return;
        }
        let full = PathBuf::from(&path);
        if !full.is_file() {
            return;
        }
        let lower = path.to_lowercase();
        let is_video = library::VIDEO_EXTS.iter().any(|e| lower.ends_with(e));
        let source: Option<PathBuf> = if is_video {
            library::ensure_video_thumb(&full, &library::thumbs_dir())
        } else {
            Some(full)
        };
        if let Some(src) = source {
            if let Some((rgba, w, h)) = library::load_thumb_rgba(&src) {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                self.preview_tex =
                    Some(ctx.load_texture("preview", img, egui::TextureOptions::LINEAR));
            }
        }
    }

    /// Settings control cards (source, sound, rotation, YouTube) shared
    /// by the wide two-column layout (left column) and the narrow
    /// stacked layout.
    fn settings_controls(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dark = self.applied_style.unwrap_or(true);
        settings_card(ui, dark, |ui| {
            card_title(ui, i18n::tr(self.lang, "source_title"));
            ui.horizontal(|ui| {
                if ui.button(i18n::tr(self.lang, "browse")).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter(
                            "images & video",
                            &[
                                "jpg", "jpeg", "png", "bmp", "gif", "mp4", "mkv", "webm", "avi",
                                "mov",
                            ],
                        )
                        .pick_file()
                    {
                        self.file = path.to_string_lossy().into_owned();
                        self.apply_volume_memory();
                        self.persist();
                    }
                }
                let name = display_file_name(&self.file);
                if name.is_empty() {
                    ui.label(egui::RichText::new(i18n::tr(self.lang, "file_hint")).weak());
                } else {
                    ui.label(egui::RichText::new(truncate_middle(&name, 48)).strong())
                        .on_hover_text(self.file.clone());
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(i18n::tr(self.lang, "monitor_label"));
                let current = if self.monitor.is_empty() {
                    i18n::tr(self.lang, "monitor_all")
                } else {
                    self.monitors
                        .iter()
                        .find(|m| m.name == self.monitor)
                        .map(|m| m.label.clone())
                        .unwrap_or_else(|| self.monitor.clone())
                };
                egui::ComboBox::from_id_salt("monitor")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(
                                &mut self.monitor,
                                String::new(),
                                i18n::tr(self.lang, "monitor_all"),
                            )
                            .changed()
                        {
                            self.persist();
                        }
                        for m in self.monitors.clone() {
                            if ui
                                .selectable_value(&mut self.monitor, m.name.clone(), &m.label)
                                .changed()
                            {
                                self.persist();
                            }
                        }
                    });
                if ui.button(i18n::tr(self.lang, "refresh")).clicked() {
                    self.monitors = discover_monitors(self.lang);
                }
            });
            #[cfg(windows)]
            {
                let mut auto = self.autostart;
                if ui
                    .checkbox(&mut auto, i18n::tr(self.lang, "autostart"))
                    .on_hover_text(i18n::tr(self.lang, "autostart_tip"))
                    .changed()
                {
                    match autostart::set_enabled(auto) {
                        Ok(()) => self.autostart = auto,
                        Err(e) => {
                            self.status = i18n::trf(self.lang, "autostart_err", &[("e", &e)]);
                        }
                    }
                }
            }
        }); // source card
        ui.add_space(8.0);
        settings_card(ui, dark, |ui| {
            egui::CollapsingHeader::new(i18n::tr(self.lang, "sound_title"))
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut self.muted, i18n::tr(self.lang, "mute_short"))
                            .changed()
                        {
                            self.apply_mute_volume();
                            self.persist();
                        }
                        ui.label(i18n::tr(self.lang, "volume_label"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("{}%", self.volume));
                        });
                    });
                    let w = ui.available_width().max(80.0);
                    if ui
                        .add_sized(
                            egui::vec2(w, 0.0),
                            egui::Slider::new(&mut self.volume, 0..=100).show_value(false),
                        )
                        .changed()
                    {
                        self.apply_mute_volume();
                        self.persist();
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .checkbox(
                                &mut self.pause_on_fullscreen,
                                i18n::tr(self.lang, "pause_fullscreen"),
                            )
                            .changed()
                        {
                            self.auto
                                .set_rules(self.pause_on_fullscreen, self.pause_on_battery);
                            self.refresh_autopause_after_rule_change();
                            self.persist();
                        }
                        if ui
                            .checkbox(
                                &mut self.pause_on_battery,
                                i18n::tr(self.lang, "pause_battery"),
                            )
                            .changed()
                        {
                            self.auto
                                .set_rules(self.pause_on_fullscreen, self.pause_on_battery);
                            self.refresh_autopause_after_rule_change();
                            self.persist();
                        }
                    });
                });
        }); // sound card
        ui.add_space(8.0);
        settings_card(ui, dark, |ui| {
            egui::CollapsingHeader::new(i18n::tr(self.lang, "rotation_title"))
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .checkbox(
                                &mut self.rotation_enabled,
                                i18n::tr(self.lang, "rotation_enable"),
                            )
                            .changed()
                        {
                            self.last_rotation = std::time::Instant::now();
                            if self.rotation_enabled {
                                // Continue after the currently playing
                                // file: manual picks don't move the
                                // cursor, so it may sit at the end (with
                                // repeat off rotation would then stop at
                                // the first EOF instead of advancing).
                                let cur = self.file.trim().to_string();
                                self.rotation.snap_to(&cur);
                            }
                            // The mpv loop flag is chosen at spawn: re-spawn
                            // a running video so the new mode applies
                            // (looped single vs. advancing playlist).
                            if self.any_running() && !is_image_path(&self.file) {
                                self.set_wallpaper();
                            }
                            self.persist();
                        }
                        let mut shuffle = self.rotation.shuffle();
                        if ui
                            .checkbox(&mut shuffle, i18n::tr(self.lang, "rotation_shuffle"))
                            .changed()
                        {
                            self.rotation.set_shuffle(shuffle);
                            self.persist();
                        }
                        let mut repeat = self.rotation.repeat();
                        if ui
                            .checkbox(&mut repeat, i18n::tr(self.lang, "rotation_repeat"))
                            .changed()
                        {
                            self.rotation.set_repeat(repeat);
                            self.persist();
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(i18n::tr(self.lang, "rotation_add")).clicked()
                            && PathBuf::from(self.file.trim()).is_file()
                        {
                            self.rotation.add(self.file.trim());
                            self.persist();
                        }
                        if ui.button(i18n::tr(self.lang, "rotation_play")).clicked() {
                            self.rotation_enabled = true;
                            if let Some(path) = self.rotation.restart() {
                                self.play_rotation_path(path);
                            } else {
                                self.persist();
                            }
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.add_enabled_ui(!self.rotation.is_empty(), |ui| {
                            if ui.button(i18n::tr(self.lang, "rotation_skip")).clicked() {
                                if let Some(path) = self.rotation.next_file() {
                                    self.play_rotation_path(path);
                                }
                            }
                        });
                        if ui.button(i18n::tr(self.lang, "rotation_clear")).clicked() {
                            self.rotation.clear();
                            self.rotation_enabled = false;
                            self.persist();
                        }
                    });
                    ui.label(
                        egui::RichText::new(i18n::trf(
                            self.lang,
                            "rotation_count",
                            &[("n", &self.rotation.len().to_string())],
                        ))
                        .strong(),
                    );
                    // GIFs have no timeline: mpv may loop one forever
                    // without an end signal, stalling the rotation on it.
                    if self
                        .rotation
                        .files()
                        .iter()
                        .any(|f| f.to_lowercase().ends_with(".gif"))
                    {
                        ui.label(
                            egui::RichText::new(i18n::tr(self.lang, "rotation_gif_warn")).weak(),
                        );
                    }
                });
        }); // rotation card
        ui.add_space(8.0);
        self.youtube_card(ui, ctx);
        #[cfg(windows)]
        {
            ui.add_space(8.0);
            self.update_section(ui, ctx);
        }
    }

    /// YouTube download card (under the preview in wide mode, last in
    /// narrow mode) — balances the columns.
    fn youtube_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dark = self.applied_style.unwrap_or(true);
        settings_card(ui, dark, |ui| {
            egui::CollapsingHeader::new(i18n::tr(self.lang, "yt_title"))
                .default_open(true)
                .show(ui, |ui| {
                    self.youtube_section(ui, ctx);
                });
        });
    }

    /// Drain in-app updater events (check results, download done, errors).
    fn poll_update_events(&mut self) {
        let events: Vec<update::UpdateEvent> = self
            .upd_rx
            .as_ref()
            .map(|rx| {
                let mut out = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    out.push(ev);
                }
                out
            })
            .unwrap_or_default();
        for ev in events {
            match ev {
                update::UpdateEvent::Checked {
                    latest,
                    url,
                    dll_url,
                } => {
                    self.upd_busy = false;
                    self.upd_rx = None;
                    self.upd_checked_at = update::now_secs();
                    self.persist();
                    match url {
                        Some(u) if latest != self.upd_skipped => {
                            self.upd_status =
                                i18n::trf(self.lang, "upd_available", &[("v", &latest)]);
                            self.upd_latest = Some(latest);
                            self.upd_url = Some(u);
                            self.upd_dll_url = dll_url;
                        }
                        _ => {
                            self.upd_latest = None;
                            self.upd_url = None;
                            self.upd_dll_url = None;
                            if !self.upd_quiet {
                                self.upd_status = i18n::tr(self.lang, "upd_current");
                            }
                        }
                    }
                }
                update::UpdateEvent::Downloaded => {
                    self.upd_busy = false;
                    self.upd_rx = None;
                    // Swap the build into place now; the restart below
                    // only flips to the already-installed file.
                    match update::install_update() {
                        Ok(()) => {
                            self.upd_ready = true;
                            self.upd_status = i18n::tr(self.lang, "upd_ready");
                        }
                        Err(e) => {
                            self.upd_status = e;
                        }
                    }
                }
                update::UpdateEvent::Error(e) => {
                    self.upd_busy = false;
                    self.upd_rx = None;
                    if !self.upd_quiet {
                        self.upd_status = e;
                    }
                }
            }
        }
    }

    /// Start a release check (quiet = silent auto-check, else the button).
    #[cfg(windows)]
    fn start_update_check(&mut self, quiet: bool) {
        if self.upd_busy {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.upd_rx = Some(rx);
        self.upd_busy = true;
        self.upd_quiet = quiet;
        if !quiet {
            self.upd_status = i18n::tr(self.lang, "upd_checking");
        }
        update::spawn_check(tx);
    }

    /// Updater card: current version, check/download/restart actions.
    /// Windows only (an AppImage cannot swap itself the same way).
    #[cfg(windows)]
    fn update_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dark = self.applied_style.unwrap_or(true);
        settings_card(ui, dark, |ui| {
            egui::CollapsingHeader::new(i18n::tr(self.lang, "upd_title"))
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("v{}", update::current_version()))
                            .small()
                            .weak(),
                    );
                    if self.upd_busy {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            if !self.upd_status.is_empty() {
                                ui.label(&self.upd_status);
                            }
                        });
                    } else if self.upd_ready {
                        ui.label(&self.upd_status);
                        if big_accent_button(ui, dark, i18n::tr(self.lang, "upd_restart")).clicked()
                        {
                            match update::spawn_restart() {
                                Ok(()) => {
                                    TRAY_QUIT.store(true, std::sync::atomic::Ordering::SeqCst);
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                                Err(e) => {
                                    self.upd_status = e;
                                }
                            }
                        }
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(i18n::tr(self.lang, "upd_check")).clicked() {
                                self.start_update_check(false);
                            }
                            if self.upd_url.is_some()
                                && ui.button(i18n::tr(self.lang, "upd_download")).clicked()
                            {
                                if let Some(url) = self.upd_url.clone() {
                                    let (tx, rx) = std::sync::mpsc::channel();
                                    self.upd_rx = Some(rx);
                                    self.upd_busy = true;
                                    self.upd_quiet = false;
                                    self.upd_status = i18n::tr(self.lang, "upd_downloading");
                                    let dll_url = self.upd_dll_url.clone();
                                    let fetch_dll = update::dll_missing();
                                    update::spawn_download(tx, url, dll_url, fetch_dll);
                                    ctx.request_repaint();
                                }
                            }
                            if self.upd_latest.is_some()
                                && ui.button(i18n::tr(self.lang, "upd_skip")).clicked()
                            {
                                self.upd_skipped = self.upd_latest.take().unwrap_or_default();
                                self.upd_url = None;
                                self.upd_dll_url = None;
                                self.upd_status.clear();
                                self.persist();
                            }
                        });
                        if !self.upd_status.is_empty() {
                            ui.label(&self.upd_status);
                        }
                    }
                });
        });
    }

    /// Preview card: live thumbnail of the current wallpaper, file name +
    /// play state, and the main Set / Pause / Stop actions (Lively-style).
    fn preview_card(&mut self, ui: &mut egui::Ui) {
        card_title(ui, i18n::tr(self.lang, "preview_title"));
        let thumb: Option<(egui::TextureId, f32, f32)> = self.preview_tex.as_ref().map(|tex| {
            let s = tex.size_vec2();
            (tex.id(), s.x, s.y)
        });
        if let Some((id, tw, th)) = thumb {
            let avail = ui.available_width().max(80.0);
            let mut w = avail.min(480.0);
            let mut h = w * th / tw.max(1.0);
            if h > 220.0 {
                h = 220.0;
                w = h * tw / th.max(1.0);
            }
            ui.horizontal(|ui| {
                ui.add_space(((avail - w) / 2.0).max(0.0));
                ui.image((id, egui::vec2(w, h)));
            });
        } else {
            ui.vertical_centered(|ui| {
                ui.add_space(28.0);
                ui.label(egui::RichText::new(i18n::tr(self.lang, "file_hint")).weak());
                ui.add_space(28.0);
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let name = display_file_name(&self.file);
            if !name.is_empty() {
                ui.label(egui::RichText::new(truncate_middle(&name, 44)).strong())
                    .on_hover_text(self.file.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let state = if self.any_running() {
                    if self.paused {
                        i18n::tr(self.lang, "status_paused")
                    } else {
                        i18n::tr(self.lang, "status_playing")
                    }
                } else {
                    i18n::tr(self.lang, "status_stopped")
                };
                ui.label(egui::RichText::new(state).small().weak());
            });
        });
        ui.add_space(4.0);
        let dark = self.applied_style.unwrap_or(true);
        if big_accent_button(ui, dark, i18n::tr(self.lang, "apply")).clicked() {
            self.set_wallpaper();
        }
        ui.horizontal_wrapped(|ui| {
            let pause_label = if self.paused {
                i18n::tr(self.lang, "video_resume")
            } else {
                i18n::tr(self.lang, "video_pause")
            };
            ui.add_enabled_ui(self.any_running(), |ui| {
                if ui.button(pause_label).clicked() {
                    self.toggle_pause();
                }
            });
            if ui.button(i18n::tr(self.lang, "stop")).clicked() {
                // Stopping halts the rotation too (no dead "enabled but idle").
                self.rotation_enabled = false;
                self.stop_video();
                self.persist();
            }
        });
    }

    /// YouTube section: URL box, tool provisioning, progress, picker.
    fn youtube_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let dark = self.applied_style.unwrap_or(true);
        let url_entered = ui
            .add(
                egui::TextEdit::singleline(&mut self.yt_url)
                    .hint_text(i18n::tr(self.lang, "yt_placeholder"))
                    .desired_width(f32::INFINITY),
            )
            .lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(4.0);
        // Download codec: H.264 everywhere, AV1 = smaller files but
        // heavier decode. Warn when ffmpeg has no AV1 decoder.
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr(self.lang, "yt_codec"));
            let mut codec = self.yt_codec;
            egui::ComboBox::from_id_salt("yt_codec")
                .selected_text(i18n::tr(
                    self.lang,
                    match codec {
                        wallmotion_core::youtube::VideoCodec::Av1 => "yt_codec_av1",
                        wallmotion_core::youtube::VideoCodec::H264 => "yt_codec_h264",
                    },
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut codec,
                        wallmotion_core::youtube::VideoCodec::H264,
                        i18n::tr(self.lang, "yt_codec_h264"),
                    );
                    ui.selectable_value(
                        &mut codec,
                        wallmotion_core::youtube::VideoCodec::Av1,
                        i18n::tr(self.lang, "yt_codec_av1"),
                    );
                });
            if codec != self.yt_codec {
                self.yt_codec = codec;
                self.persist();
            }
            if codec == wallmotion_core::youtube::VideoCodec::Av1 && self.ffmpeg_av1 == Some(false)
            {
                ui.label(egui::RichText::new(i18n::tr(self.lang, "yt_av1_soft")).weak());
            }
        });
        ui.add_space(4.0);
        if self.yt_busy {
            if ui
                .add_sized(
                    egui::vec2(ui.available_width().max(60.0), 32.0),
                    egui::Button::new(
                        egui::RichText::new(i18n::tr(self.lang, "dl_cancel")).size(15.0),
                    ),
                )
                .clicked()
            {
                youtube::cancel_child(&self.yt_child, &self.yt_cancel);
                self.yt_busy = false;
                self.yt_rx = None;
                self.yt_queue.clear();
                self.yt_queue_total = 0;
                self.yt_status = i18n::tr(self.lang, "yt_cancelled");
            }
        } else {
            let dl = big_accent_button(ui, dark, i18n::tr(self.lang, "dl_download"));
            if dl.clicked() || url_entered {
                self.start_yt_url(ctx);
            }
        }
        if self.yt_busy && self.yt_progress > 0.0 {
            ui.add(
                egui::ProgressBar::new((self.yt_progress / 100.0).clamp(0.0, 1.0))
                    .show_percentage(),
            );
        }
        if !self.yt_status.is_empty() {
            ui.label(&self.yt_status);
        }
        // Sidecar tools: auto-installed on launch (see poll_yt_events);
        // buttons below are only a manual retry / update.
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            match (&self.ytdlp_path, &self.ytdlp_ver) {
                (Some(_), Some(v)) => {
                    ui.label(egui::RichText::new(format!("yt-dlp {v}")).weak());
                }
                (Some(_), None) => {
                    ui.label(egui::RichText::new(i18n::tr(self.lang, "tool_ytdlp_found")).weak());
                }
                (None, _) => {
                    if self.tool_busy {
                        ui.spinner();
                        ui.label(i18n::tr(self.lang, "yt_dling_ytdlp"));
                    } else {
                        ui.label(i18n::tr(self.lang, "tool_ytdlp_missing"));
                    }
                }
            }
            if self.tool_rx.is_none() {
                if self.ytdlp_path.is_none() {
                    if ui.button(i18n::tr(self.lang, "btn_get_ytdlp")).clicked() {
                        self.start_tool_install(ctx, "ytdlp");
                    }
                } else if ui.button(i18n::tr(self.lang, "btn_update")).clicked() {
                    if let Some(exe) = self.ytdlp_path.clone() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.tool_rx = Some(rx);
                        self.tool_busy = true;
                        self.yt_status = i18n::tr(self.lang, "yt_updating");
                        youtube::spawn_ytdlp_update(exe, tx);
                        ctx.request_repaint();
                    }
                }
            }
            if self.ffmpeg_path.is_some() {
                ui.label(egui::RichText::new(i18n::tr(self.lang, "tool_ffmpeg_ok")).weak());
            } else {
                if self.tool_busy {
                    ui.spinner();
                }
                ui.label(i18n::tr(self.lang, "tool_ffmpeg_missing"));
                if self.tool_rx.is_none()
                    && ui.button(i18n::tr(self.lang, "btn_get_ffmpeg")).clicked()
                {
                    self.start_tool_install(ctx, "ffmpeg");
                }
            }
        });
        // Playlist picker (titles fetched, nothing downloaded yet).
        if !self.yt_playlist.is_empty() {
            let titles: Vec<String> = self.yt_playlist.iter().map(|e| e.title.clone()).collect();
            ui.label(i18n::trf(
                self.lang,
                "pl_title",
                &[("n", &titles.len().to_string())],
            ));
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for (i, title) in titles.iter().enumerate() {
                        ui.checkbox(&mut self.yt_picked[i], title);
                    }
                });
            ui.horizontal_wrapped(|ui| {
                if accent_button(
                    ui,
                    self.applied_style.unwrap_or(true),
                    i18n::tr(self.lang, "dl_selected"),
                )
                .clicked()
                {
                    let urls: Vec<String> = self
                        .yt_playlist
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.yt_picked.get(*i).copied().unwrap_or(false))
                        .map(|(_, e)| e.url.clone())
                        .collect();
                    if urls.is_empty() {
                        self.yt_status = i18n::tr(self.lang, "yt_nothing");
                    } else {
                        self.yt_queue = urls.into_iter().collect();
                        self.yt_queue_total = self.yt_queue.len();
                        self.yt_playlist.clear();
                        self.yt_picked.clear();
                        self.start_next_queued(ctx);
                    }
                }
                if ui.button(i18n::tr(self.lang, "dl_clear")).clicked() {
                    self.yt_playlist.clear();
                    self.yt_picked.clear();
                    self.yt_status.clear();
                }
            });
        }
    }

    /// Wallpaper library tab: search + favorites filter, thumbnail grid
    /// on the left, detail panel (preview, actions) on the right.
    /// Lively-style layout (replaces the old inline row list).
    /// Library grid (search + cards). Left column in wide mode, top in
    /// narrow mode. Returns the click action, if any.
    fn library_grid(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<LibAction> {
        // One-time notice: the wallpaper folder was just created.
        if self.lib_folder_notice.is_some() {
            let path = self.lib_folder_notice.clone().unwrap_or_default();
            let full = path.to_string_lossy().into_owned();
            let msg = i18n::trf(i18n::Lang::En, "lib_folder_created", &[("p", &full)]);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(msg).strong())
                    .on_hover_text(&full);
                if ui.small_button("OK").clicked() {
                    self.lib_folder_notice = None;
                }
            });
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.lib_search)
                .hint_text(i18n::tr(self.lang, "library_search"))
                .desired_width(f32::INFINITY),
        );
        // Buttons on their own wrapped row: never pushed off-screen.
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::tr(self.lang, "sort_label"));
            egui::ComboBox::from_id_salt("lib_sort")
                .selected_text(i18n::tr(self.lang, self.lib_sort.key()))
                .show_ui(ui, |ui| {
                    for mode in [LibSort::Name, LibSort::Newest, LibSort::Oldest] {
                        ui.selectable_value(
                            &mut self.lib_sort,
                            mode,
                            i18n::tr(self.lang, mode.key()),
                        );
                    }
                });
            ui.label(i18n::tr(self.lang, "type_label"));
            egui::ComboBox::from_id_salt("lib_type")
                .selected_text(i18n::tr(self.lang, self.lib_type.key()))
                .show_ui(ui, |ui| {
                    for kind in [LibType::All, LibType::Video, LibType::Image] {
                        ui.selectable_value(
                            &mut self.lib_type,
                            kind,
                            i18n::tr(self.lang, kind.key()),
                        );
                    }
                });
            ui.checkbox(
                &mut self.lib_fav_only,
                i18n::tr(self.lang, "library_fav_only"),
            );
            if ui.button(i18n::tr(self.lang, "refresh")).clicked() {
                self.rescan_library(ctx);
            }
            if ui.button(i18n::tr(self.lang, "open_folder")).clicked() {
                open_folder(&library::media_dir());
            }
        });
        {
            let dir = library::media_dir();
            let folder = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.to_string_lossy().into_owned());
            ui.label(egui::RichText::new(format!(
                "{} · {}",
                i18n::trf(
                    self.lang,
                    "library_count",
                    &[("n", &self.library.len().to_string())]
                ),
                truncate_middle(&folder, 40)
            )))
            .on_hover_text(dir.to_string_lossy().into_owned());
        }
        let query = self.lib_search.trim().to_lowercase();
        let want_video = !matches!(self.lib_type, LibType::Image);
        let want_image = !matches!(self.lib_type, LibType::Video);
        let mut items: Vec<library::MediaItem> = self
            .library
            .iter()
            .filter(|it| {
                if self.lib_fav_only && !self.favorites.contains(&it.name) {
                    return false;
                }
                match it.kind {
                    library::MediaKind::Video => {
                        if !want_video {
                            return false;
                        }
                    }
                    library::MediaKind::Image => {
                        if !want_image {
                            return false;
                        }
                    }
                }
                query.is_empty() || it.name.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        match self.lib_sort {
            LibSort::Name => items.sort_by_key(|a| a.name.to_lowercase()),
            LibSort::Newest => items.sort_by(|a, b| {
                b.modified
                    .cmp(&a.modified)
                    .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
            LibSort::Oldest => items.sort_by(|a, b| {
                a.modified
                    .cmp(&b.modified)
                    .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }),
        }
        let mut action: Option<LibAction> = None;
        // Responsive grid: columns follow the column width, rows flow down.
        let dark = self.applied_style.unwrap_or(true);
        let avail = ui.available_width().max(100.0);
        let card_w = avail.clamp(140.0, 200.0);
        let ncols = ((avail / card_w).floor() as usize).max(1);
        egui::Grid::new("lib_grid")
            .num_columns(ncols)
            .spacing(egui::vec2(8.0, 8.0))
            .show(ui, |ui| {
                for (i, item) in items.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.set_width(card_w);
                        let selected = self.lib_selected.as_deref() == Some(&item.name);
                        let stroke = if selected {
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(124, 92, 255))
                        } else {
                            egui::Stroke::NONE
                        };
                        // Display only: images/labels have hover
                        // sense, so the card gets its own click area
                        // below (single = pick, double = set).
                        let fill = if dark {
                            egui::Color32::from_rgb(38, 38, 46)
                        } else {
                            egui::Color32::WHITE
                        };
                        let card = egui::Frame::default()
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(6.0)
                            .show(ui, |ui| {
                                // Fixed thumbnail box: every card ends up the
                                // same height regardless of source aspect.
                                const THUMB_H: f32 = 110.0;
                                let w = (card_w - 20.0).max(60.0);
                                let thumb_layout =
                                    egui::Layout::centered_and_justified(egui::Direction::TopDown);
                                if let Some(tex) = self.thumbs.get(&item.name) {
                                    // Fit inside the box, keep aspect.
                                    let size = tex.size_vec2();
                                    let scale =
                                        (w / size.x.max(1.0)).min(THUMB_H / size.y.max(1.0));
                                    let iw = (size.x * scale).max(1.0);
                                    let ih = (size.y * scale).max(1.0);
                                    let id = tex.id();
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(w, THUMB_H),
                                        thumb_layout,
                                        |ui| {
                                            ui.image((id, egui::vec2(iw, ih)));
                                        },
                                    );
                                } else {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(w, THUMB_H),
                                        thumb_layout,
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(match item.kind {
                                                    library::MediaKind::Image => "[img]",
                                                    library::MediaKind::Video => "[vid]",
                                                })
                                                .weak()
                                                .small(),
                                            );
                                        },
                                    );
                                }
                                let fav = if self.favorites.contains(&item.name) {
                                    "★ "
                                } else {
                                    ""
                                };
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{fav}{}",
                                        truncate_middle(&item.name, 24)
                                    ))
                                    .small(),
                                )
                                .on_hover_text(&item.name);
                            });
                        let click = ui.interact(
                            card.response.rect,
                            ui.make_persistent_id(&item.name),
                            egui::Sense::click(),
                        );
                        if click.double_clicked() {
                            action = Some(LibAction::Set(item.name.clone()));
                        } else if click.clicked() {
                            action = Some(LibAction::Select(item.name.clone()));
                        }
                    });
                    if (i + 1) % ncols == 0 {
                        ui.end_row();
                    }
                }
            });
        action
    }

    /// Library detail panel for the selected wallpaper. Right column in
    /// wide mode, below the grid in narrow mode.
    fn library_detail(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) -> Option<LibAction> {
        let dark = self.applied_style.unwrap_or(true);
        let mut action: Option<LibAction> = None;
        let detail: Option<library::MediaItem> = self
            .lib_selected
            .as_ref()
            .and_then(|sel| self.library.iter().find(|it| &it.name == sel).cloned());
        match detail {
            Some(item) => {
                card_frame(dark).show(ui, |ui| {
                    let c = &mut *ui;
                    if let Some(tex) = self.thumbs.get(&item.name) {
                        // Natural aspect, capped width, centered — never
                        // stretched across the panel (see #preview-stretch).
                        let size = tex.size_vec2();
                        let avail = c.available_width().max(80.0);
                        let w = avail.min(420.0);
                        let h = (w * size.y / size.x.max(1.0)).min(260.0);
                        c.horizontal(|c| {
                            c.add_space(((avail - w) / 2.0).max(0.0));
                            c.image((tex.id(), egui::vec2(w, h)));
                        });
                    }
                    c.label(egui::RichText::new(truncate_middle(&item.name, 60)).strong())
                        .on_hover_text(&item.name);
                    let kind_label = match item.kind {
                        library::MediaKind::Video => i18n::tr(self.lang, "type_video"),
                        library::MediaKind::Image => i18n::tr(self.lang, "type_image"),
                    };
                    c.horizontal_wrapped(|c| {
                        c.label(egui::RichText::new(kind_label).weak());
                        c.label(egui::RichText::new(format_size_age(self.lang, &item)).weak());
                    });
                    c.add_space(4.0);
                    if big_accent_button(c, dark, i18n::tr(self.lang, "library_set")).clicked() {
                        action = Some(LibAction::Set(item.name.clone()));
                    }
                    c.horizontal_wrapped(|c| {
                        let is_fav = self.favorites.contains(&item.name);
                        if c.button(if is_fav {
                            i18n::tr(self.lang, "fav_on")
                        } else {
                            i18n::tr(self.lang, "fav_off")
                        })
                        .clicked()
                        {
                            action = Some(LibAction::ToggleFav(item.name.clone()));
                        }
                        if c.button(i18n::tr(self.lang, "library_add_rotation"))
                            .clicked()
                        {
                            action = Some(LibAction::AddRot(item.name.clone()));
                        }
                        if c.button(i18n::tr(self.lang, "library_delete")).clicked() {
                            action = Some(LibAction::Delete(item.name.clone()));
                        }
                    });
                });
            }
            None => {
                ui.label(egui::RichText::new(i18n::tr(self.lang, "lib_empty_pick")).weak());
            }
        }
        action
    }

    fn library_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Drop the selection when its file vanished.
        if let Some(sel) = &self.lib_selected {
            if !self.library.iter().any(|it| &it.name == sel) {
                self.lib_selected = None;
            }
        }
        // No selection: grid takes the full width (no empty black panel).
        // Selection: Wallpaper-Engine style grid left, detail panel right.
        // Narrow windows keep the stacked layout.
        let action: Option<LibAction> = if self.lib_selected.is_none() {
            self.library_grid(ui, ctx)
        } else if ui.available_width() > 560.0 {
            // Grid ~2/3, detail ~1/3 (capped at 340 px) — 50/50
            // made the detail dominate the window.
            let total = ui.available_width();
            let detail_w = (total * 0.34).clamp(240.0, 340.0);
            let grid_w = (total - detail_w - 8.0).max(200.0);
            let height = ui.available_height();
            let mut grid_action = None;
            let mut detail_action = None;
            ui.horizontal_top(|ui| {
                let col_layout = egui::Layout::top_down(egui::Align::Min);
                ui.allocate_ui_with_layout(egui::vec2(grid_w, height), col_layout, |ui| {
                    ui.set_width(grid_w);
                    grid_action = self.library_grid(ui, ctx);
                });
                ui.allocate_ui_with_layout(egui::vec2(detail_w, height), col_layout, |ui| {
                    ui.set_width(detail_w);
                    detail_action = self.library_detail(ui, ctx);
                });
            });
            grid_action.or(detail_action)
        } else {
            let mut stacked = self.library_grid(ui, ctx);
            ui.add_space(4.0);
            if stacked.is_none() {
                stacked = self.library_detail(ui, ctx);
            }
            stacked
        };
        match action {
            Some(LibAction::Select(name)) => {
                self.lib_selected = Some(name);
            }
            Some(LibAction::Set(name)) => {
                if let Some(path) = library::safe_path(&library::media_dir(), &name) {
                    self.lib_selected = Some(name);
                    self.file = path.to_string_lossy().into_owned();
                    self.set_wallpaper();
                    self.persist();
                }
            }
            Some(LibAction::AddRot(name)) => {
                if let Some(path) = library::safe_path(&library::media_dir(), &name) {
                    self.rotation.add(&path.to_string_lossy());
                    self.persist();
                }
            }
            Some(LibAction::Delete(name)) => {
                if let Some(path) = library::safe_path(&library::media_dir(), &name) {
                    let _ = std::fs::remove_file(&path);
                    self.rotation.remove(&path.to_string_lossy());
                    self.favorites.remove(&name);
                    if self.lib_selected.as_deref() == Some(&name) {
                        self.lib_selected = None;
                    }
                    self.persist();
                    self.rescan_library(ctx);
                }
            }
            Some(LibAction::ToggleFav(name)) => {
                if !self.favorites.remove(&name) {
                    self.favorites.insert(name);
                }
                self.persist();
            }
            None => {}
        }
    }

    fn stop_video(&mut self) {
        if let Some(run) = self.running.take() {
            reap_runner(run);
        }
        for run in self.extra.drain(..) {
            reap_runner(run);
        }
        // An in-flight preroll dies with the video (player + canvas).
        self.drop_pending();
        self.paused = false;
        self.auto_paused = false;
        self.auto.reset();
        self.status = i18n::tr(self.lang, "status_stopped");
    }

    fn toggle_pause(&mut self) {
        if !self.any_running() {
            return;
        }
        self.paused = !self.paused;
        let effective = self.paused || self.auto_paused;
        if let Some(run) = &self.running {
            let _ = run.player.set_paused(effective);
        }
        for run in &self.extra {
            let _ = run.player.set_paused(effective);
        }
        self.status = if self.paused {
            i18n::tr(self.lang, "status_paused")
        } else if self.auto_paused {
            i18n::tr(self.lang, "status_autopaused")
        } else {
            i18n::tr(self.lang, "status_playing")
        };
    }

    /// After the user flips an autopause rule: resume at once when no
    /// enabled rule fires anymore. `set_rules(false, false)` clears the
    /// state machine, but the already-paused mpv stays frozen without
    /// this (looks like "video stopped even after I turned the rule
    /// off"). Never overrides the manual Pause button.
    fn refresh_autopause_after_rule_change(&mut self) {
        if self.paused || !self.any_running() {
            return;
        }
        let want = wallmotion_core::autopause::should_pause(
            poll_fullscreen(),
            poll_battery(),
            self.pause_on_fullscreen,
            self.pause_on_battery,
        );
        if !want && self.auto_paused {
            self.auto_paused = false;
            if let Some(run) = &self.running {
                let _ = run.player.set_paused(false);
            }
            for run in &self.extra {
                let _ = run.player.set_paused(false);
            }
            // Pending warmups were spawned with the old paused state;
            // make sure the swap doesn't re-freeze them.
            for run in &self.pending {
                let _ = run.player.set_paused(false);
            }
            self.status = i18n::tr(self.lang, "status_playing");
            debug_log("autopause off (rule disabled) -> resume");
        }
    }

    /// One autopause poll (throttled by the caller). Never overrides the
    /// manual Pause button; pauses at once, resumes after 2 clean polls.
    fn autopause_tick(&mut self) {
        if !self.any_running() || self.paused {
            return;
        }
        let fullscreen = poll_fullscreen();
        let battery = poll_battery();
        match self.auto.tick(fullscreen, battery, self.paused) {
            Some(true) => {
                debug_log(&format!(
                    "autopause on fullscreen={fullscreen} battery={battery}"
                ));
                self.auto_paused = true;
                if let Some(run) = &self.running {
                    let _ = run.player.set_paused(true);
                }
                for run in &self.extra {
                    let _ = run.player.set_paused(true);
                }
                self.status = i18n::tr(self.lang, "status_autopaused");
            }
            Some(false) => {
                debug_log(&format!(
                    "autopause off fullscreen={fullscreen} battery={battery}"
                ));
                self.auto_paused = false;
                if let Some(run) = &self.running {
                    let _ = run.player.set_paused(false);
                }
                for run in &self.extra {
                    let _ = run.player.set_paused(false);
                }
                self.status = i18n::tr(self.lang, "status_playing");
            }
            None => {}
        }
    }

    fn set_wallpaper(&mut self) {
        let path = PathBuf::from(self.file.trim());
        if !path.is_file() {
            self.stop_video();
            self.status = i18n::tr(self.lang, "warn_nofile_m");
            return;
        }
        // Per-file volume memory (mirrors Python `_apply_volume_memory`).
        self.apply_volume_memory();
        if is_image_path(&self.file) {
            // Static wallpaper first (it lands behind the video canvas),
            // then teardown: no flash of the original background.
            self.set_image(&path);
            let img_status = self.status.clone();
            self.stop_video();
            self.status = img_status;
            return;
        }
        if self.any_running() {
            self.switch_video(path);
        } else {
            self.stop_video();
            self.set_video(path);
        }
    }

    #[cfg(windows)]
    fn selected_rect(&self) -> (i32, i32, i32, i32) {
        if self.monitor.is_empty() {
            return wallmotion_win::canvas::sys::virtual_screen();
        }
        if let Some(m) = self.monitors.iter().find(|m| m.name == self.monitor) {
            if let Some((l, t, r, b)) = m.rect {
                return (l, t, (r - l).max(1), (b - t).max(1));
            }
        }
        // Monitor vanished (unplugged) -> fall back to virtual screen.
        wallmotion_win::canvas::sys::virtual_screen()
    }

    #[cfg(windows)]
    fn set_image(&mut self, path: &std::path::Path) {
        use wallmotion_win::wallpaper;
        let (x, y, w, h) = self.selected_rect();
        let _ = (x, y); // SystemParametersInfo sets all monitors at once.
        let fitted = wallpaper::fit_image_to_screen(path, w as u32, h as u32);
        if wallpaper::set_static_wallpaper(&fitted) {
            self.status = i18n::trf(
                self.lang,
                "img_set",
                &[("w", &w.to_string()), ("h", &h.to_string())],
            );
        } else {
            self.status = i18n::tr(self.lang, "img_failed");
        }
    }

    #[cfg(not(windows))]
    fn set_image(&mut self, _path: &std::path::Path) {
        self.status = i18n::tr(self.lang, "no_canvas");
    }

    /// "Playing behind icons (WxH at X,Y on M)" status line.
    #[cfg(windows)]
    fn video_status(&self) -> String {
        let where_tag = if self.monitor.is_empty() {
            i18n::tr(self.lang, "monitor_all")
        } else {
            self.monitor.clone()
        };
        let (x, y, w, h) = self.selected_rect();
        i18n::trf(
            self.lang,
            "status_video_where",
            &[
                ("w", &w.to_string()),
                ("h", &h.to_string()),
                ("x", &x.to_string()),
                ("y", &y.to_string()),
                ("m", &where_tag),
            ],
        )
    }

    /// Start in-process playback of `path` on the given canvas without
    /// touching the current playback. The caller adopts the runner, or
    /// destroys a freshly built canvas on failure. Err = localized
    /// status text.
    /// Windows-only callers (other platforms report `no_canvas`
    /// without spawning): silence the Linux dead-code lint.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn launch_video(
        &mut self,
        canvas: isize,
        rect: (i32, i32, i32, i32),
        path: &std::path::Path,
        muted: bool,
        audible: bool,
    ) -> Result<RunningVideo, String> {
        #[cfg(windows)]
        {
            let player = match wallmotion_mpv::Player::open(canvas) {
                Ok(p) => p,
                Err(e) => return Err(i18n::trf(self.lang, "mpv_failed", &[("e", &e)])),
            };
            // Rotation mode plays without loop: EndOfFile advances the
            // playlist (same contract the exiting sidecar used to give).
            let loop_file = !(self.rotation_enabled && !self.rotation.is_empty());
            if let Err(e) = player.set_muted(muted) {
                return Err(i18n::trf(self.lang, "mpv_failed", &[("e", &e)]));
            }
            if !muted {
                if let Err(e) = player.set_volume(self.volume as f32 / 100.0) {
                    return Err(i18n::trf(self.lang, "mpv_failed", &[("e", &e)]));
                }
            }
            match player.load_file(path, loop_file) {
                Ok(()) => Ok(RunningVideo {
                    player,
                    canvas,
                    rect,
                    audible,
                    eof: None,
                    painted: false,
                }),
                Err(e) => Err(i18n::trf(self.lang, "mpv_failed", &[("e", &e)])),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (canvas, rect, path, muted, audible);
            Err(i18n::tr(self.lang, "no_canvas"))
        }
    }
    /// Any video playing (primary or extra monitor runner).
    fn any_running(&self) -> bool {
        self.running.is_some() || !self.extra.is_empty()
    }

    /// Target canvas rects: one per selected monitor (same video on
    /// each), or the whole virtual screen when the monitor list is
    /// unavailable. `monitor` empty = all monitors.
    #[cfg(windows)]
    fn target_rects(&self) -> Vec<(i32, i32, i32, i32)> {
        if !self.monitor.is_empty() {
            if let Some(m) = self.monitors.iter().find(|m| m.name == self.monitor) {
                if let Some((l, t, r, b)) = m.rect {
                    return vec![(l, t, (r - l).max(1), (b - t).max(1))];
                }
            }
        } else {
            let rects: Vec<(i32, i32, i32, i32)> = self
                .monitors
                .iter()
                .filter_map(|m| {
                    m.rect
                        .map(|(l, t, r, b)| (l, t, (r - l).max(1), (b - t).max(1)))
                })
                .collect();
            if !rects.is_empty() {
                return rects;
            }
        }
        let (x, y, w, h) = wallmotion_win::canvas::sys::virtual_screen();
        vec![(x, y, w, h)]
    }

    /// Seamless switch: the successor warms up on fresh hidden
    /// canvases while the old video keeps playing, then they swap on
    /// the first frame (see `begin_preroll`/`commit_preroll`). A stale
    /// layout falls back to a full rebuild.
    fn switch_video(&mut self, path: PathBuf) {
        if !self.any_running() {
            self.set_video(path);
            return;
        }
        // Preroll iff the monitor layout is unchanged (same rects, same
        // order); the old runners stay put until the swap.
        #[cfg(windows)]
        let reuse = {
            let mut olds: Vec<(i32, i32, i32, i32)> = vec![];
            if let Some(ref run) = self.running {
                olds.push(run.rect);
            }
            olds.extend(self.extra.iter().map(|r| r.rect));
            olds == self.target_rects()
        };
        #[cfg(not(windows))]
        let reuse = false;
        if !reuse {
            if let Some(run) = self.running.take() {
                reap_runner(run);
            }
            for run in self.extra.drain(..) {
                reap_runner(run);
            }
            self.drop_pending();
            self.set_video(path);
            return;
        }
        debug_log(&format!("switch {}", path.display()));
        // Double-buffered: the successor warms up on hidden canvases
        // while the old video keeps playing; the swap happens on the
        // first frame (see `commit_preroll`), so there is no gap.
        #[cfg(windows)]
        self.begin_preroll(path);
        #[cfg(not(windows))]
        let _ = path;
    }

    /// Drop uncommitted preroll runners (player + canvas each).
    /// Best effort, never panics.
    fn drop_pending(&mut self) {
        for run in self.pending.drain(..) {
            run.player.terminate();
            #[cfg(windows)]
            wallmotion_win::canvas::sys::destroy_canvas(run.canvas);
            #[cfg(not(windows))]
            let _ = run.canvas;
        }
        self.pending_deadline = None;
    }

    /// Start the successor video on fresh hidden canvases (one per
    /// monitor rect). Old runners keep playing until `commit_preroll`.
    /// Replaces any in-flight preroll; falls back to keeping the old
    /// video when nothing could start. Windows only (other platforms
    /// never reach here — `reuse` is false there).
    #[cfg(windows)]
    fn begin_preroll(&mut self, path: PathBuf) {
        use wallmotion_win::canvas::sys as canvas;
        self.drop_pending();
        // The predecessor just hit EOF (rotation) but stays in `running`
        // until the swap: clear its stale mark so `rotation_tick` doesn't
        // advance twice while the successor is still warming up.
        if let Some(run) = &mut self.running {
            run.eof = None;
        }
        for run in &mut self.extra {
            run.eof = None;
        }
        let mut started: Vec<RunningVideo> = vec![];
        for (i, (x, y, w, h)) in self.target_rects().into_iter().enumerate() {
            let wc = match canvas::setup_wallpaper_canvas(x, y, w, h) {
                Some(wc) => wc,
                None => continue,
            };
            // Hidden warmup: mpv keeps decoding into a hidden canvas,
            // so the first frame is ready before the swap.
            canvas::set_canvas_visible(wc.canvas, false);
            let audible = i == 0;
            match self.launch_video(
                wc.canvas,
                (x, y, w, h),
                &path,
                self.muted || !audible,
                audible,
            ) {
                Ok(run) => started.push(run),
                Err(msg) => {
                    canvas::destroy_canvas(wc.canvas);
                    self.status = msg;
                }
            }
        }
        if started.is_empty() {
            debug_log("preroll: no monitor could start, keeping old video");
            return;
        }
        // setup_wallpaper_canvas pins WorkerW right below each fresh
        // canvas, which covers the still-playing old canvases until the
        // swap (opaque WorkerW = original wallpaper on screen). Re-stack
        // at once: old (visible) on top, warmups hidden beneath.
        let mut stack: Vec<isize> = vec![];
        if let Some(run) = &self.running {
            stack.push(run.canvas);
        }
        for run in &self.extra {
            stack.push(run.canvas);
        }
        for run in &started {
            stack.push(run.canvas);
        }
        canvas::ensure_canvases_stacked(&stack);
        debug_log(&format!("preroll {} canvas(es)", started.len()));
        self.pending = started;
        self.pending_deadline = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
    }

    /// Swap prerolled runners in: show the warmed-up canvases stacked
    /// below the icons, then reap the predecessors. Called once every
    /// canvas painted (or on the 3 s deadline — same contract as the
    /// first-frame sync). Windows only.
    #[cfg(windows)]
    fn commit_preroll(&mut self) {
        use wallmotion_win::canvas::sys as canvas;
        if self.pending.is_empty() {
            return;
        }
        let fresh: Vec<RunningVideo> = std::mem::take(&mut self.pending);
        self.pending_deadline = None;
        if fresh.iter().all(|run| !run.painted) {
            debug_log("swap go painted=false");
        }
        let canvases: Vec<isize> = fresh.iter().map(|run| run.canvas).collect();
        for &c in &canvases {
            canvas::set_canvas_visible(c, true);
        }
        canvas::ensure_canvases_stacked(&canvases);
        if let Some(run) = self.running.take() {
            reap_runner(run);
        }
        for run in self.extra.drain(..) {
            reap_runner(run);
        }
        let effective_pause = self.paused || self.auto_paused;
        let mut it = fresh.into_iter();
        self.running = it.next();
        self.extra = it.collect();
        if let Some(run) = &self.running {
            let _ = run.player.set_paused(effective_pause);
        }
        for run in &self.extra {
            let _ = run.player.set_paused(effective_pause);
        }
        // Keep pause state across the swap: clearing `paused` here would
        // unpause behind the user's back on every rotation advance, and
        // clearing `auto` would replay-then-repause while a fullscreen
        // app stays open. Fresh runners already got `effective_pause`.
        self.status = if self.paused {
            i18n::tr(self.lang, "status_paused")
        } else if self.auto_paused {
            i18n::tr(self.lang, "status_autopaused")
        } else {
            self.video_status()
        };
        debug_log("swap ok");
    }

    /// Fresh playback of `path` on every selected monitor (first one
    /// audible, the rest muted). Any previous runners are reaped first.
    /// Two phases so all mpvs start back-to-back and warm up in
    /// parallel: canvases first, spawns second.
    fn set_video(&mut self, path: PathBuf) {
        if let Some(run) = self.running.take() {
            reap_runner(run);
        }
        for run in self.extra.drain(..) {
            reap_runner(run);
        }
        #[cfg(windows)]
        {
            use wallmotion_win::canvas::sys as canvas;
            // Phase 1: canvases (window setup can reshuffle the desktop).
            let mut stages: Vec<(isize, (i32, i32, i32, i32))> = vec![];
            let mut failed: Option<String> = None;
            for (x, y, w, h) in self.target_rects() {
                match canvas::setup_wallpaper_canvas(x, y, w, h) {
                    Some(wc) => stages.push((wc.canvas, (x, y, w, h))),
                    None => {
                        failed = Some(i18n::tr(self.lang, "no_canvas"));
                        break;
                    }
                }
            }
            if stages.is_empty() {
                if let Some(msg) = failed {
                    self.status = msg;
                }
                return;
            }
            // Phase 2: spawns back-to-back (parallel warmup).
            for (i, (canvas, rect)) in stages.into_iter().enumerate() {
                let audible = i == 0;
                match self.launch_video(canvas, rect, &path, self.muted || !audible, audible) {
                    Ok(run) => {
                        if self.running.is_none() {
                            self.status = self.video_status();
                            self.running = Some(run);
                        } else {
                            self.extra.push(run);
                        }
                    }
                    Err(msg) => {
                        canvas::destroy_canvas(canvas);
                        failed = Some(msg);
                        break;
                    }
                }
            }
            if self.running.is_none() {
                if let Some(msg) = failed {
                    self.status = msg;
                }
            } else {
                if failed.is_some() {
                    debug_log("set_video: some monitors failed to start");
                }
                self.arm_sync();
            }
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            self.status = i18n::tr(self.lang, "no_canvas");
        }
    }

    fn apply_mute_volume(&mut self) {
        // The audible runner follows the user settings; monitor echoes
        // stay muted no matter what (one video must not echo).
        for run in self.running.iter().chain(self.extra.iter()) {
            let muted = self.muted || !run.audible;
            let _ = run.player.set_muted(muted);
            if !muted {
                let _ = run.player.set_volume(self.volume as f32 / 100.0);
            }
        }
    }

    /// Arm the first-frame sync: runners start frozen, the event pump
    /// releases them together once every canvas painted (or the 3 s
    /// deadline). Honors a pause the user hits meanwhile (captured
    /// here, same as the old probe thread did).
    /// Windows-only caller (same reason as `launch_video`).
    #[cfg_attr(not(windows), allow(dead_code))]
    fn arm_sync(&mut self) {
        for run in self.running.iter_mut().chain(self.extra.iter_mut()) {
            run.painted = false;
        }
        if self.running.is_none() && self.extra.is_empty() {
            return;
        }
        self.sync_waiting = true;
        self.sync_deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        self.sync_resume = !self.paused && !self.auto_paused;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        heartbeat();
        // Tray menu actions (backup path; the tray thread usually wins).
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            debug_log(&format!("menu event on ui thread: {}", event.id.0.as_str()));
            match event.id.0.as_str() {
                TRAY_SHOW_ID => show_window(ctx),
                TRAY_QUIT_ID => {
                    self.quit_requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }
        // Quit requested by the tray thread: close with cleanup.
        if TRAY_QUIT.load(std::sync::atomic::Ordering::SeqCst) {
            self.quit_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Global media hotkeys (Ctrl+Alt+arrows / Space / M). Work even
        // with the window hidden; the worker requested a repaint.
        if !self.hotkeys_attached {
            self.hotkeys.attach_ctx(ctx);
            self.hotkeys_attached = true;
        }
        for action in self.hotkeys.drain() {
            match action {
                hotkeys::HotkeyAction::Next => {
                    if let Some(path) = self.rotation.next_file() {
                        self.play_rotation_path(path);
                    }
                }
                hotkeys::HotkeyAction::Prev => {
                    if let Some(path) = self.rotation.prev_file() {
                        self.play_rotation_path(path);
                    }
                }
                hotkeys::HotkeyAction::Pause => self.toggle_pause(),
                hotkeys::HotkeyAction::Mute => {
                    self.muted = !self.muted;
                    self.apply_mute_volume();
                    self.persist();
                }
            }
            ctx.request_repaint();
        }
        // Remote CLI commands from later invocations (single instance).
        let pending: Vec<String> = self
            .remote_rx
            .as_ref()
            .map(|rx| {
                let mut out = Vec::new();
                while let Ok(line) = rx.try_recv() {
                    out.push(line);
                }
                out
            })
            .unwrap_or_default();
        for line in pending {
            self.apply_remote_command(ctx, &line);
        }
        // Fresh-start `--set`: auto-play on the first frame.
        if self.pending_start {
            self.pending_start = false;
            self.set_wallpaper();
        }
        // Explorer drag & drop: copy dropped media into the library.
        self.handle_dropped_files(ctx);
        // Cache our HWND once (for taskbar-free hide/reveal).
        #[cfg(windows)]
        if APP_HWND.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            APP_HWND.store(find_app_hwnd(), std::sync::atomic::Ordering::SeqCst);
        }
        // Scan the library once on startup (thumbnails need a Context).
        if !self.lib_scanned {
            self.rescan_library(ctx);
        }
        // YouTube worker events (progress, playlist, done, tool installs).
        self.poll_yt_events(ctx);
        // In-app updater: silent daily check on first frame, then events.
        #[cfg(windows)]
        if !self.upd_auto_done {
            self.upd_auto_done = true;
            update::cleanup_leftovers();
            if update::now_secs().saturating_sub(self.upd_checked_at) >= update::CHECK_INTERVAL_SECS
            {
                self.start_update_check(true);
            }
        }
        self.poll_update_events();
        // Settings preview thumbnail (rebuilt only when the file changes).
        self.ensure_preview(ctx);
        // Theme: follow the OS (polled) or the manual sun button.
        if self.follow_system && self.last_theme_poll.elapsed() >= theme::POLL_INTERVAL {
            self.last_theme_poll = std::time::Instant::now();
            if let Some(t) = theme::read_system_theme() {
                self.sys_theme = Some(t);
            }
        }
        let effective = if self.follow_system {
            self.sys_theme.unwrap_or(self.theme)
        } else {
            self.theme
        };
        let want_dark = effective == theme::AppTheme::Dark;
        if self.applied_style != Some(want_dark) {
            apply_style(ctx, want_dark);
            self.applied_style = Some(want_dark);
        }
        // App logo texture (once): brand header next to the title.
        if self.logo.is_none() {
            if let Some((rgba, w, h)) = app_icon_rgba(48) {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                self.logo = Some(ctx.load_texture("app-logo", img, egui::TextureOptions::LINEAR));
            }
        }
        // `--minimized` (autostart logon): hide like X did (tray keeps
        // running) and resume the saved wallpaper without popping up.
        // Runs on the first frame, before any paint, so nothing flashes.
        if self.start_hidden {
            self.start_hidden = false;
            #[cfg(windows)]
            {
                if APP_HWND.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                    APP_HWND.store(find_app_hwnd(), std::sync::atomic::Ordering::SeqCst);
                }
                hide_app_window();
            }
            #[cfg(not(windows))]
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            if PathBuf::from(self.file.trim()).is_file() {
                self.set_wallpaper();
            }
        }
        // X/red close hides to tray (video keeps playing, tray icon
        // stays so the app can be reopened); only the tray Quit action
        // lets the close proceed (on_exit stops video).
        // NOTE: never Visible(false) here — hidden windows stop receiving
        // frames in winit, the UI loop dies and tray menu goes dead with
        // it (verified with examples/probe.rs). Instead we hide at the
        // Win32 level: no taskbar button, event loop untouched.
        if !self.quit_requested && ctx.input(|i| i.viewport().close_requested()) {
            debug_log("close requested: hide to tray");
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            #[cfg(windows)]
            {
                if APP_HWND.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                    APP_HWND.store(find_app_hwnd(), std::sync::atomic::Ordering::SeqCst);
                }
                hide_app_window();
            }
            #[cfg(not(windows))]
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        // Drag & drop overlay: shown while a file hovers over the window.
        self.draw_drop_hint(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Language + theme toggles, top-right (like the Python header).
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .checkbox(&mut self.follow_system, i18n::tr(self.lang, "theme_auto"))
                            .changed()
                        {
                            if let Some(t) = theme::read_system_theme() {
                                self.sys_theme = Some(t);
                            }
                            self.last_theme_poll = std::time::Instant::now();
                            self.persist();
                        }
                        ui.add_enabled_ui(!self.follow_system, |ui| {
                            let tip = if self.theme == theme::AppTheme::Dark {
                                i18n::tr(self.lang, "theme_light")
                            } else {
                                i18n::tr(self.lang, "theme_dark")
                            };
                            if theme_toggle_button(ui, self.theme)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.theme = self.theme.toggle();
                                self.persist();
                            }
                        });
                        if ui.button(self.lang.button_label()).clicked() {
                            self.lang = self.lang.toggle();
                            self.persist();
                        }
                        if ui
                            .button(i18n::tr(self.lang, "hotkeys_short"))
                            .on_hover_text(i18n::tr(self.lang, "hotkeys_title"))
                            .clicked()
                        {
                            self.show_hotkeys = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.image((logo.id(), egui::vec2(34.0, 34.0)));
                    }
                    ui.vertical(|ui| {
                        ui.heading("WallMotion");
                    });
                });
                ui.add_space(6.0);
                // Pill tabs: active one gets the brand accent fill.
                ui.horizontal(|ui| {
                    let dark = self.applied_style.unwrap_or(true);
                    for (tab, key) in [
                        (Tab::Settings, "settings_title"),
                        (Tab::Library, "library_title"),
                    ] {
                        let active = self.tab == tab;
                        let label = i18n::tr(self.lang, key);
                        let btn = if active {
                            egui::Button::new(
                                egui::RichText::new(label).color(egui::Color32::WHITE),
                            )
                            .fill(accent_color(dark))
                        } else {
                            egui::Button::new(label)
                        };
                        if ui.add(btn).clicked() {
                            self.tab = tab;
                        }
                    }
                });
                ui.separator();
                if self.tab == Tab::Settings {
                    // Wallpaper-Engine style: controls left, live preview
                    // right. Narrow windows keep the stacked layout.
                    if ui.available_width() > 560.0 {
                        ui.columns(2, |uis| {
                            self.settings_controls(&mut uis[0], ctx);
                            let dark = self.applied_style.unwrap_or(true);
                            card_frame(dark).show(&mut uis[1], |ui| {
                                self.preview_card(ui);
                            });
                        });
                    } else {
                        let dark = self.applied_style.unwrap_or(true);
                        card_frame(dark).show(ui, |ui| {
                            self.preview_card(ui);
                        });
                        ui.add_space(8.0);
                        self.settings_controls(ui, ctx);
                    }
                } else {
                    self.library_tab(ui, ctx);
                }
                ui.add_space(4.0);
                ui.separator();
                ui.label(egui::RichText::new(&self.status).small().weak());
            }); // ScrollArea
        });
        // Player events (first-frame marks, end-of-file marks), then
        // the rotation tick (video-end driven; images dwell on interval).
        self.poll_player_events();
        self.rotation_tick();
        // Preroll swap: all hidden canvases painted (or the deadline
        // passed) — show them and reap the predecessors.
        #[cfg(windows)]
        {
            let ready = !self.pending.is_empty()
                && (self.pending.iter().all(|run| run.painted)
                    || self
                        .pending_deadline
                        .is_some_and(|d| std::time::Instant::now() >= d));
            if ready {
                self.commit_preroll();
            }
        }
        // Z-order watchdog: opaque WorkerW backgrounds drift above our
        // canvases (video vanishes, audio plays on). Re-pin below icons,
        // logging only real reorderings (plus a playback poll line).
        // 500 ms: a drift then flashes the original wallpaper only
        // briefly instead of up to 2 s. Cheap (a few Win32 calls).
        if self.any_running() && self.last_zcheck.elapsed() >= std::time::Duration::from_millis(500)
        {
            self.last_zcheck = std::time::Instant::now();
            #[cfg(windows)]
            {
                use wallmotion_win::canvas::sys as canvas;
                // Stack all canvases (primary + extra monitors) below
                // the icons in order. Per-canvas "right below DefView"
                // pinning made dual-monitor canvases fight each other
                // (each swap flashed the original wallpaper).
                let mut all: Vec<isize> = Vec::new();
                if let Some(run) = &self.running {
                    all.push(run.canvas);
                }
                for run in &self.extra {
                    all.push(run.canvas);
                }
                if all.iter().any(|&c| !canvas::canvas_below_defview(c)) {
                    debug_log(&format!("zfix tree: {}", canvas::debug_desktop_tree()));
                    canvas::ensure_canvases_stacked(&all);
                }
            }
            playback_poll_log(
                &self.file,
                self.running
                    .as_ref()
                    .and_then(|r| r.player.prop_bool("pause")),
                self.running
                    .as_ref()
                    .and_then(|r| r.player.prop_f64("time-pos")),
                self.running.as_ref().map(|r| r.eof.is_some()),
            );
        }
        // Autopause poll (Python POLL_INTERVAL_MS): pause at once, resume
        // after 2 clean polls. Throttled — update() runs every ~50ms.
        if self.last_poll.elapsed()
            >= std::time::Duration::from_millis(wallmotion_core::autopause::POLL_INTERVAL_MS)
        {
            self.last_poll = std::time::Instant::now();
            self.autopause_tick();
        }
        // Shortcuts cheat-sheet window (opened from the header button).
        self.hotkeys_window(ctx);
        // Keep the loop alive while minimized to tray: without a
        // periodic repaint, update() never runs and tray clicks die.
        // 50ms keeps the menu snappy at negligible idle cost.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        debug_log("on_exit: stop video");
        self.stop_video();
    }
}

fn tray_icon_rgba() -> Vec<u8> {
    // 32x32 accent tile with a darker border (no asset dependency).
    // Fallback only: `build_tray` prefers the real logo from icon.png.
    let mut buf = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let edge = x < 2 || y < 2 || x >= 30 || y >= 30;
            let (r, g, b) = if edge { (60, 44, 140) } else { (124, 92, 255) };
            let o = (y * 32 + x) * 4;
            buf[o] = r;
            buf[o + 1] = g;
            buf[o + 2] = b;
            buf[o + 3] = 255;
        }
    }
    buf
}

/// App logo (`assets/icon.png`, 256x256) decoded + resized to RGBA.
/// Embedded with `include_bytes!` so release builds need no asset dir.
fn app_icon_rgba(size: u32) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = include_bytes!("../../../../assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img
        .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), w, h))
}

/// Window title-bar icon, if the logo decodes.
fn window_icon() -> Option<egui::IconData> {
    let (rgba, w, h) = app_icon_rgba(64)?;
    Some(egui::IconData {
        rgba,
        width: w,
        height: h,
    })
}

/// Brand accent (the logo purple), theme-aware for contrast.
fn accent_color(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(124, 92, 255)
    } else {
        egui::Color32::from_rgb(93, 72, 200)
    }
}

/// Drop a runner's player and destroy its canvas. Takes ownership so no
/// stale handles survive a switch/stop (multi-monitor: one per screen).
/// The player is terminated first — see `Player::terminate`.
fn reap_runner(run: RunningVideo) {
    run.player.terminate();
    #[cfg(windows)]
    wallmotion_win::canvas::sys::destroy_canvas(run.canvas);
    #[cfg(not(windows))]
    let _ = run.canvas;
}

/// Whole-app style: brand accent selection, rounded widgets/windows.
fn apply_style(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let accent = accent_color(dark);
    visuals.selection.bg_fill = accent;
    // Selected text (combo dropdowns, menus) uses the selection stroke
    // color: keep it white so it stays readable on the accent fill.
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, egui::Color32::WHITE);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(8);
    }
    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.menu_corner_radius = egui::CornerRadius::same(8);
    ctx.set_visuals(visuals);
    // Roomier buttons app-wide (tabs, rows, dialogs).
    ctx.style_mut(|s| {
        s.spacing.button_padding = egui::vec2(10.0, 6.0);
    });
}

/// Primary call-to-action button: accent fill, white text.
fn accent_button(ui: &mut egui::Ui, dark: bool, text: String) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
            .fill(accent_color(dark)),
    )
}

/// Big primary call-to-action: full width, taller, larger text.
/// Used for Set wallpaper / Download so the main actions stand out.
fn big_accent_button(ui: &mut egui::Ui, dark: bool, text: String) -> egui::Response {
    ui.add_sized(
        egui::vec2(ui.available_width().max(60.0), 32.0),
        egui::Button::new(
            egui::RichText::new(text)
                .size(15.0)
                .color(egui::Color32::WHITE),
        )
        .fill(accent_color(dark)),
    )
}

/// Card container for settings/library sections: subtle elevated fill,
/// rounded corners, comfortable padding. Groups related controls so the
/// window reads as a few clear blocks instead of one long form.
fn card_frame(dark: bool) -> egui::Frame {
    let fill = if dark {
        egui::Color32::from_rgb(33, 33, 40)
    } else {
        egui::Color32::from_rgb(240, 240, 245)
    };
    egui::Frame::default()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(12.0)
}

/// Section heading inside a card, with a breath of space below.
fn card_title(ui: &mut egui::Ui, text: String) {
    ui.label(egui::RichText::new(text).strong());
    ui.add_space(4.0);
}

/// Settings card that always stretches to the full available width, so
/// sibling cards align their right edges even when their content is
/// short (the rotation card used to end up narrower).
fn settings_card(ui: &mut egui::Ui, dark: bool, add_contents: impl FnOnce(&mut egui::Ui)) {
    let full = ui.available_width().max(0.0);
    card_frame(dark).show(ui, |ui| {
        ui.set_width((full - 24.0).max(0.0));
        add_contents(ui);
    });
}

/// Short display name for a file path (file name only). The full path
/// goes to the tooltip, so `\\?\C:\...` monsters never stretch/break
/// the layout (the #1 "meh" offender in the old UI).
fn display_file_name(path: &str) -> String {
    let t = path.trim();
    if t.is_empty() {
        return String::new();
    }
    std::path::Path::new(t)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| t.to_string())
}

/// Image extensions handled by the static-wallpaper path (shared by
/// `set_wallpaper` and the rotation dwell check so both agree on what
/// "ends" and what just sits). GIF is video now: mpv plays it animated.
fn is_image_path(path: &str) -> bool {
    const IMAGES: [&str; 4] = ["jpg", "jpeg", "png", "bmp"];
    PathBuf::from(path.trim())
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .map(|ext| IMAGES.contains(&ext.as_str()))
        .unwrap_or(false)
}

/// "2.0 MB · před 3 dny" style detail meta for a library item.
fn format_size_age(lang: i18n::Lang, item: &library::MediaItem) -> String {
    let size = library::format_size(item.size);
    let age = match item.modified.and_then(|t| t.elapsed().ok()) {
        None => return size,
        Some(d) => {
            let secs = d.as_secs();
            if secs < 60 {
                i18n::tr(lang, "age_now")
            } else if secs < 3_600 {
                i18n::trf(lang, "age_min", &[("n", &(secs / 60).to_string())])
            } else if secs < 86_400 {
                i18n::trf(lang, "age_hour", &[("n", &(secs / 3_600).to_string())])
            } else {
                i18n::trf(lang, "age_day", &[("n", &(secs / 86_400).to_string())])
            }
        }
    };
    format!("{size} · {age}")
}

/// Middle-truncate a long label to ~`max` chars (`very-lo…-name.mp4`).
fn truncate_middle(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = (max.saturating_sub(1) / 2).max(1);
    let head: String = s.chars().take(keep).collect();
    let tail: String = s
        .chars()
        .rev()
        .take(keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// Header theme toggle: a painted sun/moon button (font-independent —
/// the ☀/☾ glyphs are missing from egui's default font).
fn theme_toggle_button(ui: &mut egui::Ui, current: theme::AppTheme) -> egui::Response {
    // Same height as the CZ text button next to it: text row + padding,
    // floored by interact_size (exactly like egui::Button sizes itself).
    let sp = ui.spacing().clone();
    let h = (ui.text_style_height(&egui::TextStyle::Button) + sp.button_padding.y * 2.0)
        .max(sp.interact_size.y);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, h), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let painter = ui.painter();
        painter.rect_filled(rect, visuals.corner_radius, visuals.bg_fill);
        let fg = visuals.fg_stroke.color;
        let c = rect.center();
        // Scale the glyph with the button height (drawn for ~22 px).
        let s = rect.height() / 22.0;
        match current {
            // Sun for the light theme: disc + 8 rays.
            theme::AppTheme::Light => {
                painter.circle_filled(c, 4.5 * s, fg);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::PI / 4.0;
                    let dir = egui::vec2(a.cos(), a.sin());
                    painter.line_segment(
                        [c + dir * 6.5 * s, c + dir * 9.0 * s],
                        egui::Stroke::new(1.5_f32, fg),
                    );
                }
            }
            // Moon for the dark theme: disc with a bite taken out.
            theme::AppTheme::Dark => {
                painter.circle_filled(c, 5.5 * s, fg);
                painter.circle_filled(c + egui::vec2(2.2 * s, -1.6 * s), 4.4 * s, visuals.bg_fill);
            }
        }
    }
    response
}

fn build_tray() -> Option<tray_icon::TrayIcon> {
    use tray_icon::{menu::Menu, Icon, TrayIconBuilder};
    let menu = Menu::new();
    let show = tray_icon::menu::MenuItem::with_id(TRAY_SHOW_ID, "Open", true, None);
    let quit = tray_icon::menu::MenuItem::with_id(TRAY_QUIT_ID, "Quit", true, None);
    menu.append(&show).ok()?;
    menu.append(&quit).ok()?;
    let icon = match app_icon_rgba(32) {
        Some((rgba, w, h)) => Icon::from_rgba(rgba, w, h).ok()?,
        None => Icon::from_rgba(tray_icon_rgba(), 32, 32).ok()?,
    };
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("WallMotion (native)")
        .build()
        .ok()
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{}", cli::usage());
            std::process::exit(2);
        }
    };
    if args.version {
        println!("{}", wallmotion_core::VERSION);
        return;
    }
    let cmd = cli::args_to_command(&args);
    let action = cli::has_action(&args);
    if action && remote::send_command(&cmd.to_string()) {
        println!("Sent to the running instance.");
        return;
    }
    if instance::another_instance_running() {
        if action {
            eprintln!("Another instance is running but did not accept the command.");
        }
        return;
    }
    // Second gate: the command port is bound exclusively, so even a
    // deleted/stale lock file can never fork a duplicate instance (each
    // one would spawn its own canvas + mpv and fight over the desktop).
    let remote_rx = match remote::start_server() {
        Some(rx) => rx,
        None => {
            eprintln!("Another WallMotion instance is already running.");
            return;
        }
    };
    let _tray = build_tray();
    debug_log(&format!(
        "main start action={action} server=true tray={}",
        if _tray.is_some() { "ok" } else { "FAILED" }
    ));
    let mut viewport = egui::ViewportBuilder::default()
        // Wide enough for the two-column layout (controls | preview)
        // right away; narrow windows stack the cards below each other.
        .with_inner_size([940.0, 680.0])
        .with_min_inner_size([420.0, 480.0])
        .with_title("WallMotion (native)");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "WallMotion (native)",
        options,
        Box::new(|cc| {
            tray_menu_thread(cc.egui_ctx.clone());
            // Wake the UI loop twice a second from outside: when the
            // window hides to the tray, frames may stop coming on their
            // own and rotation/EOF polling would stall with them.
            // Harmless while visible (just an extra repaint request).
            let wake = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                wake.request_repaint();
            });
            let mut app = App {
                remote_rx: Some(remote_rx),
                start_hidden: args.minimized && !action,
                ..Default::default()
            };
            if action {
                app.apply_launch_command(&cmd);
            }
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("settings error: {e:?}");
    }
}
