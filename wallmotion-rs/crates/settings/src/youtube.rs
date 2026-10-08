//! YouTube sidecars: find/provision `yt-dlp` + `ffmpeg` and run
//! downloads on background threads.
//!
//! Design (why the Python version died for everyone): the Python app
//! drove the `yt_dlp` *Python module* frozen at build time — it went
//! stale within weeks and every old release shipped a dead downloader.
//! Here we drive the official standalone `yt-dlp` executable, stored in
//! a `tools/` dir next to the downloads folder, with one-click install
//! (`Get yt-dlp` pulls the official build from GitHub) and one-click
//! self-update (`yt-dlp -U`). `ffmpeg` is optional: without it we
//! download single-file formats (typically max ~720p); with it we get
//! merged 1080p H.264.
//!
//! Only std + wallmotion-core. Downloads use `curl.exe` (ships with
//! Windows 10+) with a PowerShell `Invoke-WebRequest` fallback, so no
//! new Rust dependencies are needed.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc, Mutex,
};
use wallmotion_core::youtube as yt;

/// Events from background YouTube workers to the UI thread.
#[derive(Debug)]
pub enum YtEvent {
    /// Download progress 0-100.
    Progress(f32),
    /// Playlist picker entries (titles only, no download yet).
    Playlist(Vec<yt::PlaylistEntry>),
    /// Download done, final file path.
    Finished(String),
    /// Mapped code (`NEED_FFMPEG`, `NEED_SIGNIN`, `UNAVAILABLE`,
    /// `TIMEOUT`, `EMPTY_PLAYLIST`) or raw stderr (truncated).
    Error(String),
    /// Tool provisioning/update finished (human-readable info).
    Tool(String),
}

/// Shared handle to a running download child (for Cancel).
pub type ChildSlot = Arc<Mutex<Option<std::process::Child>>>;

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

fn candidate_dirs() -> Vec<PathBuf> {
    vec![exe_dir(), yt::tools_dir()]
}

/// Locate the `yt-dlp` sidecar: `WALLMOTION_YTDLP` env, exe dir,
/// tools dir, then PATH. `None` = offer the one-click install.
pub fn find_ytdlp() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WALLMOTION_YTDLP") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = yt::ytdlp_exe_name();
    for dir in candidate_dirs() {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    find_on_path(name)
}

/// Locate `ffmpeg`: `WALLMOTION_FFMPEG` env, exe dir, tools dir, PATH.
pub fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WALLMOTION_FFMPEG") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = yt::ffmpeg_exe_name();
    for dir in candidate_dirs() {
        let cand = dir.join(name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    find_on_path(name)
}

/// Never pop a console window for child processes. The app itself is
/// `#![windows_subsystem = "windows"]`, but every spawned exe (yt-dlp,
/// ffmpeg, curl, powershell, tar) gets its own console unless told not to.
#[cfg(windows)]
pub fn hide_console(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

/// No-op off Windows.
#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut std::process::Command) {}

/// `yt-dlp --version`, trimmed first line. Best effort.
pub fn ytdlp_version(exe: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--version").stdin(std::process::Stdio::null());
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Spawn a single-video download on a background thread.
/// Streams `--newline` progress as [`YtEvent::Progress`].
pub fn spawn_download(
    ytdlp: PathBuf,
    url: String,
    out_dir: PathBuf,
    ffmpeg: Option<PathBuf>,
    slot: ChildSlot,
    cancel: Arc<AtomicBool>,
    tx: Sender<YtEvent>,
) {
    std::thread::spawn(move || {
        let ffmpeg_dir = ffmpeg
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()));
        let template = format!("{}/%(id)s.%(ext)s", out_dir.to_string_lossy());
        let mut args = yt::download_args(&template, ffmpeg_dir.as_deref());
        args.push(url.clone());
        let video_id = yt::extract_video_id(&url);
        let mut cmd = std::process::Command::new(&ytdlp);
        cmd.args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        hide_console(&mut cmd);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(YtEvent::Error(format!("cannot start yt-dlp: {e}")));
                return;
            }
        };
        // Drain stderr on a side thread so a full pipe never blocks us.
        let stderr_handle = child.stderr.take().map(|mut err| {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = err.read_to_string(&mut buf);
                buf
            })
        });
        let mut printed: Vec<String> = Vec::new();
        // Publish the live child so Cancel can kill it; take it back
        // once stdout closes (normal exit) or Cancel took it first.
        let stdout = child.stdout.take();
        *slot.lock().unwrap() = Some(child);
        if let Some(out) = stdout {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                if let Some(pct) = yt::progress_of_line(&line) {
                    let _ = tx.send(YtEvent::Progress(pct));
                } else {
                    let t = line.trim();
                    if !t.is_empty() && !t.starts_with('[') {
                        printed.push(t.to_string());
                    }
                }
            }
        }
        let status = slot.lock().unwrap().take().and_then(|mut c| c.wait().ok());
        let stderr = stderr_handle
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        if cancel.load(Ordering::SeqCst) {
            let _ = tx.send(YtEvent::Error("cancelled".to_string()));
            return;
        }
        let ok = status.map(|s| s.success()).unwrap_or(false);
        if ok {
            match yt::resolve_downloaded_path(&out_dir, &printed, video_id.as_deref()) {
                Some(path) => {
                    let _ = tx.send(YtEvent::Finished(path.to_string_lossy().into_owned()));
                }
                None => {
                    let _ = tx.send(YtEvent::Error("file not found".to_string()));
                }
            }
        } else {
            let tail: String = stderr.lines().rev().take(6).collect::<Vec<_>>().join(" | ");
            let _ = tx.send(YtEvent::Error(yt::map_download_error(
                &tail,
                ffmpeg.is_some(),
            )));
        }
    });
}

