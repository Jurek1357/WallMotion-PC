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

mod config;
mod instance;

use config::AppConfig;

const TRAY_SHOW_ID: &str = "show";
const TRAY_QUIT_ID: &str = "quit";

/// Set by the tray thread on Quit; the UI thread performs the close
/// (with video cleanup) on its next frame.
static TRAY_QUIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Blocking menu loop: instant response even when the UI thread is
/// throttled while hidden. Only signals; all Qt/egui work stays on
/// the UI thread.
fn tray_menu_thread(ctx: egui::Context) {
    std::thread::spawn(move || {
        use std::sync::atomic::Ordering;
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().recv() {
            match event.id.0.as_str() {
                TRAY_SHOW_ID => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.request_repaint();
                }
                TRAY_QUIT_ID => {
                    TRAY_QUIT.store(true, Ordering::SeqCst);
                    ctx.request_repaint();
                }
                _ => {}
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

impl Default for App {
    fn default() -> Self {
        let saved = AppConfig::load();
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
        }
    }
}

impl App {
    fn persist(&self) {
        AppConfig {
            last_path: self.file.clone(),
            muted: self.muted,
            volume: self.volume,
            monitor: self.monitor.clone(),
            pause_on_fullscreen: self.pause_on_fullscreen,
            pause_on_battery: self.pause_on_battery,
        }
        .save();
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
        // Tray menu actions from the icon thread.
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                TRAY_SHOW_ID => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
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
        // X/red close hides to tray (video keeps playing); only the
        // tray Quit action lets the close proceed (on_exit stops video).
        if !self.quit_requested && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WallMotion (native)");
            ui.label(format!("Backend: {}", wallmotion_win::backend_name()));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Video file:");
                ui.text_edit_singleline(&mut self.file);
                if ui.button("Browse…").clicked() {
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
                            .selectable_value(&mut self.monitor, String::new(), "All monitors")
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
            ui.label(&self.status);
        });
        // Autopause poll (Python POLL_INTERVAL_MS): pause at once, resume
        // after 2 clean polls. Throttled — update() runs every ~50ms.
        if self.last_poll.elapsed()
            >= std::time::Duration::from_millis(wallmotion_core::autopause::POLL_INTERVAL_MS)
        {
            self.last_poll = std::time::Instant::now();
            self.autopause_tick();
        }
        // Keep the loop alive while hidden in the tray: without a
        // periodic repaint, update() never runs and tray clicks die.
        // 50ms keeps the menu snappy at negligible idle cost.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_video();
    }
}

fn tray_icon_rgba() -> Vec<u8> {
    // 32x32 accent tile with a darker border (no asset dependency).
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

fn build_tray() -> Option<tray_icon::TrayIcon> {
    use tray_icon::{menu::Menu, Icon, TrayIconBuilder};
    let menu = Menu::new();
    let show = tray_icon::menu::MenuItem::with_id(TRAY_SHOW_ID, "Open", true, None);
    let quit = tray_icon::menu::MenuItem::with_id(TRAY_QUIT_ID, "Quit", true, None);
    menu.append(&show).ok()?;
    menu.append(&quit).ok()?;
    let icon = Icon::from_rgba(tray_icon_rgba(), 32, 32).ok()?;
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("WallMotion (native)")
        .build()
        .ok()
}

fn main() {
    if instance::another_instance_running() {
        return;
    }
    let _tray = build_tray();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 340.0])
            .with_title("WallMotion (native)"),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "WallMotion (native)",
        options,
        Box::new(|cc| {
            tray_menu_thread(cc.egui_ctx.clone());
            Ok(Box::new(App::default()))
        }),
    ) {
        eprintln!("settings error: {e:?}");
    }
}
