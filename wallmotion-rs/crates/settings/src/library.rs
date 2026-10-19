//! Local media library: browse downloaded wallpapers in-app.
//!
//! The Python app serves the library as a localhost web page
//! (`wallmotion/webui.py`, ffmpeg thumbnails + browser grid). The native
//! app does it better: the grid lives directly in the settings window,
//! no browser needed. Scanning + naming mirrors the Python side
//! (`list_media`, `format_size`, `safe_name`).

use std::path::{Path, PathBuf};

/// Video extensions (lowercase, with dot). GIF rides along: mpv/ffmpeg
/// decodes it animated, so it plays as video, not a still image.
pub const VIDEO_EXTS: &[&str] = &[
    ".mp4", ".mkv", ".webm", ".avi", ".mov", ".m4v", ".ogv", ".ts", ".gif",
];

/// Image extensions (lowercase, with dot).
pub const IMAGE_EXTS: &[&str] = &[".jpg", ".jpeg", ".png", ".bmp", ".webp"];

/// Thumbnail width in pixels; height follows the aspect ratio.
pub const THUMB_WIDTH: u32 = 320;

/// Video frame position for thumbnails, in seconds.
pub const THUMB_SECONDS: u64 = 1;

/// One media file in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    pub name: String,
    pub kind: MediaKind,
    pub size: u64,
    /// Last-modified time (sorting newest-first; None when unreadable).
    pub modified: Option<std::time::SystemTime>,
}

/// Image or video (labels are localized in the UI layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

fn kind_of(name: &str) -> Option<MediaKind> {
    let lower = name.to_lowercase();
    let dot = lower.rfind('.').map(|i| &lower[i..])?;
    if VIDEO_EXTS.contains(&dot) {
        Some(MediaKind::Video)
    } else if IMAGE_EXTS.contains(&dot) {
        Some(MediaKind::Image)
    } else {
        None
    }
}

/// `2.0 MB` style label. Mirrors Python `format_size`.
pub fn format_size(size: u64) -> String {
    if size >= 1_048_576 {
        format!("{:.1} MB", size as f64 / 1_048_576.0)
    } else {
        format!("{} KB", (size / 1024).max(1))
    }
}

/// Downloads dir holding the library.
///
/// The Python app keeps videos next to its exe (`dist/downloads` frozen,
/// `downloads/` from sources) while the native exe used to look next to
/// *its own* exe (`target/release/downloads` in dev) — an empty folder,
/// so the gallery stayed empty. Resolution order, first hit wins:
/// 1. `WALLMOTION_DOWNLOADS` env override (must exist),
/// 2. exe-adjacent `downloads/` when it holds media,
/// 3. legacy Python spots (`dist/downloads`, `downloads/`) found by
///    walking up from the exe dir, first one holding media,
/// 4. first existing dir of the above,
/// 5. exe-adjacent `downloads/` (fresh users: created on first download).
pub fn media_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("WALLMOTION_DOWNLOADS") {
        let p = PathBuf::from(p);
        if p.is_dir() {
            return p;
        }
    }
    let local = exe_adjacent_downloads();
    let mut cands = vec![local.clone()];
    cands.extend(legacy_download_dirs(&exe_dir()));
    if let Some(dir) = cands.iter().find(|d| has_media(d)) {
        return dir.clone();
    }
    if let Some(dir) = cands.iter().find(|d| d.is_dir()) {
        return dir.clone();
    }
    local
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn exe_adjacent_downloads() -> PathBuf {
    exe_dir().join("downloads")
}

/// Legacy Python download spots: `<root>/dist/downloads` (frozen exe)
/// and `<root>/downloads` (sources), where `<root>` is any ancestor of
/// `exe` (dev layout: `.../wallpaper_app/wallmotion-rs/target/release`).
fn legacy_download_dirs(exe: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut anc = Some(exe);
    for _ in 0..6 {
        let Some(dir) = anc else { break };
        for cand in [dir.join("dist").join("downloads"), dir.join("downloads")] {
            if cand.is_dir() && !out.contains(&cand) {
                out.push(cand);
            }
        }
        anc = dir.parent();
    }
    out
}

fn has_media(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                e.path().is_file() && kind_of(&name).is_some()
            })
        })
        .unwrap_or(false)
}

/// Cache dir for generated video thumbnails.
pub fn thumbs_dir() -> PathBuf {
    std::env::temp_dir().join("wallmotion-thumbs")
}

/// Sorted `[{name, kind, size}]` of images/videos in `directory`.
/// Mirrors Python `list_media`.
pub fn list_media(directory: &Path) -> Vec<MediaItem> {
    let mut names: Vec<String> = match std::fs::read_dir(directory) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_file() && kind_of(&name).is_some() {
                    Some(name)
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return vec![],
    };
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let full = directory.join(&name);
            let meta = std::fs::metadata(&full).ok()?;
            let size = meta.len();
            let modified = meta.modified().ok();
            kind_of(&name).map(|kind| MediaItem {
                name,
                kind,
                size,
                modified,
            })
        })
        .collect()
}