/// Spawn a playlist fetch (titles only) on a background thread.
pub fn spawn_playlist_fetch(
    ytdlp: PathBuf,
    url: String,
    cancel: Arc<AtomicBool>,
    tx: Sender<YtEvent>,
) {
    std::thread::spawn(move || {
        let mut args = yt::playlist_args();
        args.push(url);
        let mut cmd = std::process::Command::new(&ytdlp);
        cmd.args(&args).stdin(std::process::Stdio::null());
        hide_console(&mut cmd);
        let out = cmd.output();
        if cancel.load(Ordering::SeqCst) {
            let _ = tx.send(YtEvent::Error("cancelled".to_string()));
            return;
        }
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).into_owned();
                let entries = yt::parse_playlist_entries(&text, yt::MAX_PLAYLIST_ENTRIES);
                if entries.is_empty() {
                    let _ = tx.send(YtEvent::Error("EMPTY_PLAYLIST".to_string()));
                } else {
                    let _ = tx.send(YtEvent::Playlist(entries));
                }
            }
            Ok(o) => {
                let tail: String = String::from_utf8_lossy(&o.stderr)
                    .lines()
                    .rev()
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(" | ");
                let _ = tx.send(YtEvent::Error(yt::map_download_error(&tail, true)));
            }
            Err(e) => {
                let _ = tx.send(YtEvent::Error(format!("cannot start yt-dlp: {e}")));
            }
        }
    });
}

/// Download a file with `curl.exe`, falling back to PowerShell.
fn fetch_url(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    // Fast path: curl ships with Windows 10+.
    let mut curl = std::process::Command::new("curl.exe");
    curl.args(["-fL", "--connect-timeout", "20", "-o"])
        .arg(dest)
        .arg(url)
        .stdin(std::process::Stdio::null());
    hide_console(&mut curl);
    let curl = curl.output();
    if let Ok(o) = curl {
        if o.status.success() && dest.is_file() {
            return Ok(());
        }
    }
    // Fallback: PowerShell.
    let ps = format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
        url,
        dest.to_string_lossy().replace('\'', "''")
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &ps])
        .stdin(std::process::Stdio::null());
    hide_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("download failed (curl+powershell): {e}"))?;
    if out.status.success() && dest.is_file() {
        Ok(())
    } else {
        Err("download failed (network or URL)".to_string())
    }
}

/// One-click install/update of the yt-dlp sidecar into `tools/`.
pub fn spawn_provision_ytdlp(tx: Sender<YtEvent>) {
    std::thread::spawn(move || {
        let dir = yt::tools_dir();
        let dest = dir.join(yt::ytdlp_exe_name());
        match fetch_url(yt::YTDLP_DOWNLOAD_URL, &dest) {
            Ok(()) => match ytdlp_version(&dest) {
                Some(v) => {
                    let _ = tx.send(YtEvent::Tool(format!("yt-dlp ready ({v})")));
                }
                None => {
                    let _ = tx.send(YtEvent::Error(
                        "downloaded yt-dlp does not run (blocked?)".to_string(),
                    ));
                }
            },
            Err(e) => {
                let _ = tx.send(YtEvent::Error(e));
            }
        }
    });
}

