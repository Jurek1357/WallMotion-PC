"""Download YouTube video (yt-dlp) in background so UI does not freeze."""

from __future__ import annotations

import ipaddress
import os
import shutil
import urllib.parse

from PySide6.QtCore import QThread, Signal

from wallmotion.paths import downloads_dir

YT_DIR = str(downloads_dir())

# F1: only allow http(s) links to known YouTube domains.
MAX_YT_URL_LENGTH = 2048

# Playlist picker: max entries loaded from playlist (extract_flat, no download).
MAX_PLAYLIST_ENTRIES = 50


def is_valid_youtube_url(url: str) -> bool:
    """Check that URL is a safe YouTube link before passing to yt-dlp.

    Only legitimate YouTube domains allowed (youtube.com + subdomains,
    youtu.be, youtube-nocookie.com). Rejected: empty/unsupported URL,
    non-http/https scheme (e.g. file://), localhost, bare IP addresses
    and overly long URLs.
    """
    if not url or not isinstance(url, str):
        return False
    url = url.strip()
    if not url or len(url) > MAX_YT_URL_LENGTH:
        return False
    if any(ch.isspace() for ch in url):
        return False
    try:
        parsed = urllib.parse.urlparse(url)
    except Exception:
        return False
    if parsed.scheme not in ("http", "https"):
        return False
    host = (parsed.hostname or "").lower()
    if not host:
        return False
    if host == "localhost":
        return False
    try:
        # Reject raw IPv4/IPv6 addresses (incl. 127.0.0.1 etc.).
        ipaddress.ip_address(host)
        return False
    except ValueError:
        pass
    is_youtube = host == "youtube.com" or host.endswith(".youtube.com")
    is_short = host == "youtu.be" or host.endswith(".youtu.be")
    is_nocookie = (
        host == "youtube-nocookie.com"
        or host.endswith(".youtube-nocookie.com")
    )
    if not (is_youtube or is_short or is_nocookie):
        return False
    path = parsed.path or ""
    query = parsed.query or ""
    if is_short:
        # youtu.be/<id> - must contain video ID
        return len(path.strip("/")) > 0
    if is_nocookie:
        # youtube-nocookie.com is only used as /embed/<id>
        return path.startswith("/embed/") and len(path) > len("/embed/")
    # youtube.com: only video pages (watch/shorts/embed/live/v)
    if path.startswith(("/watch", "/shorts/", "/embed/", "/live/", "/v/")):
        return True
    if path.startswith("/playlist") and "list=" in query:
        return True
    # /watch may come with a different path, the v= param decides
    if "v=" in query:
        return True
    return False


def is_playlist_url(url: str) -> bool:
    """True when the URL points to a playlist (picker flow).

    Only /playlist URLs open the picker. A /watch URL with &list=
    downloads just that single video (current behavior) - predictable
    and free of surprise bulk downloads.
    """
    if not is_valid_youtube_url(url):
        return False
    try:
        path = urllib.parse.urlparse(url.strip()).path or ""
    except Exception:
        return False
    return path.startswith("/playlist")


def parse_playlist_entries(info: dict, limit: int = MAX_PLAYLIST_ENTRIES) -> list:
    """Extract [{id, title, url}] from extract_flat playlist info.

    Pure function (no network) - unit tested. Skips entries without id.
    """
    entries = []
    try:
        raw = info.get("entries") or []
    except Exception:
        return entries
    for e in list(raw)[: max(0, int(limit))]:
        try:
            if not isinstance(e, dict):
                continue
            vid = e.get("id")
            if not vid:
                continue
            title = e.get("title") or vid
            entries.append({
                "id": vid,
                "title": title,
                "url": f"https://www.youtube.com/watch?v={vid}",
            })
        except Exception:
            continue
    return entries


class PlaylistFetchWorker(QThread):
    """Fetch playlist entries (titles only, no download) on a background thread."""

    loaded = Signal(list)
    error = Signal(str)

    def __init__(self, url: str, parent=None):
        super().__init__(parent)
        self.url = url.strip()

    def run(self):
        if not is_playlist_url(self.url):
            self.error.emit("not a playlist URL")
            return
        try:
            import yt_dlp
        except ImportError:
            self.error.emit("yt-dlp není nainstalované (pip install yt-dlp)")
            return
        try:
            opts = {
                "quiet": True,
                "no_warnings": True,
                "extract_flat": True,  # list only, no download
                "noplaylist": False,
                "playlistend": MAX_PLAYLIST_ENTRIES,
                "skip_download": True,
            }
            with yt_dlp.YoutubeDL(opts) as ydl:
                info = ydl.extract_info(self.url, download=False)
            entries = parse_playlist_entries(info or {})
            if entries:
                self.loaded.emit(entries)
            else:
                self.error.emit("EMPTY_PLAYLIST")
        except Exception as e:
            self.error.emit(str(e)[:300])


def resolve_downloaded_path(info: dict, ydl) -> str:
    """Find the actual downloaded video file (after possible merge)."""
    try:
        req = info.get("requested_downloads")
        if req:
            fp = req[0].get("filepath")
            if fp and os.path.exists(fp):
                return fp
    except Exception:
        pass
    try:
        vid = info.get("id", "video")
        for ext in ("mp4", "mkv", "webm", "avi", "mov"):
            cand = os.path.join(YT_DIR, f"{vid}.{ext}")
            if os.path.exists(cand):
                return cand
    except Exception:
        pass
    try:
        return ydl.prepare_filename(info)
    except Exception:
        return ""


