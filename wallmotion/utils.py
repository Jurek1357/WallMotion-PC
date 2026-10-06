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


def bundle_dir() -> str:
    """Directory holding bundled data (assets/, locales/).

    PyInstaller onefile: sys._MEIPASS (temp unpack dir). Nuitka onefile
    and source runs: repository/payload root derived from __file__.
    Frozen exe directory is the last resort (PyInstaller --onedir style
    layouts where data sits next to the binary).
    """
    try:
        meipass = getattr(sys, "_MEIPASS", None)
        if meipass:
            return str(meipass)
    except Exception:
        pass
    try:
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        if os.path.isdir(os.path.join(root, "assets")):
            return root
    except Exception:
        pass
    try:
        if getattr(sys, "frozen", False):
            return os.path.dirname(os.path.abspath(sys.executable))
    except Exception:
        pass
    try:
        return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    except Exception:
        return ""


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
    """Path to a file in assets/ (works in frozen EXE and source runs)."""
    try:
        base = bundle_dir()
        if base:
            return os.path.join(base, "assets", name)
    except Exception:
        pass
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
