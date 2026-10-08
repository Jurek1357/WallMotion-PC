//! Native settings window over the Rust core (video set/stop for now).
//!
//! eframe UI driving `wallmotion-win` canvas setup and the
//! `wallmotion-player` mpv sidecar. Windows only for playback;
//! elsewhere the window opens with an explanatory status.

//! No console window: this is a GUI app (also in debug builds, so a
//! plain double-click never leaves a terminal behind).
#![windows_subsystem = "windows"]

use std::path::PathBuf;
use wallmotion_player::{default_ipc_endpoint, ipc_set, IpcValue, SpawnOptions};

mod cli;
mod config;
mod instance;
mod library;
mod remote;
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

/// Show the main window: un-minimize, un-hide, repaint.
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.request_repaint();
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
    child: std::process::Child,
    ipc: String,
    canvas: isize,
}

struct App {
    file: String,
    status: String,
    muted: bool,
    volume: u8,
    running: Option<RunningVideo>,
    quit_requested: bool,
    paused: bool,
    monitor: String,
    monitors: Vec<MonitorChoice>,
    pause_on_fullscreen: bool,
    pause_on_battery: bool,
    auto: wallmotion_core::autopause::AutoPause,
    auto_paused: bool,
    last_poll: std::time::Instant,
    remote_rx: Option<std::sync::mpsc::Receiver<String>>,
    pending_start: bool,
    rotation: wallmotion_core::rotation::RotationQueue,
    rotation_enabled: bool,
    rotation_interval: u64,
    last_rotation: std::time::Instant,
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
    // Library gallery (Lively-style tab).
    tab: Tab,
    favorites: std::collections::HashSet<String>,
    lib_search: String,
    lib_fav_only: bool,
    lib_selected: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Settings,
    Library,
}

#[derive(Debug, Clone)]
struct MonitorChoice {
    name: String,
    label: String,
    rect: Option<(i32, i32, i32, i32)>,
}

fn monitor_label(name: &str, rect: (i32, i32, i32, i32), primary: bool) -> String {
    let (l, t, r, b) = rect;
    let tag = if primary { " - primary" } else { "" };
    format!("Monitor {} ({}x{}){tag}", name, r - l, b - t)
}