/// `yt-dlp -U` self-update (fixes stale builds — the Python killer).
pub fn spawn_ytdlp_update(exe: PathBuf, tx: Sender<YtEvent>) {
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("-U").stdin(std::process::Stdio::null());
        hide_console(&mut cmd);
        let out = cmd.output();
        match out {
            Ok(o) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                let last = text.lines().rev().find(|l| !l.trim().is_empty());
                match (o.status.success(), ytdlp_version(&exe)) {
                    (true, Some(v)) => {
                        let _ = tx.send(YtEvent::Tool(format!("yt-dlp updated ({v})")));
                    }
                    _ => {
                        let _ =
                            tx.send(YtEvent::Error(last.unwrap_or("update failed").to_string()));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(YtEvent::Error(format!("update failed: {e}")));
            }
        }
    });
}

/// Find `bin/ffmpeg.exe` inside an extracted tree (max depth 4).
fn find_extracted_ffmpeg(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 4 {
            continue;
        }
        let entries = std::fs::read_dir(&dir).ok()?;
        for e in entries.map_while(Result::ok) {
            let p = e.path();
            if p.is_file()
                && p.file_name()
                    .map(|n| n == yt::ffmpeg_exe_name())
                    .unwrap_or(false)
            {
                return Some(p);
            }
            if p.is_dir() {
                stack.push((p, depth + 1));
            }
        }
    }
    None
}

/// One-click ffmpeg install: download essentials zip, extract with
/// `tar.exe` (ships with Windows), move `ffmpeg.exe` into `tools/`.
pub fn spawn_provision_ffmpeg(tx: Sender<YtEvent>) {
    std::thread::spawn(move || {
        let fail = |m: String| {
            let _ = tx.send(YtEvent::Error(m));
        };
        let tmp = std::env::temp_dir().join("wallmotion-ffmpeg-dl");
        let _ = std::fs::remove_dir_all(&tmp);
        if std::fs::create_dir_all(&tmp).is_err() {
            fail("cannot create temp dir".to_string());
            return;
        }
        let zip = tmp.join("ffmpeg.zip");
        if let Err(e) = fetch_url(yt::FFMPEG_DOWNLOAD_URL, &zip) {
            fail(e);
            return;
        }
        let mut tar = std::process::Command::new("tar.exe");
        tar.args(["-xf"])
            .arg(&zip)
            .args(["-C"])
            .arg(&tmp)
            .stdin(std::process::Stdio::null());
        hide_console(&mut tar);
        let out = tar.output();
        if !out.map(|o| o.status.success()).unwrap_or(false) {
            fail("cannot unpack ffmpeg.zip".to_string());
            return;
        }
        let found = find_extracted_ffmpeg(&tmp);
        let found = match found {
            Some(p) => p,
            None => {
                fail("ffmpeg.exe not found in archive".to_string());
                return;
            }
        };
        let dest = yt::tools_dir().join(yt::ffmpeg_exe_name());
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&dest);
        if std::fs::rename(&found, &dest).is_err() {
            fail("cannot install ffmpeg.exe".to_string());
            return;
        }
        let _ = std::fs::remove_dir_all(&tmp);
        // Verify it actually runs (SmartScreen can block it).
        let mut verify = std::process::Command::new(&dest);
        verify.arg("-version").stdin(std::process::Stdio::null());
        hide_console(&mut verify);
        let runs = verify.output().map(|o| o.status.success()).unwrap_or(false);
        if runs {
            let _ = tx.send(YtEvent::Tool("ffmpeg ready (1080p merges on)".to_string()));
        } else {
            let _ = tx.send(YtEvent::Error(
                "ffmpeg.exe is blocked from running".to_string(),
            ));
        }
    });
}

/// Human text for a mapped error code (UI language: English).
pub fn error_text(code: &str) -> String {
    match code {
        "NEED_FFMPEG" => "Merge needs ffmpeg (Get ffmpeg below, or single-file ~720p).".to_string(),
        "NEED_SIGNIN" => "YouTube asks for sign-in for this video.".to_string(),
        "UNAVAILABLE" => "Video unavailable or private.".to_string(),
        "TIMEOUT" => "Network timeout, try again.".to_string(),
        "EMPTY_PLAYLIST" => "Playlist is empty.".to_string(),
        "cancelled" => "Cancelled.".to_string(),
        other => format!("Download error: {other}"),
    }
}

/// Cancel a running download child, if any.
pub fn cancel_child(slot: &ChildSlot, flag: &Arc<AtomicBool>) {
    flag.store(true, Ordering::SeqCst);
    if let Some(mut child) = slot.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn new_cancel_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

pub fn new_child_slot() -> ChildSlot {
    Arc::new(Mutex::new(None))
}
