"""Shared helpers: logging, paths, silence for FFmpeg."""

from __future__ import annotations

import ctypes
import os
import sys

from wallmotion.paths import log_path

DEBUG_LOG = str(log_path())

# Supported media extensions (shared by wallpaper picker, web library...).
# Kept here (Qt-free) so headless modules can use them without QtGui.
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".bmp", ".gif"}
VIDEO_EXTS = {".mp4", ".avi", ".mkv", ".mov", ".wmv", ".webm"}


def debug_log(msg: str) -> None:
    try:
        import datetime
        try:
            os.makedirs(os.path.dirname(DEBUG_LOG), exist_ok=True)
        except Exception:
            pass
        ts = datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]
        with open(DEBUG_LOG, "a", encoding="utf-8") as f:
            f.write(f"[{ts}] {msg}\n")
    except Exception:
        pass


def app_version() -> str:
    """Release tag (e.g. 'v1.0.6') or 'dev'.

    CI writes the git tag into assets/VERSION before packaging, so
    frozen builds know their own version; a source checkout without
    the file falls back to 'dev'.
    """
    try:
        with open(_asset_path("VERSION"), encoding="utf-8") as f:
            tag = f.read().strip()
        if tag:
            return tag
    except Exception:
        pass
    return "dev"


def _asset_path(name: str) -> str:
    """Path to a file in assets/ (also works in frozen EXE via _MEIPASS)."""
    try:
        meipass = getattr(sys, "_MEIPASS", None)
        if meipass:
            return os.path.join(meipass, "assets", name)
        # utils.py lives in wallmotion/, assets/ are in the repo root
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        return os.path.join(root, "assets", name)
    except Exception:
        return os.path.join("assets", name)


def _quiet_ffmpeg(level: int = 8) -> bool:
    """Silence native FFmpeg logs (Input #0, MFT, ...) that go directly to
    stderr outside Qt logging, so QT_LOGGING_RULES does not catch them.

    Explicitly loads the avutil DLL from the PySide6 folder and sets av_log_set_level.
    It is the same DLL instance later used by the Qt backend, so the setting
    also applies to playback. Level 8 = FATAL (silent, decoder errors stay
    hidden - OK for a wallpaper app).
    """
    versions = ("59", "60", "58", "57")
    if sys.platform == "win32":
        names = [f"avutil-{v}.dll" for v in versions]
    elif sys.platform == "darwin":
        names = [f"libavutil.{v}.dylib" for v in versions]
    else:
        names = [f"libavutil.so.{v}" for v in versions]
    candidates = []
    try:
        import PySide6 as _pyside

        _base = os.path.dirname(_pyside.__file__)
        candidates.extend(os.path.join(_base, n) for n in names)
    except Exception:
        pass
    candidates.extend(names)  # fallback: already loaded instance in the process
    for cand in candidates:
        try:
            dll = ctypes.CDLL(cand)
            dll.av_log_set_level(level)
            return True
        except Exception:
            continue
    return False


def _app_base_dir() -> str:
    """App base dir (delegates to wallmotion.paths, kept for compatibility)."""
    from wallmotion.paths import _app_base_dir as _base

    return str(_base())