def _ffmpeg_exe() -> str | None:
    """Path to ffmpeg (for merging split tracks), or None.

    Looks for system ffmpeg on PATH and as fallback the optional
    imageio-ffmpeg package (pip install imageio-ffmpeg).
    """
    try:
        found = shutil.which("ffmpeg")
        if found:
            return found
    except Exception:
        pass
    try:
        import imageio_ffmpeg
        exe = imageio_ffmpeg.get_ffmpeg_exe()
        if exe and os.path.exists(exe):
            return exe
    except Exception:
        pass
    return None


def _ffmpeg_available() -> bool:
    """Check whether ffmpeg is available (needed for merging bv+ba)."""
    return _ffmpeg_exe() is not None


# F10 + audio: only H.264/AVC (avc1), max 1080p, always with audio track.
# MERGED (requires ffmpeg.exe): 1080p merged from separate tracks, AAC first.
_YT_FORMAT_MERGED = (
    "bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba[acodec^=mp4a]/"
    "bv*[vcodec^=avc1][height<=1080]+ba[acodec^=mp4a]/"
    "bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba[acodec!=none]/"
    "b[vcodec^=avc1][acodec^=mp4a][height<=1080][ext=mp4]/"
    "bv*[vcodec^=avc1][height<=1080]+ba[acodec!=none]/"
    "b[vcodec^=avc1][acodec!=none][height<=1080]"
)
# SINGLE (no ffmpeg.exe): single file with audio only, no merging.
_YT_FORMAT_SINGLE = (
    "b[vcodec^=avc1][acodec^=mp4a][height<=1080][ext=mp4]/"
    "b[vcodec^=avc1][acodec!=none][height<=1080][ext=mp4]/"
    "b[vcodec^=avc1][acodec^=mp4a][height<=1080]/"
    "b[vcodec^=avc1][acodec!=none][height<=1080]"
)


def parse_progress_percent(text: str) -> float | None:
    """' 42.5%' -> 42.5 for the progress bar. Pure, unit-tested."""
    try:
        cleaned = str(text or "").strip()
        if cleaned.endswith("%"):
            cleaned = cleaned[:-1].strip()
        value = float(cleaned)
        if 0.0 <= value <= 100.0:
            return value
    except Exception:
        pass
    return None


def map_download_error(err: str) -> str:
    """Map raw yt-dlp errors to short codes. Pure, unit-tested.

    Codes (resolved to locale strings in the UI): NEED_FFMPEG,
    NEED_SIGNIN, UNAVAILABLE, TIMEOUT. Anything else passes through.
    """
    text = str(err or "")
    if "Requested format is not available" in text and not _ffmpeg_available():
        return "NEED_FFMPEG"
    if "Sign in to confirm" in text:
        return "NEED_SIGNIN"
    if "Private video" in text or "Video unavailable" in text:
        return "UNAVAILABLE"
    lowered = text.lower()
    if "timed out" in lowered or "timeout" in lowered:
        return "TIMEOUT"
    return text[:300]


class DownloadWorker(QThread):
    progress = Signal(str)
    finished = Signal(str)
    error = Signal(str)

    def __init__(self, url: str, parent=None):
        super().__init__(parent)
        self.url = url.strip()

    def run(self):
        # F1 (defense in depth): re-validate URL in thread before
        # passing to yt-dlp.
        if not is_valid_youtube_url(self.url):
            self.error.emit("neplatný YouTube odkaz")
            return
        try:
            import yt_dlp
        except ImportError:
            self.error.emit("yt-dlp není nainstalované (pip install yt-dlp)")
            return
        try:
            os.makedirs(YT_DIR, exist_ok=True)

            def hook(d):
                try:
                    if d.get("status") == "downloading":
                        pct = (d.get("_percent_str") or "").strip()
                        self.progress.emit(pct)
                except Exception:
                    pass

            opts = {
                # F10: require H.264/AVC (avc1) and max 1080p. All
                # options filter vcodec^=avc1 + height<=1080, so
                # AV1 or VP9 is never downloaded (Qt/FFmpeg backend gets
                # not even a frame from them). When AVC is unavailable,
                # download intentionally fails instead of fetching
                # unplayable video.
                # Audio: every branch requires an audio track (merged
                # bv+ba, or single file with acodec!=none), so the
                # downloaded video has sound - playback is then driven
                # by the Mute checkbox.
                # Audio prefers AAC (mp4a), which Qt plays reliably
                # on Windows; opus only as fallback.
                # Without ffmpeg.exe separate tracks cannot be merged
                # (yt-dlp would end with a postprocessing error and not
                # fall through to the next option), so without it only
                # a single file with audio is downloaded (progressive,
                # typically max 720p).
                "format": (
                    _YT_FORMAT_MERGED if _ffmpeg_available()
                    else _YT_FORMAT_SINGLE
                ),
                "outtmpl": os.path.join(YT_DIR, "%(id)s.%(ext)s"),
                "merge_output_format": "mp4",
                "quiet": True,
                "no_warnings": True,
                "noplaylist": True,
                "noprogress": True,  # custom progress sent via signal
                "max_filesize": 500 * 1024 * 1024,  # guard against GB-sized videos
                # Fail fast instead of hanging forever on throttled
                # connections (YouTube stalls googlevideo streams rather
                # than refusing them).
                "socket_timeout": 25,
                "retries": 3,
                "fragment_retries": 3,
                "progress_hooks": [hook],
            }
            ffexe = _ffmpeg_exe()
            if ffexe:
                try:
                    opts["ffmpeg_location"] = os.path.dirname(os.path.abspath(ffexe))
                except Exception:
                    pass
            with yt_dlp.YoutubeDL(opts) as ydl:
                info = ydl.extract_info(self.url, download=True)
                path = resolve_downloaded_path(info, ydl)
            if path and os.path.exists(path):
                self.finished.emit(path)
            else:
                self.error.emit("soubor se nenasel")
        except Exception as e:
            self.error.emit(map_download_error(e))