/// Result of importing files dropped onto the window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    /// Files copied into the library.
    pub added: usize,
    /// Non-media files, folders, or files already in the library.
    pub skipped: usize,
    /// Copy failures (permission, disk, …).
    pub failed: usize,
}

/// Copy dropped files into `dest` (the library folder).
///
/// Only video/image extensions are accepted (same set as the gallery);
/// a file already sitting in `dest` is skipped, and a name clash gets a
/// `name (2).ext` suffix so nothing is overwritten. `dest` is created
/// when missing. Never panics.
pub fn import_files(dest: &Path, sources: &[PathBuf]) -> ImportReport {
    let mut report = ImportReport::default();
    if std::fs::create_dir_all(dest).is_err() {
        report.failed = sources.len();
        return report;
    }
    let base = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());
    for src in sources {
        let Some(file_name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            report.failed += 1;
            continue;
        };
        if !src.is_file() || kind_of(&file_name).is_none() {
            report.skipped += 1;
            continue;
        }
        // Already inside the library folder (dropped from the folder
        // itself): nothing to copy.
        let in_place = src
            .canonicalize()
            .ok()
            .and_then(|p| p.parent().map(|d| d == base))
            .unwrap_or(false);
        if in_place {
            report.skipped += 1;
            continue;
        }
        let target = unique_target(dest, &file_name);
        match std::fs::copy(src, &target) {
            Ok(_) => report.added += 1,
            Err(_) => report.failed += 1,
        }
    }
    report
}

/// `dest/name` when free, else `stem (2).ext`, `stem (3).ext`, …
fn unique_target(dest: &Path, name: &str) -> PathBuf {
    let first = dest.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 2..1000 {
        let candidate = match &ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let full = dest.join(candidate);
        if !full.exists() {
            return full;
        }
    }
    first
}

/// Absolute path for a library file name, or None on traversal/missing.
/// Mirrors Python `safe_name`.
pub fn safe_path(directory: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || Path::new(name).file_name()?.to_string_lossy() != name {
        return None;
    }
    let base = directory.canonicalize().ok()?;
    let full = base.join(name);
    if full.parent() != Some(base.as_path()) || !full.is_file() {
        return None;
    }
    Some(full)
}

/// ffmpeg binary (exe dir, tools dir or `PATH`), if any.
/// Shared with the YouTube downloader so installed ffmpeg also
/// unlocks video thumbnails here.
pub fn ffmpeg_exe() -> Option<PathBuf> {
    crate::youtube::find_ffmpeg()
}

/// Generate (once) a 320px JPEG thumbnail for a video. Returns the cached
/// path, or None without ffmpeg / on failure. Mirrors `ensure_thumbnail`.
pub fn ensure_video_thumb(video: &Path, cache_dir: &Path) -> Option<PathBuf> {
    let ffmpeg = ffmpeg_exe()?;
    let stem = video.file_stem()?.to_string_lossy().into_owned();
    std::fs::create_dir_all(cache_dir).ok()?;
    let out = cache_dir.join(format!("{stem}.jpg"));
    let fresh = out.is_file()
        && out.metadata().and_then(|m| m.modified()).ok()
            >= video.metadata().and_then(|m| m.modified()).ok();
    if fresh {
        return Some(out);
    }
    let mut cmd = std::process::Command::new(&ffmpeg);
    cmd.args([
        "-y",
        "-v",
        "error",
        "-ss",
        &THUMB_SECONDS.to_string(),
        "-i",
        &video.to_string_lossy(),
        "-frames:v",
        "1",
        "-vf",
        &format!("scale={THUMB_WIDTH}:-1"),
        &out.to_string_lossy(),
    ])
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
    crate::youtube::hide_console(&mut cmd);
    let status = cmd.status().ok()?;
    if status.success() && out.is_file() {
        Some(out)
    } else {
        None
    }
}