fn discover_monitors() -> Vec<MonitorChoice> {
    #[cfg(windows)]
    {
        wallmotion_win::canvas::sys::list_monitors()
            .into_iter()
            .map(|m| {
                let label = monitor_label(&m.name, m.rect, m.primary);
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
        Self {
            file: saved.last_path,
            status: "Pick a video file, then Set as wallpaper.".to_string(),
            muted: saved.muted,
            volume: saved.volume,
            running: None,
            quit_requested: false,
            paused: false,
            monitor: saved.monitor,
            monitors: discover_monitors(),
            auto: wallmotion_core::autopause::AutoPause::new(
                saved.pause_on_fullscreen,
                saved.pause_on_battery,
            ),
            auto_paused: false,
            pause_on_fullscreen: saved.pause_on_fullscreen,
            pause_on_battery: saved.pause_on_battery,
            last_poll: std::time::Instant::now(),
            remote_rx: None,
            pending_start: false,
            rotation_enabled: saved.rotation.enabled,
            rotation_interval: interval,
            last_rotation: std::time::Instant::now(),
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
            tab: Tab::Settings,
            favorites: saved.favorites.into_iter().collect(),
            lib_search: String::new(),
            lib_fav_only: false,
            lib_selected: None,
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
        }
        .save();
    }

    /// Restore remembered volume for the current file, if any.
    /// Mirrors Python `_apply_volume_memory`: the volume is always
    /// recalled, but mute is never turned OFF by memory (it may turn ON).
    fn apply_volume_memory(&mut self) {
        let path = self.file.trim().to_string();
        if path.is_empty() {
            return;
        }
        if let Some(entry) = wallmotion_core::volumememory::lookup(Some(&self.volumes), &path) {
            self.volume = entry.volume;
            if entry.muted {
                self.muted = true;
            }
        }
    }

    /// Play a rotation item now: set file, apply, restart the timer.
    fn play_rotation_path(&mut self, path: String) {
        self.file = path;
        self.set_wallpaper();
        self.persist();
        self.last_rotation = std::time::Instant::now();
    }

    /// (Re)detect the yt-dlp/ffmpeg sidecars. Cheap fs checks; called
    /// once at startup and after every provision/update event.
    fn rescan_tools(&mut self) {
        self.ytdlp_path = youtube::find_ytdlp();
        self.ytdlp_ver = self.ytdlp_path.as_deref().and_then(youtube::ytdlp_version);
        self.ffmpeg_path = youtube::find_ffmpeg();
        self.yt_tools_scanned = true;
    }

    /// Start one download (or a playlist fetch) for the URL in the box.
    fn start_yt_url(&mut self, ctx: &egui::Context) {
        use wallmotion_core::youtube as yt;
        let url = self.yt_url.trim().to_string();
        if url.is_empty() {
            self.yt_status = "Paste a YouTube link first.".to_string();
            return;
        }
        if !yt::is_valid_youtube_url(&url) {
            self.yt_status = "Not a YouTube video/playlist link.".to_string();
            return;
        }
        let Some(exe) = self.ytdlp_path.clone() else {
            self.yt_status = "yt-dlp not found — click Get yt-dlp below.".to_string();
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
            self.yt_status = "Loading playlist…".to_string();
            youtube::spawn_playlist_fetch(exe, url, self.yt_cancel.clone(), tx);
        } else {
            self.yt_queue.clear();
            self.yt_queue_total = 1;
            self.yt_status = "Downloading 0%".to_string();
            youtube::spawn_download(
                exe,
                url,
                library::media_dir(),
                self.ffmpeg_path.clone(),
                self.yt_child.clone(),
                self.yt_cancel.clone(),
                tx,
            );
        }
        ctx.request_repaint();
    }

    /// Start the next queued playlist item, if any.
    fn start_next_queued(&mut self, ctx: &egui::Context) {
        let Some(url) = self.yt_queue.pop_front() else {
            self.yt_queue_total = 0;
            return;
        };
        let Some(exe) = self.ytdlp_path.clone() else {
            self.yt_status = "yt-dlp not found — click Get yt-dlp below.".to_string();
            self.yt_busy = false;
            return;
        };
        let done = self.yt_queue_total - self.yt_queue.len();
        let (tx, rx) = std::sync::mpsc::channel();
        self.yt_rx = Some(rx);
        self.yt_busy = true;
        self.yt_progress = 0.0;
        self.yt_cancel = youtube::new_cancel_flag();
        self.yt_status = format!("Downloading {done}/{} 0%", self.yt_queue_total);
        youtube::spawn_download(
            exe,
            url,
            library::media_dir(),
            self.ffmpeg_path.clone(),
            self.yt_child.clone(),
            self.yt_cancel.clone(),
            tx,
        );
        ctx.request_repaint();
    }

    /// Drain YouTube worker events (progress, playlist, done, errors).
    fn poll_yt_events(&mut self, ctx: &egui::Context) {
        if !self.yt_tools_scanned {
            self.rescan_tools();
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
                    self.yt_status = if self.yt_queue_total > 1 {
                        let done = self.yt_queue_total - self.yt_queue.len();
                        format!("Downloading {done}/{} {pct:.0}%", self.yt_queue_total)
                    } else {
                        format!("Downloading {pct:.0}%")
                    };
                }
                youtube::YtEvent::Playlist(entries) => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_picked = vec![true; entries.len()];
                    self.yt_playlist = entries;
                    self.yt_status = "Pick videos, then Download selected.".to_string();
                }
                youtube::YtEvent::Finished(path) => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_progress = 100.0;
                    self.yt_status = "Downloaded.".to_string();
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
                    self.yt_status = youtube::error_text(&code);
                    // Playlist queue: skip the broken video, keep going.
                    if !self.yt_queue.is_empty() {
                        self.start_next_queued(ctx);
                    } else {
                        self.yt_queue_total = 0;
                    }
                }
                youtube::YtEvent::Tool(info) => {
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_status = info;
                    self.rescan_tools();
                }
            }
        }
    }

    /// Rescan the downloads folder and (re)build thumbnails. Images load
    /// directly; videos need ffmpeg on PATH (else a text badge is shown).
    fn rescan_library(&mut self, ctx: &egui::Context) {
        let dir = library::media_dir();
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

    /// One rotation tick. Timer-driven (videos loop, so unlike Python
    /// there is no follow-video-end mode).
    fn rotation_tick(&mut self) {
        if !self.rotation_enabled || self.rotation.is_empty() {
            return;
        }
        if self.last_rotation.elapsed()
            < std::time::Duration::from_secs(self.rotation_interval.max(1))
        {
            return;
        }
        self.last_rotation = std::time::Instant::now();
        if let Some(path) = self.rotation.next_file() {
            if !self.rotation.repeat() && path == self.file {
                // End of a non-repeating list: keep the last wallpaper
                // and switch rotation off (mirrors Python).
                self.rotation_enabled = false;
            } else {
                self.file = path;
                self.set_wallpaper();
            }
            self.persist();
        }
    }

    /// YouTube section: URL box, tool provisioning, progress, picker.
    fn youtube_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("YouTube download:");
        ui.horizontal(|ui| {
            ui.label("Link:");
            let url_entered = ui.text_edit_singleline(&mut self.yt_url).lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if self.yt_busy {
                if ui.button("Cancel").clicked() {
                    youtube::cancel_child(&self.yt_child, &self.yt_cancel);
                    self.yt_busy = false;
                    self.yt_rx = None;
                    self.yt_queue.clear();
                    self.yt_queue_total = 0;
                    self.yt_status = "Cancelled.".to_string();
                }
            } else if ui.button("Download").clicked() || url_entered {
                self.start_yt_url(ctx);
            }
        });
        if self.yt_busy && self.yt_progress > 0.0 {
            ui.add(
                egui::ProgressBar::new((self.yt_progress / 100.0).clamp(0.0, 1.0))
                    .show_percentage(),
            );
        }
        if !self.yt_status.is_empty() {
            ui.label(&self.yt_status);
        }
        // Sidecar tools: one-click install/update so it works out of box.
        ui.horizontal(|ui| {
            match (&self.ytdlp_path, &self.ytdlp_ver) {
                (Some(_), Some(v)) => {
                    ui.label(format!("yt-dlp {v}"));
                }
                (Some(_), None) => {
                    ui.label("yt-dlp found");
                }
                (None, _) => {
                    ui.label("yt-dlp missing");
                }
            }
            if !self.yt_busy {
                if self.ytdlp_path.is_none() {
                    if ui.button("Get yt-dlp").clicked() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.yt_rx = Some(rx);
                        self.yt_busy = true;
                        self.yt_progress = 0.0;
                        self.yt_status = "Downloading yt-dlp…".to_string();
                        youtube::spawn_provision_ytdlp(tx);
                        ctx.request_repaint();
                    }
                } else if ui.button("Update").clicked() {
                    if let Some(exe) = self.ytdlp_path.clone() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.yt_rx = Some(rx);
                        self.yt_busy = true;
                        self.yt_progress = 0.0;
                        self.yt_status = "Updating yt-dlp…".to_string();
                        youtube::spawn_ytdlp_update(exe, tx);
                        ctx.request_repaint();
                    }
                }
            }
            if self.ffmpeg_path.is_some() {
                ui.label("ffmpeg ok");
            } else {
                ui.label("ffmpeg missing (~720p max)");
                if !self.yt_busy && ui.button("Get ffmpeg").clicked() {
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.yt_rx = Some(rx);
                    self.yt_busy = true;
                    self.yt_progress = 0.0;
                    self.yt_status = "Downloading ffmpeg (~80 MB)…".to_string();
                    youtube::spawn_provision_ffmpeg(tx);
                    ctx.request_repaint();
                }
            }
        });
        // Playlist picker (titles fetched, nothing downloaded yet).
        if !self.yt_playlist.is_empty() {
            let titles: Vec<String> = self.yt_playlist.iter().map(|e| e.title.clone()).collect();
            ui.label(format!("Playlist ({}):", titles.len()));
            egui::ScrollArea::vertical()
                .max_height(140.0)
                .show(ui, |ui| {
                    for (i, title) in titles.iter().enumerate() {
                        ui.checkbox(&mut self.yt_picked[i], title);
                    }
                });
            ui.horizontal(|ui| {
                if ui.button("Download selected").clicked() {
                    let urls: Vec<String> = self
                        .yt_playlist
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| self.yt_picked.get(*i).copied().unwrap_or(false))
                        .map(|(_, e)| e.url.clone())
                        .collect();
                    if urls.is_empty() {
                        self.yt_status = "Nothing selected.".to_string();
                    } else {
                        self.yt_queue = urls.into_iter().collect();
                        self.yt_queue_total = self.yt_queue.len();
                        self.yt_playlist.clear();
                        self.yt_picked.clear();
                        self.start_next_queued(ctx);
                    }
                }
                if ui.button("Clear").clicked() {
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
    fn library_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lib_search).hint_text("Search…"));
            ui.checkbox(&mut self.lib_fav_only, "Favorites only");
            if ui.button("Refresh").clicked() {
                self.rescan_library(ctx);
            }
            if ui.button("Open folder").clicked() {
                open_folder(&library::media_dir());
            }
        });
        let query = self.lib_search.trim().to_lowercase();
        let items: Vec<library::MediaItem> = self
            .library
            .iter()
            .filter(|it| {
                if self.lib_fav_only && !self.favorites.contains(&it.name) {
                    return false;
                }
                query.is_empty() || it.name.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        // Drop the selection when its file vanished.
        if let Some(sel) = &self.lib_selected {
            if !self.library.iter().any(|it| &it.name == sel) {
                self.lib_selected = None;
            }
        }
        enum LibAction {
            Select(String),
            Set(String),
            AddRot(String),
            Delete(String),
            ToggleFav(String),
        }
        let mut action: Option<LibAction> = None;
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .max_height(380.0)
                .show(&mut cols[0], |ui| {
                    for item in &items {
                        let selected = self.lib_selected.as_deref() == Some(&item.name);
                        let stroke = if selected {
                            egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(124, 92, 255))
                        } else {
                            egui::Stroke::NONE
                        };
                        let mut clicked = false;
                        egui::Frame::default()
                            .stroke(stroke)
                            .inner_margin(4.0)
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    if let Some(tex) = self.thumbs.get(&item.name) {
                                        let size = tex.size_vec2();
                                        let w = 140.0;
                                        let h = (w * size.y / size.x.max(1.0)).clamp(40.0, 90.0);
                                        if ui.image((tex.id(), egui::vec2(w, h))).clicked() {
                                            clicked = true;
                                        }
                                    } else {
                                        let badge = match item.kind {
                                            library::MediaKind::Image => "[img]",
                                            library::MediaKind::Video => "[vid]",
                                        };
                                        if ui.button(badge).clicked() {
                                            clicked = true;
                                        }
                                    }
                                    let fav = if self.favorites.contains(&item.name) {
                                        "★ "
                                    } else {
                                        ""
                                    };
                                    if ui
                                        .label(
                                            egui::RichText::new(format!("{fav}{}", item.name))
                                                .small(),
                                        )
                                        .clicked()
                                    {
                                        clicked = true;
                                    }
                                });
                            });
                        if clicked {
                            action = Some(LibAction::Select(item.name.clone()));
                        }
                    }
                });
            // Detail panel for the selected wallpaper.
            let detail: Option<library::MediaItem> = self
                .lib_selected
                .as_ref()
                .and_then(|sel| self.library.iter().find(|it| &it.name == sel).cloned());
            match detail {
                Some(item) => {
                    let c = &mut cols[1];
                    if let Some(tex) = self.thumbs.get(&item.name) {
                        let size = tex.size_vec2();
                        let w = c.available_width().max(80.0);
                        let h = (w * size.y / size.x.max(1.0)).clamp(60.0, 220.0);
                        c.image((tex.id(), egui::vec2(w, h)));
                    }
                    c.label(egui::RichText::new(&item.name).strong());
                    c.label(
                        egui::RichText::new(format!(
                            "{} · {}",
                            item.kind.label(),
                            library::format_size(item.size)
                        ))
                        .small(),
                    );
                    let is_fav = self.favorites.contains(&item.name);
                    if c.button(if is_fav {
                        "★ Favorited"
                    } else {
                        "☆ Favorite"
                    })
                    .clicked()
                    {
                        action = Some(LibAction::ToggleFav(item.name.clone()));
                    }
                    if c.add_sized(
                        egui::vec2(c.available_width().max(60.0), 0.0),
                        egui::Button::new("Set wallpaper"),
                    )
                    .clicked()
                    {
                        action = Some(LibAction::Set(item.name.clone()));
                    }
                    if c.button("Add to rotation").clicked() {
                        action = Some(LibAction::AddRot(item.name.clone()));
                    }
                    if c.button("Delete").clicked() {
                        action = Some(LibAction::Delete(item.name.clone()));
                    }
                }
                None => {
                    cols[1].label("Pick a wallpaper on the left.");
                }
            }
        });
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

    fn mpv_bin() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("WALLMOTION_MPV") {
            let p = PathBuf::from(path);
            if p.is_file() {
                return Some(p);
            }
        }
        wallmotion_player::find_mpv(None)
    }

    fn stop_video(&mut self) {
        if let Some(mut run) = self.running.take() {
            let _ = run.child.kill();
            let _ = run.child.wait();
            #[cfg(windows)]
            wallmotion_win::canvas::sys::destroy_canvas(run.canvas);
            #[cfg(not(windows))]
            let _ = run.canvas;
        }
        self.paused = false;
        self.auto_paused = false;
        self.auto.reset();
        self.status = "Stopped.".to_string();
    }

    fn toggle_pause(&mut self) {
        if let Some(run) = &self.running {
            self.paused = !self.paused;
            let effective = self.paused || self.auto_paused;
            ipc_set(&run.ipc, "pause", &IpcValue::Bool(effective));
            self.status = if self.paused {
                "Paused.".to_string()
            } else if self.auto_paused {
                "Auto-paused (fullscreen/battery).".to_string()
            } else {
                "Playing.".to_string()
            };
        }
    }

    /// One autopause poll (throttled by the caller). Never overrides the
    /// manual Pause button; pauses at once, resumes after 2 clean polls.
    fn autopause_tick(&mut self) {
        if self.running.is_none() || self.paused {
            return;
        }
        let fullscreen = poll_fullscreen();
        let battery = poll_battery();
        match self.auto.tick(fullscreen, battery, self.paused) {
            Some(true) => {
                self.auto_paused = true;
                if let Some(run) = &self.running {
                    ipc_set(&run.ipc, "pause", &IpcValue::Bool(true));
                }
                self.status = "Auto-paused (fullscreen app or battery).".to_string();
            }
            Some(false) => {
                self.auto_paused = false;
                if let Some(run) = &self.running {
                    ipc_set(&run.ipc, "pause", &IpcValue::Bool(false));
                }
                self.status = "Playing.".to_string();
            }
            None => {}
        }
    }

    fn set_wallpaper(&mut self) {
        self.stop_video();
        let path = PathBuf::from(self.file.trim());
        if !path.is_file() {
            self.status = "Pick an existing image or video file first.".to_string();
            return;
        }
        // Per-file volume memory (mirrors Python `_apply_volume_memory`).
        self.apply_volume_memory();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        const IMAGES: [&str; 5] = ["jpg", "jpeg", "png", "bmp", "gif"];
        if IMAGES.contains(&ext.as_str()) {
            self.set_image(&path);
            return;
        }
        self.set_video(path);
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
            self.status = format!("Image fitted to {w}x{h} and set.");
        } else {
            self.status = "Could not set the image.".to_string();
        }
    }

    #[cfg(not(windows))]
    fn set_image(&mut self, _path: &std::path::Path) {
        self.status = "Static images need Windows in this build.".to_string();
    }

    fn set_video(&mut self, path: PathBuf) {
        use wallmotion_win::canvas::sys as canvas;
        let mpv = match Self::mpv_bin() {
            Some(p) => p,
            None => {
                self.status = "mpv not found (PATH or WALLMOTION_MPV).".to_string();
                return;
            }
        };
        #[cfg(windows)]
        {
            let (x, y, w, h) = self.selected_rect();
            let wc = match canvas::setup_wallpaper_canvas(x, y, w, h) {
                Some(wc) => wc,
                None => {
                    self.status = "No desktop canvas (run on Windows).".to_string();
                    return;
                }
            };
            let opts = SpawnOptions {
                mpv,
                wid: wc.canvas,
                file: path,
                muted: self.muted,
                volume: self.volume as f32 / 100.0,
                ipc_endpoint: default_ipc_endpoint(),
            };
            match wallmotion_player::spawn_mpv(&opts) {
                Ok(child) => {
                    let where_tag = if self.monitor.is_empty() {
                        "all monitors".to_string()
                    } else {
                        self.monitor.clone()
                    };
                    self.status =
                        format!("Playing behind icons ({w}x{h} at {x},{y} on {where_tag}).");
                    self.running = Some(RunningVideo {
                        child,
                        ipc: opts.ipc_endpoint,
                        canvas: wc.canvas,
                    });
                }
                Err(e) => {
                    canvas::destroy_canvas(wc.canvas);
                    self.status = format!("mpv failed to start: {e}");
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = mpv;
            self.status = "Video wallpaper needs Windows in this build.".to_string();
        }
    }

    fn apply_mute_volume(&mut self) {
        if let Some(run) = &self.running {
            ipc_set(&run.ipc, "mute", &IpcValue::Bool(self.muted));
            if !self.muted {
                ipc_set(&run.ipc, "volume", &IpcValue::Int(self.volume as i64));
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
        // Scan the library once on startup (thumbnails need a Context).
        if !self.lib_scanned {
            self.rescan_library(ctx);
        }
        // YouTube worker events (progress, playlist, done, tool installs).
        self.poll_yt_events(ctx);
        // X/red close minimizes to tray (video keeps playing); only the
        // tray Quit action lets the close proceed (on_exit stops video).
        // NOTE: never Visible(false) here — hidden windows stop receiving
        // frames in winit, the UI loop dies and tray menu goes dead with
        // it (verified with examples/probe.rs). Minimized windows keep
        // framing, and with_taskbar(false) keeps the taskbar clean.
        if !self.quit_requested && ctx.input(|i| i.viewport().close_requested()) {
            debug_log("close requested: minimize to tray");
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("WallMotion (native)");
                ui.label(format!("Backend: {}", wallmotion_win::backend_name()));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
                    ui.selectable_value(&mut self.tab, Tab::Library, "Wallpaper library");
                });
                ui.separator();
                if self.tab == Tab::Settings {
                    ui.horizontal(|ui| {
                        ui.label("Video file:");
                        ui.text_edit_singleline(&mut self.file);
                        if ui.button("Browse…").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter(
                                    "images & video",
                                    &[
                                        "jpg", "jpeg", "png", "bmp", "gif", "mp4", "mkv", "webm",
                                        "avi", "mov",
                                    ],
                                )
                                .pick_file()
                            {
                                self.file = path.to_string_lossy().into_owned();
                                self.apply_volume_memory();
                                self.persist();
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Set as wallpaper").clicked() {
                            self.set_wallpaper();
                        }
                        if ui.button("Stop").clicked() {
                            self.stop_video();
                        }
                        let pause_label = if self.paused { "Resume" } else { "Pause" };
                        ui.add_enabled_ui(self.running.is_some(), |ui| {
                            if ui.button(pause_label).clicked() {
                                self.toggle_pause();
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Monitor:");
                        let current = if self.monitor.is_empty() {
                            "All monitors".to_string()
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
                                        "All monitors",
                                    )
                                    .changed()
                                {
                                    self.persist();
                                }
                                for m in self.monitors.clone() {
                                    if ui
                                        .selectable_value(
                                            &mut self.monitor,
                                            m.name.clone(),
                                            &m.label,
                                        )
                                        .changed()
                                    {
                                        self.persist();
                                    }
                                }
                            });
                        if ui.button("Refresh").clicked() {
                            self.monitors = discover_monitors();
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.muted, "Mute").changed() {
                            self.apply_mute_volume();
                            self.persist();
                        }
                        ui.label("Volume:");
                        if ui
                            .add(egui::Slider::new(&mut self.volume, 0..=100).show_value(false))
                            .changed()
                        {
                            self.apply_mute_volume();
                            self.persist();
                        }
                        ui.label(format!("{}%", self.volume));
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(&mut self.pause_on_fullscreen, "Pause on fullscreen")
                            .changed()
                        {
                            self.auto
                                .set_rules(self.pause_on_fullscreen, self.pause_on_battery);
                            self.persist();
                        }
                        if ui
                            .checkbox(&mut self.pause_on_battery, "Pause on battery")
                            .changed()
                        {
                            self.auto
                                .set_rules(self.pause_on_fullscreen, self.pause_on_battery);
                            self.persist();
                        }
                    });
                    ui.separator();
                    ui.label("Rotation playlist:");
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.rotation_enabled, "Rotate").changed() {
                            self.last_rotation = std::time::Instant::now();
                            self.persist();
                        }
                        ui.label("Every:");
                        egui::ComboBox::from_id_salt("rot_interval")
                            .selected_text(wallmotion_core::rotation::format_interval(
                                self.rotation_interval,
                            ))
                            .show_ui(ui, |ui| {
                                for &secs in wallmotion_core::rotation::INTERVALS {
                                    if ui
                                        .selectable_value(
                                            &mut self.rotation_interval,
                                            secs,
                                            wallmotion_core::rotation::format_interval(secs),
                                        )
                                        .changed()
                                    {
                                        self.last_rotation = std::time::Instant::now();
                                        self.persist();
                                    }
                                }
                            });
                        let mut shuffle = self.rotation.shuffle();
                        if ui.checkbox(&mut shuffle, "Shuffle").changed() {
                            self.rotation.set_shuffle(shuffle);
                            self.persist();
                        }
                        let mut repeat = self.rotation.repeat();
                        if ui.checkbox(&mut repeat, "Repeat").changed() {
                            self.rotation.set_repeat(repeat);
                            self.persist();
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Add current").clicked()
                            && PathBuf::from(self.file.trim()).is_file()
                        {
                            self.rotation.add(self.file.trim());
                            self.persist();
                        }
                        if ui.button("Play").clicked() {
                            self.rotation_enabled = true;
                            if let Some(path) = self.rotation.restart() {
                                self.play_rotation_path(path);
                            } else {
                                self.persist();
                            }
                        }
                        ui.add_enabled_ui(!self.rotation.is_empty(), |ui| {
                            if ui.button("Skip").clicked() {
                                if let Some(path) = self.rotation.next_file() {
                                    self.play_rotation_path(path);
                                }
                            }
                        });
                        if ui.button("Clear").clicked() {
                            self.rotation.clear();
                            self.rotation_enabled = false;
                            self.persist();
                        }
                        ui.label(format!("{} files", self.rotation.len()));
                    });
                    ui.separator();
                    self.youtube_section(ui, ctx);
                } else {
                    self.library_tab(ui, ctx);
                }
                ui.separator();
                ui.label(&self.status);
            }); // ScrollArea
        });
        // Rotation tick (timer-driven; videos loop, no follow-video mode).
        self.rotation_tick();
        // Autopause poll (Python POLL_INTERVAL_MS): pause at once, resume
        // after 2 clean polls. Throttled — update() runs every ~50ms.
        if self.last_poll.elapsed()
            >= std::time::Duration::from_millis(wallmotion_core::autopause::POLL_INTERVAL_MS)
        {
            self.last_poll = std::time::Instant::now();
            self.autopause_tick();
        }
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
    let remote_rx = remote::start_server();
    let _tray = build_tray();
    debug_log(&format!(
        "main start action={action} server={} tray={}",
        remote_rx.is_some(),
        if _tray.is_some() { "ok" } else { "FAILED" }
    ));
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([480.0, 660.0])
        .with_taskbar(false)
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
            let mut app = App {
                remote_rx,
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
