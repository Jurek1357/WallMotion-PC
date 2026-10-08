//! YouTube downloads via a standalone yt-dlp sidecar.
//!
//! Port of `wallmotion/youtube.py`, re-architected so it works for
//! everyone who downloads the app:
//!
//! - Python drove the `yt_dlp` **Python module** bundled at build time.
//!   It went stale within weeks (YouTube breaks old yt-dlp regularly)
//!   and every frozen release shipped a dead downloader. Rust drives
//!   the official standalone **`yt-dlp` executable** instead, with a
//!   one-click in-app download + `yt-dlp -U` self-update.
//! - `ffmpeg` was silently missing in the frozen app (never collected
//!   by PyInstaller), so merges failed. Rust degrades gracefully to
//!   single-file formats without it and offers a one-click install.
//!
//! This module is pure logic (no processes): URL validation, playlist
//! parsing, progress/error mapping, CLI arg builders. Spawning lives in
//! the settings crate; unit tests cover everything here.

use std::path::{Path, PathBuf};

/// Reject URLs longer than this (mirrors Python `MAX_YT_URL_LENGTH`).
pub const MAX_URL_LENGTH: usize = 2048;
/// Max playlist entries loaded by the picker (mirrors Python).
pub const MAX_PLAYLIST_ENTRIES: usize = 50;
/// Guard against GB-sized videos (mirrors Python `max_filesize`).
pub const MAX_FILESIZE: &str = "500M";

/// Standalone yt-dlp executable (official builds, no Python needed).
pub const YTDLP_DOWNLOAD_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
/// Static ffmpeg essentials build (contains `bin/ffmpeg.exe`).
pub const FFMPEG_DOWNLOAD_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

/// Only H.264/AVC up to 1080p, always with audio (merged tracks,
/// needs ffmpeg). Same branches as the Python `_YT_FORMAT_MERGED`.
pub const FORMAT_MERGED: &str = "bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba[acodec^=mp4a]/\
    bv*[vcodec^=avc1][height<=1080]+ba[acodec^=mp4a]/\
    bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba[acodec!=none]/\
    b[vcodec^=avc1][acodec^=mp4a][height<=1080][ext=mp4]/\
    bv*[vcodec^=avc1][height<=1080]+ba[acodec!=none]/\
    b[vcodec^=avc1][acodec!=none][height<=1080]";
/// Single file with audio, no merging (no ffmpeg available).
/// Same branches as the Python `_YT_FORMAT_SINGLE`.
pub const FORMAT_SINGLE: &str = "b[vcodec^=avc1][acodec^=mp4a][height<=1080][ext=mp4]/\
    b[vcodec^=avc1][acodec!=none][height<=1080][ext=mp4]/\
    b[vcodec^=avc1][acodec^=mp4a][height<=1080]/\
    b[vcodec^=avc1][acodec!=none][height<=1080]";

/// One playlist entry from `--print "%(id)s\t%(title)s"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub id: String,
    pub title: String,
    pub url: String,
}

fn split_scheme(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        Some((scheme, rest))
    } else {
        None
    }
}

