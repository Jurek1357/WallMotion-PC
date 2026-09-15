"""Spolecne pomocne funkce: logovani, cesty, ticho pro FFmpeg."""

from __future__ import annotations

import ctypes
import os
import sys
import tempfile

DEBUG_LOG = os.path.join(tempfile.gettempdir(), "live_wallpaper_debug.log")


def debug_log(msg: str) -> None:
    try:
        import datetime
        ts = datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]
        with open(DEBUG_LOG, "a", encoding="utf-8") as f:
            f.write(f"[{ts}] {msg}\n")
    except Exception:
        pass


def _asset_path(name: str) -> str:
    """Cesta k souboru v assets/ (funguje i ve zmrazenem EXE pres _MEIPASS)."""
    try:
        meipass = getattr(sys, "_MEIPASS", None)
        if meipass:
            return os.path.join(meipass, "assets", name)
        # utils.py lezi ve wallmotion/, assets/ jsou v koreni repozitare
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        return os.path.join(root, "assets", name)
    except Exception:
        return os.path.join("assets", name)


def _quiet_ffmpeg(level: int = 8) -> bool:
    """Ztisi nativni FFmpeg logy (Input #0, MFT, ...), ktere jdou primo na
    stderr mimo Qt logovani, takze je QT_LOGGING_RULES nechyti.

    Explicitne nacte avutil DLL ze slozky PySide6 a nastavi av_log_set_level.
    Je to stejna instance DLL, jakou pak pouzije Qt backend, takze nastaveni
    plati i pro prehravani. Uroven 8 = FATAL (ticho, chyby dekoderu zustanou
    skryte - pro tapetovou appku OK).
    """
    names = ["avutil-59.dll", "avutil-60.dll", "avutil-58.dll", "avutil-57.dll"]
    candidates = []
    try:
        import PySide6 as _pyside

        _base = os.path.dirname(_pyside.__file__)
        candidates.extend(os.path.join(_base, n) for n in names)
    except Exception:
        pass
    candidates.extend(names)  # fallback: uz nactena instance v procesu
    for cand in candidates:
        try:
            dll = ctypes.CDLL(cand)
            dll.av_log_set_level(level)
            return True
        except Exception:
            continue
    return False


def _app_base_dir() -> str:
    """Adresar se spustitelnym souborem (u EXE) nebo se zdrojakem.

    U zmrazeneho EXE (onefile) se nesmi pouzit _MEIPASS (docasny rozbalovaci
    adresar) - videa patri vedle EXE. Ze zdrojaku vedle repozitare.
    """
    try:
        if getattr(sys, "frozen", False):
            return os.path.dirname(os.path.abspath(sys.executable))
    except Exception:
        pass
    # wallmotion/utils.py -> koren repozitare (kde lezi main.py)
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
