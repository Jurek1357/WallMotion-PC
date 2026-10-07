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

use config::AppConfig;

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
        }
    }
}

impl App {
    fn persist(&self) {
        AppConfig {
            last_path: self.file.clone(),
            muted: self.muted,
            volume: self.volume,
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
        self.status = "Stopped.".to_string();
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
    fn set_image(&mut self, path: &std::path::Path) {
        use wallmotion_win::{canvas::sys as canvas, wallpaper};
        let (w, h) = canvas::primary_size();
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
            let (w, h) = canvas::primary_size();
            let wc = match canvas::setup_wallpaper_canvas(0, 0, w, h) {
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
                    self.status = format!("Playing behind icons ({w}x{h}).");
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
            ui.separator();
            ui.label(&self.status);
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_video();
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 340.0])
            .with_title("WallMotion (native)"),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "WallMotion (native)",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    ) {
        eprintln!("settings error: {e:?}");
    }
}