/// Lowercase host without userinfo/port. `None` on malformed authority.
fn host_of(rest: &str) -> Option<String> {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return None;
    }
    // Strip userinfo.
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    // IPv6 literal in brackets.
    if let Some(stripped) = hostport.strip_prefix('[') {
        let end = stripped.find(']')?;
        return Some(stripped[..end].to_lowercase());
    }
    // Strip :port (single colon only; more colons = bare IPv6).
    let host = if hostport.matches(':').count() == 1 {
        hostport.split(':').next().unwrap_or_default()
    } else {
        hostport
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

fn is_ipv4_literal(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
}

fn is_youtube_host(host: &str) -> bool {
    host == "youtube.com" || host.ends_with(".youtube.com")
}

fn is_short_host(host: &str) -> bool {
    host == "youtu.be" || host.ends_with(".youtu.be")
}

fn is_nocookie_host(host: &str) -> bool {
    host == "youtube-nocookie.com" || host.ends_with(".youtube-nocookie.com")
}

/// Path + query of the URL (everything after the authority).
fn path_and_query(rest: &str) -> &str {
    match rest.find(['/', '?', '#']) {
        Some(i) => &rest[i..],
        None => "",
    }
}

fn path_of(rest: &str) -> &str {
    let pq = path_and_query(rest);
    match pq.find(['?', '#']) {
        Some(i) => &pq[..i],
        None => pq,
    }
}

/// Check that the URL is a safe YouTube link before passing to yt-dlp.
/// Same rules as Python `is_valid_youtube_url` (domains, no IPs,
/// no localhost, video/playlist paths only).
pub fn is_valid_youtube_url(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_URL_LENGTH {
        return false;
    }
    if url.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    let (_scheme, rest) = match split_scheme(url) {
        Some(p) => p,
        None => return false,
    };
    let host = match host_of(rest) {
        Some(h) => h,
        None => return false,
    };
    if host == "localhost" {
        return false;
    }
    if is_ipv4_literal(&host) || host.contains(':') {
        return false;
    }
    let path = path_of(rest);
    let pq = path_and_query(rest);
    if is_short_host(&host) {
        return !path.strip_prefix('/').unwrap_or_default().is_empty()
            && !host_contains_extra_path(path);
    }
    if !is_youtube_host(&host) && !is_nocookie_host(&host) {
        return false;
    }
    if is_nocookie_host(&host) {
        return path.starts_with("/embed/") && path.len() > "/embed/".len();
    }
    if path.starts_with("/watch")
        || path.starts_with("/shorts/")
        || path.starts_with("/embed/")
        || path.starts_with("/live/")
        || path.starts_with("/v/")
    {
        return true;
    }
    if path.starts_with("/playlist") && pq.contains("list=") {
        return true;
    }
    if pq.contains("v=") {
        return true;
    }
    false
}

/// youtu.be IDs live in a single path segment; reject nested paths.
fn host_contains_extra_path(path: &str) -> bool {
    path.strip_prefix('/').unwrap_or_default().contains('/')
}

/// True for `/playlist` URLs (picker flow). A `/watch` URL with `&list=`
/// downloads just that single video (mirrors Python).
pub fn is_playlist_url(url: &str) -> bool {
    if !is_valid_youtube_url(url) {
        return false;
    }
    let rest = url.trim().split_once("://").map(|p| p.1).unwrap_or("");
    path_of(rest).starts_with("/playlist")
}

/// Best-effort video id from a watch/shorts/youtu.be URL (for the
/// downloaded-file fallback scan). `None` for playlists/unknown shapes.
pub fn extract_video_id(url: &str) -> Option<String> {
    let url = url.trim();
    let (_scheme, rest) = split_scheme(url)?;
    let host = host_of(rest)?;
    let path = path_of(rest).to_string();
    let pq = path_and_query(rest).to_string();
    let valid_id = |s: &str| {
        let s = s.split(['&', '#', '?', '/']).next().unwrap_or_default();
        (!s.is_empty()
            && s.len() <= 32
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
        .then(|| s.to_string())
    };
    if is_short_host(&host) {
        let id = path.strip_prefix('/').unwrap_or_default();
        return valid_id(id);
    }
    if !is_youtube_host(&host) {
        return None;
    }
    for marker in ["/shorts/", "/embed/", "/live/", "/v/"] {
        if let Some(after) = path.strip_prefix(marker) {
            if let Some(id) = valid_id(after) {
                return Some(id);
            }
        }
    }
    // query `v=` param
    for part in pq.split(['?', '&']) {
        if let Some(v) = part.strip_prefix("v=") {
            if let Some(id) = valid_id(v) {
                return Some(id);
            }
        }
    }
    None
}

/// Parse `--print "%(id)s\t%(title)s"` output into entries.
/// Pure (no network); skips log lines and entries without id.
pub fn parse_playlist_entries(text: &str, limit: usize) -> Vec<PlaylistEntry> {
    let mut out = Vec::new();
    for line in text.lines().take(limit.max(1)) {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        let (id, title) = match line.split_once('\t') {
            Some((id, title)) => (id.trim(), title.trim()),
            None => (line.trim(), line.trim()),
        };
        if id.is_empty() || id.starts_with('[') {
            continue;
        }
        let title = if title.is_empty() { id } else { title };
        out.push(PlaylistEntry {
            id: id.to_string(),
            title: title.to_string(),
            url: format!("https://www.youtube.com/watch?v={id}"),
        });
        if out.len() >= limit.max(1) {
            break;
        }
    }
    out
}

/// `' 42.5%' -> 42.5` for the progress bar. Pure.
pub fn parse_progress_percent(text: &str) -> Option<f32> {
    let cleaned = text.trim().strip_suffix('%').unwrap_or(text.trim());
    let cleaned = cleaned.trim();
    cleaned
        .parse::<f32>()
        .ok()
        .filter(|v| (0.0..=100.0).contains(v))
}

/// A progress percent from one `--newline` yt-dlp stdout line, if any.
/// Progress lines look like `[download]  42.5% of ~ 12.3MiB ...`.
pub fn progress_of_line(line: &str) -> Option<f32> {
    let line = line.trim();
    if !line.starts_with("[download]") {
        return None;
    }
    line.split_whitespace()
        .find(|tok| tok.ends_with('%'))
        .and_then(parse_progress_percent)
}

/// Map raw yt-dlp stderr to a short code. Anything else passes through
/// (truncated). `ffmpeg_present` decides the ambiguous "Requested format
/// is not available" case (mirrors Python `map_download_error`).
pub fn map_download_error(err: &str, ffmpeg_present: bool) -> String {
    if err.contains("Requested format is not available") && !ffmpeg_present {
        return "NEED_FFMPEG".to_string();
    }
    if err.contains("ffmpeg is not installed") {
        return "NEED_FFMPEG".to_string();
    }
    if err.contains("Sign in to confirm") {
        return "NEED_SIGNIN".to_string();
    }
    if err.contains("Private video") || err.contains("Video unavailable") {
        return "UNAVAILABLE".to_string();
    }
    let lowered = err.to_lowercase();
    if lowered.contains("timed out") || lowered.contains("timeout") {
        return "TIMEOUT".to_string();
    }
    err.chars().take(300).collect()
}

/// CLI args for a single-video download (without the exe or URL).
/// `--print` lines carry the final filepath (after merge/move when
/// ffmpeg ran); progress is parsed from `--newline` stdout lines.
pub fn download_args(out_template: &str, ffmpeg_dir: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--no-playlist".to_string(),
        "--socket-timeout".to_string(),
        "25".to_string(),
        "--retries".to_string(),
        "3".to_string(),
        "--fragment-retries".to_string(),
        "3".to_string(),
        "--max-filesize".to_string(),
        MAX_FILESIZE.to_string(),
        "-f".to_string(),
        if ffmpeg_dir.is_some() {
            FORMAT_MERGED.to_string()
        } else {
            FORMAT_SINGLE.to_string()
        },
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "-o".to_string(),
        out_template.to_string(),
        "--newline".to_string(),
        "--no-warnings".to_string(),
        "--print".to_string(),
        "after_move:filepath".to_string(),
        "--print".to_string(),
        "filepath".to_string(),
    ];
    if let Some(dir) = ffmpeg_dir {
        args.push("--ffmpeg-location".to_string());
        args.push(dir.to_string());
    }
    args
}

/// CLI args for the playlist picker (titles only, no download).
pub fn playlist_args() -> Vec<String> {
    vec![
        "--flat-playlist".to_string(),
        "--no-warnings".to_string(),
        "--socket-timeout".to_string(),
        "25".to_string(),
        "--print".to_string(),
        "%(id)s\t%(title)s".to_string(),
        "--playlist-end".to_string(),
        MAX_PLAYLIST_ENTRIES.to_string(),
        "--no-download".to_string(),
    ]
}

/// Find the actual downloaded file: last `--print`ed path that exists,
/// else `<dir>/<video_id>.<ext>` (post-merge candidates), else nothing.
pub fn resolve_downloaded_path(
    dir: &Path,
    printed: &[String],
    video_id: Option<&str>,
) -> Option<PathBuf> {
    for line in printed.iter().rev() {
        let cand = PathBuf::from(line.trim());
        if !line.trim().is_empty() && cand.is_file() {
            return Some(cand);
        }
    }
    if let Some(id) = video_id {
        for ext in ["mp4", "mkv", "webm", "avi", "mov"] {
            let cand = dir.join(format!("{id}.{ext}"));
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Sibling `tools/` dir next to the downloads dir (exe-adjacent on
/// Windows, XDG data on Linux). Holds provisioned `yt-dlp`/`ffmpeg`.
pub fn tools_dir() -> PathBuf {
    let downloads = crate::paths::app_dirs().downloads;
    downloads
        .parent()
        .map(|p| p.join("tools"))
        .unwrap_or_else(|| PathBuf::from("tools"))
}

/// Sidecar binary name for this platform.
pub fn ytdlp_exe_name() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// ffmpeg binary name for this platform.
pub fn ffmpeg_exe_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_video_urls() {
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(is_valid_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/shorts/abc123XYZ_-"
        ));
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/embed/dQw4w9WgXcQ"
        ));
        assert!(is_valid_youtube_url(
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"
        ));
        assert!(is_valid_youtube_url(
            "https://www.youtube.com/playlist?list=PLabc123"
        ));
        assert!(is_valid_youtube_url("https://m.youtube.com/watch?v=abc"));
    }

    #[test]
    fn rejects_bad_urls() {
        assert!(!is_valid_youtube_url(""));
        assert!(!is_valid_youtube_url("not a url"));
        assert!(!is_valid_youtube_url("ftp://youtube.com/watch?v=abc"));
        assert!(!is_valid_youtube_url("https://vimeo.com/123"));
        assert!(!is_valid_youtube_url("https://youtube.com/"));
        assert!(!is_valid_youtube_url("file:///etc/passwd"));
        assert!(!is_valid_youtube_url("http://localhost:8080/watch?v=abc"));
        assert!(!is_valid_youtube_url("https://127.0.0.1/watch?v=abc"));
        assert!(!is_valid_youtube_url("https://youtu.be/"));
        assert!(!is_valid_youtube_url(
            "https://www.youtube-nocookie.com/watch?v=abc"
        ));
        assert!(!is_valid_youtube_url("https://fakeyoutube.com/watch?v=abc"));
        assert!(!is_valid_youtube_url(
            "https://youtube.com.evil.com/watch?v=abc"
        ));
    }

    #[test]
    fn playlist_detection() {
        assert!(is_playlist_url(
            "https://www.youtube.com/playlist?list=PLabc123"
        ));
        // watch+list downloads just the single video (no picker)
        assert!(!is_playlist_url(
            "https://www.youtube.com/watch?v=abc&list=PLabc123"
        ));
        assert!(!is_playlist_url("https://youtu.be/abc123"));
    }

    #[test]
    fn video_id_extraction() {
        assert_eq!(
            extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            extract_video_id("https://www.youtube.com/shorts/abc123"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_video_id("https://www.youtube.com/playlist?list=PLx"),
            None
        );
    }

    #[test]
    fn playlist_parsing() {
        let text = "[youtube:tab] PLx: Downloading 3 videos\nabc\tFirst video\ndef\tSecond\n";
        let entries = parse_playlist_entries(text, 50);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "https://www.youtube.com/watch?v=abc");
        assert_eq!(entries[1].title, "Second");
    }

    #[test]
    fn progress_parsing() {
        assert_eq!(parse_progress_percent(" 42.5%"), Some(42.5));
        assert_eq!(parse_progress_percent("100%"), Some(100.0));
        assert_eq!(parse_progress_percent("oops"), None);
        assert_eq!(
            progress_of_line("[download]  42.5% of ~ 12.34MiB at 1.2MiB/s ETA 00:07"),
            Some(42.5)
        );
        assert_eq!(progress_of_line("C:\\vids\\abc.mp4"), None);
    }

    #[test]
    fn error_mapping() {
        assert_eq!(
            map_download_error("ERROR: Requested format is not available", false),
            "NEED_FFMPEG"
        );
        // With ffmpeg present the same text is a real format miss.
        assert!(
            map_download_error("ERROR: Requested format is not available", true)
                .contains("Requested format")
        );
        assert_eq!(
            map_download_error("ERROR: Sign in to confirm your age", true),
            "NEED_SIGNIN"
        );
        assert_eq!(
            map_download_error("ERROR: Private video", true),
            "UNAVAILABLE"
        );
    }

    #[test]
    fn arg_builders() {
        let args = download_args("C:/d/%(id)s.%(ext)s", Some("C:/t"));
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&FORMAT_MERGED.to_string()));
        assert!(args.contains(&"--ffmpeg-location".to_string()));
        let args = download_args("C:/d/%(id)s.%(ext)s", None);
        assert!(args.contains(&FORMAT_SINGLE.to_string()));
        assert!(!args.contains(&"--ffmpeg-location".to_string()));
        let pl = playlist_args();
        assert!(pl.contains(&"--flat-playlist".to_string()));
    }
}
