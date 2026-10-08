//! Local media library: browse downloaded wallpapers in-app.
//!
//! The Python app serves the library as a localhost web page
//! (`wallmotion/webui.py`, ffmpeg thumbnails + browser grid). The native
//! app does it better: the grid lives directly in the settings window,
//! no browser needed. Scanning + naming mirrors the Python side
//! (`list_media`, `format_size`, `safe_name`).

use std::path::{Path, PathBuf};

/// Video extensions (lowercase, with dot).
pub const VIDEO_EXTS: &[&str] = &[
    ".mp4", ".mkv", ".webm", ".avi", ".mov", ".m4v", ".ogv", ".ts",
];

/// Image extensions (lowercase, with dot).
pub const IMAGE_EXTS: &[&str] = &[".jpg", ".jpeg", ".png", ".bmp", ".gif", ".webp"];

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
}

/// Image or video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Video => "video",
        }
    }
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

/// Downloads dir holding the library (same as the Python app).
pub fn media_dir() -> PathBuf {
    wallmotion_core::paths::app_dirs().downloads
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
            let size = std::fs::metadata(&full).ok()?.len();
            kind_of(&name).map(|kind| MediaItem { name, kind, size })
        })
        .collect()
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

/// ffmpeg binary from `PATH`, if any.
pub fn ffmpeg_exe() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
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
    let status = std::process::Command::new(&ffmpeg)
        .args([
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
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
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
