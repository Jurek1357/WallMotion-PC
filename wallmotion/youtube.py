"""Stahovani videa z YouTube (yt-dlp) na pozadi, at nezamrzne UI."""

from __future__ import annotations

import ipaddress
import os
import shutil
import urllib.parse

from PySide6.QtCore import QThread, Signal

from wallmotion.utils import _app_base_dir

YT_DIR = os.path.join(_app_base_dir(), "downloads")

# F1: pouzivat smi jen http(s) odkazy na zname YouTube domeny.
MAX_YT_URL_LENGTH = 2048


def is_valid_youtube_url(url: str) -> bool:
    """Overi, ze URL je bezpecny YouTube odkaz, nez se preda yt-dlp.

    Povolene jsou jen legitimni YouTube domeny (youtube.com + subdomeny,
    youtu.be, youtube-nocookie.com). Odmita se: prazdne/nepodporovane URL,
    jine schema nez http/https (napr. file://), localhost, hole IP adresy
    a prilis dlouhe URL.
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
        # Odmitnout raw IPv4/IPv6 adresy (vcetne 127.0.0.1 apod.).
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
        # youtu.be/<id> - musi obsahovat ID videa
        return len(path.strip("/")) > 0
    if is_nocookie:
        # youtube-nocookie.com se pouziva jen jako /embed/<id>
        return path.startswith("/embed/") and len(path) > len("/embed/")
    # youtube.com: jen stranky videi (watch/shorts/embed/live/v)
    if path.startswith(("/watch", "/shorts/", "/embed/", "/live/", "/v/")):
        return True
    if path.startswith("/playlist") and "list=" in query:
        return True
    # /watch muze prijit i s jinou cestou, rozhoduje parametr v=
    if "v=" in query:
        return True
    return False


def resolve_downloaded_path(info: dict, ydl) -> str:
    """Najde skutecny soubor stazeneho videa (po pripadnem mergu)."""
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
    """Cesta k ffmpeg (slouceni oddelenych stop), nebo None.

    Hleda systemovy ffmpeg v PATH a jako zalohu volitelny balik
    imageio-ffmpeg (pip install imageio-ffmpeg).
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
    """Zjisti, zda je k dispozici ffmpeg (potreba pro slouceni bv+ba)."""
    return _ffmpeg_exe() is not None


# F10 + zvuk: jen H.264/AVC (avc1), max 1080p, vzdy se zvukovou stopou.
# MERGED (vyzaduje ffmpeg.exe): 1080p sloucene ze zvlastnich stop, AAC prvni.
_YT_FORMAT_MERGED = (
    "bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba[acodec^=mp4a]/"
    "bv*[vcodec^=avc1][height<=1080]+ba[acodec^=mp4a]/"
    "bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba[acodec!=none]/"
    "b[vcodec^=avc1][acodec^=mp4a][height<=1080][ext=mp4]/"
    "bv*[vcodec^=avc1][height<=1080]+ba[acodec!=none]/"
    "b[vcodec^=avc1][acodec!=none][height<=1080]"
)
# SINGLE (bez ffmpeg.exe): jen jeden soubor se zvukem, bez slucovani.
_YT_FORMAT_SINGLE = (
    "b[vcodec^=avc1][acodec^=mp4a][height<=1080][ext=mp4]/"
    "b[vcodec^=avc1][acodec!=none][height<=1080][ext=mp4]/"
    "b[vcodec^=avc1][acodec^=mp4a][height<=1080]/"
    "b[vcodec^=avc1][acodec!=none][height<=1080]"
)


class DownloadWorker(QThread):
    progress = Signal(str)
    finished = Signal(str)
    error = Signal(str)

    def __init__(self, url: str, parent=None):
        super().__init__(parent)
        self.url = url.strip()

    def run(self):
        # F1 (obrana do hloubky): URL znovu overit i ve vlakne, nez se
        # preda yt-dlp.
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
                # F10: vyzadovat H.264/AVC (avc1) a max. 1080p. Vsechny
                # volby maji filtr vcodec^=avc1 + height<=1080, takze se
                # nikdy nestahne AV1 ani VP9 (Qt/FFmpeg backend z nich
                # nedostane ani snimek). Kdyz neni AVC k dispozici,
                # stahovani schvalne selze, misto aby stahlo neprehratelne
                # video.
                # Zvuk: kazda vetev vyzaduje audio stopu (sloucene
                # bv+ba, nebo jeden soubor s acodec!=none), aby stazene
                # video melo zvuk - prehravani pak ridi checkbox Ztlumit.
                # Audio se preferuje AAC (mp4a), ktere Qt na Windows
                # prehraje spolehlive; opus az jako zalozni.
                # Bez ffmpeg.exe nelze slucovat oddelene stopy (yt-dlp
                # by skoncilo chybou postprocessingu a nevratilo se
                # k dalsi volbe), proto se bez nej stahuje jen jeden
                # soubor se zvukem (progresivni, typicky max 720p).
                "format": (
                    _YT_FORMAT_MERGED if _ffmpeg_available()
                    else _YT_FORMAT_SINGLE
                ),
                "outtmpl": os.path.join(YT_DIR, "%(id)s.%(ext)s"),
                "merge_output_format": "mp4",
                "quiet": True,
                "no_warnings": True,
                "noplaylist": True,
                "noprogress": True,  # vlastni progress posilame signálem
                "max_filesize": 500 * 1024 * 1024,  # pojistka proti GB videim
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
            # Nektera videa (jako tohle) nemaji zadny jeden soubor
            # s obrazem i zvukem - zvuk jde jen sloucit pres ffmpeg.
            # Bez nej misto krypticke hlasky posleme pokyn k instalaci.
            if "Requested format is not available" in str(e) and not _ffmpeg_available():
                self.error.emit("NEED_FFMPEG")
            else:
                self.error.emit(str(e)[:300])
