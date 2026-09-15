"""Staticka tapeta (obrazek): nastaveni pres Windows API + fit na obrazovku."""

from __future__ import annotations

import ctypes
import os
import tempfile

from PySide6.QtCore import Qt
from PySide6.QtGui import QImage

IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".bmp", ".gif"}
VIDEO_EXTS = {".mp4", ".avi", ".mkv", ".mov", ".wmv", ".webm"}


def set_static_wallpaper(image_path: str) -> None:
    SPI_SETDESKWALLPAPER = 20
    SPIF_UPDATEINIFILE = 0x01
    SPIF_SENDCHANGE = 0x02
    ctypes.windll.user32.SystemParametersInfoW(
        SPI_SETDESKWALLPAPER, 0, image_path, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE
    )


def get_current_wallpaper() -> str:
    SPI_GETDESKWALLPAPER = 0x73
    buf = ctypes.create_unicode_buffer(260)
    ctypes.windll.user32.SystemParametersInfoW(SPI_GETDESKWALLPAPER, 260, buf, 0)
    return buf.value


def fit_image_to_screen(image_path: str, width: int, height: int) -> str:
    """Upravi obrazek presne na rozmer obrazovky (cover + oriznuti na stred)
    a ulozi ho do docasneho BMP. Vrati cestu k upravenemu souboru,
    pri chybe vrati puvodni cestu."""
    try:
        img = QImage(image_path)
        if img.isNull() or width <= 0 or height <= 0:
            return image_path
        scaled = img.scaled(
            width, height,
            Qt.AspectRatioMode.KeepAspectRatioByExpanding,
            Qt.TransformationMode.SmoothTransformation,
        )
        x = max(0, (scaled.width() - width) // 2)
        y = max(0, (scaled.height() - height) // 2)
        cropped = scaled.copy(x, y, width, height)
        out = os.path.join(tempfile.gettempdir(), "live_wallpaper_fitted.bmp")
        if cropped.save(out, "BMP"):
            return out
    except Exception:
        pass
    return image_path
