"""Windows backend: deleguje na stavajici GDI/WorkerW implementaci."""

from __future__ import annotations

from wallmotion import screens, wallpaper, win32
from wallmotion.platform import WallpaperBackend


class WindowsBackend(WallpaperBackend):
    name = "windows"

    def set_static_wallpaper(self, image_path: str) -> None:
        wallpaper.set_static_wallpaper(image_path)

    def measure_screens(self) -> dict:
        return screens.measure_screens()

    def get_workerw_handle(self):
        """WorkerW okno plochy, kam se vklada video (Windows-only)."""
        return win32.get_workerw_handle()