/// Load an image file as RGBA thumbnail (width capped at 320px).
/// Used for real images and cached video JPEGs. No panic on bad input.
pub fn load_thumb_rgba(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::open(path).ok()?;
    let img = img.thumbnail(THUMB_WIDTH, THUMB_WIDTH * 9);
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Some((rgba.into_raw(), w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("wallmotion-lib-test-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.mp4"), b"video").unwrap();
        std::fs::write(dir.join("a.jpg"), b"image").unwrap();
        std::fs::write(dir.join("note.txt"), b"skip me").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        dir
    }

    #[test]
    fn lists_only_media_sorted() {
        let dir = fixture();
        let items = list_media(&dir);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "a.jpg");
        assert_eq!(items[0].kind, MediaKind::Image);
        assert_eq!(items[1].name, "b.mp4");
        assert_eq!(items[1].kind, MediaKind::Video);
        assert!(items[0].size > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dir_is_empty() {
        assert!(list_media(Path::new("/definitely/not/here")).is_empty());
    }

    #[test]
    fn sizes_shaped() {
        assert_eq!(format_size(512), "1 KB");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(2_097_152), "2.0 MB");
    }

    #[test]
    fn safe_path_blocks_traversal() {
        let dir = fixture();
        assert!(safe_path(&dir, "a.jpg").is_some());
        assert!(safe_path(&dir, "../a.jpg").is_none());
        assert!(safe_path(&dir, "nope.jpg").is_none());
        assert!(safe_path(&dir, "").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn has_media_detects_files() {
        let dir = fixture();
        assert!(has_media(&dir));
        let empty = std::env::temp_dir().join(format!(
            "wallmotion-lib-empty-{}",
            std::sync::atomic::AtomicU64::new(0).fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&empty).unwrap();
        assert!(!has_media(&empty));
        assert!(!has_media(Path::new("/definitely/not/here")));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn legacy_dirs_found_by_walking_up() {
        // Fake dev layout: <tmp>/wallpaper_app/wallmotion-rs/target/release
        // with videos in <tmp>/wallpaper_app/dist/downloads.
        let base = std::env::temp_dir().join("wallmotion-lib-legacy");
        let _ = std::fs::remove_dir_all(&base);
        let exe = base.join("wallpaper_app/wallmotion-rs/target/release");
        let legacy = base.join("wallpaper_app/dist/downloads");
        std::fs::create_dir_all(&exe).unwrap();
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("a.mp4"), b"video").unwrap();
        let found = legacy_download_dirs(&exe);
        assert!(found.contains(&legacy));
        assert!(found.iter().any(|p| has_media(p)));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn env_override_wins() {
        let dir = fixture();
        let saved = std::env::var_os("WALLMOTION_DOWNLOADS");
        std::env::set_var("WALLMOTION_DOWNLOADS", &dir);
        assert_eq!(media_dir(), dir);
        match saved {
            Some(v) => std::env::set_var("WALLMOTION_DOWNLOADS", v),
            None => std::env::remove_var("WALLMOTION_DOWNLOADS"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_copies_media_only() {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let src = std::env::temp_dir().join(format!("wallmotion-import-src-{n}"));
        let dest = std::env::temp_dir().join(format!("wallmotion-import-dest-{n}"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.mp4"), b"video").unwrap();
        std::fs::write(src.join("b.jpg"), b"image").unwrap();
        std::fs::write(src.join("note.txt"), b"skip").unwrap();
        let report = import_files(
            &dest,
            &[src.join("a.mp4"), src.join("b.jpg"), src.join("note.txt")],
        );
        assert_eq!(
            report,
            ImportReport {
                added: 2,
                skipped: 1,
                failed: 0
            }
        );
        assert!(dest.join("a.mp4").is_file());
        assert!(dest.join("b.jpg").is_file());
        assert!(!dest.join("note.txt").exists());
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn import_skips_in_place_and_renames_clash() {
        let dest = std::env::temp_dir().join("wallmotion-import-clash");
        let other = std::env::temp_dir().join("wallmotion-import-other");
        let _ = std::fs::remove_dir_all(&dest);
        let _ = std::fs::remove_dir_all(&other);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(dest.join("a.mp4"), b"orig").unwrap();
        std::fs::write(other.join("a.mp4"), b"new").unwrap();
        // Dropping the library file itself: already there -> skipped.
        let r1 = import_files(&dest, &[dest.join("a.mp4")]);
        assert_eq!(
            r1,
            ImportReport {
                added: 0,
                skipped: 1,
                failed: 0
            }
        );
        // Same name from elsewhere: renamed, the original stays intact.
        let r2 = import_files(&dest, &[other.join("a.mp4")]);
        assert_eq!(
            r2,
            ImportReport {
                added: 1,
                skipped: 0,
                failed: 0
            }
        );
        assert_eq!(std::fs::read(dest.join("a.mp4")).unwrap(), b"orig");
        assert_eq!(std::fs::read(dest.join("a (2).mp4")).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&dest);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn import_creates_missing_dest() {
        let dest = std::env::temp_dir().join("wallmotion-import-fresh");
        let _ = std::fs::remove_dir_all(&dest);
        let report = import_files(&dest, &[]);
        assert!(dest.is_dir());
        assert_eq!(report, ImportReport::default());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn bad_image_is_none() {
        let dir = fixture();
        assert!(load_thumb_rgba(&dir.join("a.jpg")).is_none()); // not a real jpeg
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_image_loads_thumb() {
        let dir = std::env::temp_dir().join("wallmotion-lib-img");
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::DynamicImage::new_rgb8(640, 360);
        let path = dir.join("w.png");
        img.save(&path).unwrap();
        let (rgba, w, h) = load_thumb_rgba(&path).unwrap();
        assert_eq!(w, 320);
        assert_eq!(rgba.len(), w as usize * h as usize * 4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
