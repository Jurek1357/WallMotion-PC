"""Platform backend seam: rozhrani tapety pro jednotlive OS.

Windows backend pouziva stavajici GDI/WorkerW trik (wallmotion.win32).
Linux/macOS backendy se doplni sem, aniz by se sahalo na zbytek aplikace:
UI, stahovani, konfigurace i jazyky jsou uz dnes multiplatformni.
"""

from __future__ import annotations

import sys
from abc import ABC, abstractmethod


class WallpaperBackend(ABC):
    """Rozhrani, ktere musi splnit backend kazde platformy."""

    name: str = "base"

    @abstractmethod
    def set_static_wallpaper(self, image_path: str) -> None:
        """Nastavi staticky obrazek jako tapetu plochy."""

    @abstractmethod
    def measure_screens(self) -> dict:
        """Zmeri obrazovky (stejny format jako wallmotion.screens)."""


def get_backend() -> WallpaperBackend:
    """Vrati backend pro aktualni OS. Nezname OS vyhodi vyjimku."""
    if sys.platform == "win32":
        from wallmotion.platform.windows import WindowsBackend
        return WindowsBackend()
    raise NotImplementedError(
        f"Platforma {sys.platform} zatim nema backend (viz TODO.md)"
    )
