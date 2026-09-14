"""
Live Wallpaper App
-------------------
Jednoducha Windows aplikace s prehlednym rozhranim, ktera umi nastavit
jako pozadi plochy:
  - staticky obrazek (jpg, png, bmp, ...)
  - video (mp4, avi, mkv, mov, ...), ktere se poté porad prehrava
    na pozadi za ikonami plochy (jako Wallpaper Engine / Lively).

Princip zive video tapety:
  Windows ma skryte okno "Progman", ktere po poslani zpravy 0x052C
  vytvori dalsi skryte okno "WorkerW". Toto WorkerW okno lezi mezi
  plochou (SHELLDLL_DefView, kde jsou ikony) a pozadim. Kdyz do nej
  vlozime vlastni okno s prehravanym videem, vypada to jako by video
  bylo primo pozadim plochy.

Pozadavky (requirements.txt):
    pip install -r requirements.txt

Spusteni:
    python main.py
"""

import sys
import os
import ctypes
import ipaddress
import json
import tempfile
import time
import urllib.parse

from PySide6.QtCore import Qt, QUrl, Signal, QLoggingCategory, QThread, QTimer
from PySide6.QtGui import QIcon, QAction, QPixmap, QImage, QDragEnterEvent, QDropEvent
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFileDialog, QSystemTrayIcon, QMenu,
    QMessageBox, QCheckBox, QFrame, QComboBox, QLineEdit
)
from PySide6.QtMultimedia import QMediaPlayer, QAudioOutput, QVideoSink, QVideoFrame

import win32gui
import win32con
import win32api

# --- GDI konstanty pro vykreslovani videa primo na plochu -------------------
_GDI32 = ctypes.windll.gdi32
_SRCCOPY = 0x00CC0020
_DIB_RGB_COLORS = 0
_HALFTONE = 4
_COLORONCOLOR = 3
_BLACKNESS = 0x00000042


class _BitmapInfoHeader(ctypes.Structure):
    _fields_ = [
        ("biSize", ctypes.c_uint32),
        ("biWidth", ctypes.c_int32),
        ("biHeight", ctypes.c_int32),
        ("biPlanes", ctypes.c_uint16),
        ("biBitCount", ctypes.c_uint16),
        ("biCompression", ctypes.c_uint32),
        ("biSizeImage", ctypes.c_uint32),
        ("biXPelsPerMeter", ctypes.c_int32),
        ("biYPelsPerMeter", ctypes.c_int32),
        ("biClrUsed", ctypes.c_uint32),
        ("biClrImportant", ctypes.c_uint32),
    ]


class _BitmapInfo(ctypes.Structure):
    _fields_ = [("header", _BitmapInfoHeader), ("palette", ctypes.c_uint32)]


DEBUG_LOG = os.path.join(tempfile.gettempdir(), "live_wallpaper_debug.log")


def debug_log(msg: str) -> None:
    try:
        import datetime
        ts = datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]
        with open(DEBUG_LOG, "a", encoding="utf-8") as f:
            f.write(f"[{ts}] {msg}\n")
    except Exception:
        pass


try:
    _GDI32.StretchDIBits.argtypes = [
        ctypes.c_void_p,
        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint,
    ]
    _GDI32.StretchDIBits.restype = ctypes.c_int
    _GDI32.PatBlt.argtypes = [
        ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_uint
    ]
    _GDI32.PatBlt.restype = ctypes.c_int
    _GDI32.SetStretchBltMode.argtypes = [ctypes.c_void_p, ctypes.c_int]
    _GDI32.SetStretchBltMode.restype = ctypes.c_int
except Exception:
    pass


# --- Nativni Win32 platno pro video (misto Qt okna) ---------------------------
_USER32 = ctypes.windll.user32
_WS_CHILD = 0x40000000
_WS_VISIBLE = 0x10000000
_WS_CLIPCHILDREN = 0x02000000
_WS_EX_NOACTIVATE = 0x08000000
_WS_EX_TRANSPARENT = 0x00000020  # kliky prochazeji skrz na plochu
_WS_EX_TOOLWINDOW = 0x00000080
_WS_EX_LAYERED = 0x00080000
_WS_EX_NOREDIRECTIONBITMAP = 0x00200000
_LWA_ALPHA = 0x00000002
_GWL_EXSTYLE = -20

try:
    _USER32.CreateWindowExW.argtypes = [
        ctypes.c_uint32, ctypes.c_wchar_p, ctypes.c_wchar_p, ctypes.c_uint32,
        ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int,
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
    ]
    _USER32.CreateWindowExW.restype = ctypes.c_void_p
    _USER32.DestroyWindow.argtypes = [ctypes.c_void_p]
    _USER32.DestroyWindow.restype = ctypes.c_int
    _USER32.SetLayeredWindowAttributes.argtypes = [
        ctypes.c_void_p, ctypes.c_uint32, ctypes.c_ubyte, ctypes.c_uint32
    ]
    _USER32.SetLayeredWindowAttributes.restype = ctypes.c_int
    _USER32.GetWindowLongW.argtypes = [ctypes.c_void_p, ctypes.c_int]
    _USER32.GetWindowLongW.restype = ctypes.c_long
except Exception:
    pass


_CANVAS_CLASS = "WallMotionCanvas"
_CS_OWNDC = 0x0020


class _WndClassEx(ctypes.Structure):
    _fields_ = [
        ("cbSize", ctypes.c_uint),
        ("style", ctypes.c_uint),
        ("lpfnWndProc", ctypes.c_void_p),
        ("cbClsExtra", ctypes.c_int),
        ("cbWndExtra", ctypes.c_int),
        ("hInstance", ctypes.c_void_p),
        ("hIcon", ctypes.c_void_p),
        ("hCursor", ctypes.c_void_p),
        ("hbrBackground", ctypes.c_void_p),
        ("lpszMenuName", ctypes.c_wchar_p),
        ("lpszClassName", ctypes.c_wchar_p),
        ("hIconSm", ctypes.c_void_p),
    ]


def _ensure_canvas_class() -> bool:
    """Zaregistruje vlastni tridu platna: vlastni DC (OWNDC) a ZADNE mazani
    pozadi (NULL brush). Klasicke STATIC by pri kazdem prekresleni blikalo
    bile; nase platno malujeme kompletne sami kazdy snimek pres GDI."""
    try:
        if _USER32.GetClassInfoW(None, _CANVAS_CLASS, None):
            return True
    except Exception:
        pass
    try:
        _USER32.DefWindowProcW.argtypes = [
            ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p
        ]
        _USER32.DefWindowProcW.restype = ctypes.c_void_p
        _USER32.GetModuleHandleW.argtypes = [ctypes.c_wchar_p]
        _USER32.GetModuleHandleW.restype = ctypes.c_void_p
        wc = _WndClassEx()
        wc.cbSize = ctypes.sizeof(_WndClassEx)
        wc.style = _CS_OWNDC
        wc.lpfnWndProc = _USER32.DefWindowProcW
        wc.hInstance = _USER32.GetModuleHandleW(None)
        wc.hbrBackground = None  # nemazat pozadi = zadne bile blikani
        wc.lpszClassName = _CANVAS_CLASS
        _USER32.RegisterClassExW.argtypes = [ctypes.POINTER(_WndClassEx)]
        _USER32.RegisterClassExW.restype = ctypes.c_uint16
        atom = _USER32.RegisterClassExW(ctypes.byref(wc))
        return bool(atom)
    except Exception as e:
        debug_log(f"CANVAS CLASS: registrace selhala: {e!r}")
        return False

CONFIG_PATH = os.path.join(os.path.expanduser("~"), ".live_wallpaper_config.json")


def _asset_path(name: str) -> str:
    """Cesta k souboru v assets/ (funguje i ve zmrazenem EXE pres _MEIPASS)."""
    try:
        base = getattr(sys, "_MEIPASS", None) or os.path.dirname(
            os.path.abspath(__file__)
        )
    except Exception:
        base = ""
    return os.path.join(base, "assets", name)


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

IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".bmp", ".gif"}
VIDEO_EXTS = {".mp4", ".avi", ".mkv", ".mov", ".wmv", ".webm"}

ACCENT = "#7c5cff"

THEMES = {
    "dark": {
        "bg": "#1b1c22",
        "card": "#25262e",
        "card_hover": "#2d2f3a",
        "text": "#eceef2",
        "dim": "#9195a3",
        "border": "#3a3c47",
        "input": "#202128",
    },
    "light": {
        "bg": "#eef0f5",
        "card": "#ffffff",
        "card_hover": "#e2e6ee",
        "text": "#1c1e24",
        "dim": "#5d6472",
        "border": "#cfd5e1",
        "input": "#ffffff",
    },
}


def build_stylesheet(theme: str) -> str:
    t = THEMES.get(theme, THEMES["dark"])
    return f"""
QMainWindow {{
    background-color: {t['bg']};
}}
QWidget {{
    color: {t['text']};
    font-family: "Segoe UI", sans-serif;
}}
QLabel#title {{
    font-size: 17px;
    font-weight: 600;
}}
QLabel#subtitle {{
    color: {t['dim']};
    font-size: 12px;
}}
QLabel#status {{
    color: {t['dim']};
    font-size: 12px;
}}
QPushButton {{
    background-color: {ACCENT};
    color: white;
    border: none;
    border-radius: 8px;
    padding: 10px 16px;
    font-size: 13px;
    font-weight: 600;
}}
QPushButton:hover {{
    background-color: #8f74ff;
}}
QPushButton:pressed {{
    background-color: #6a4de0;
}}
QPushButton#secondary {{
    background-color: {t['card']};
    color: {t['text']};
}}
QPushButton#secondary:hover {{
    background-color: {t['card_hover']};
}}
QPushButton:disabled {{
    background-color: {t['card_hover']};
    color: {t['dim']};
}}
QCheckBox {{
    color: {t['dim']};
    font-size: 12px;
}}
QLineEdit {{
    background-color: {t['input']};
    color: {t['text']};
    border: 1px solid {t['border']};
    border-radius: 8px;
    padding: 9px 12px;
    font-size: 12px;
}}
QComboBox {{
    background-color: {t['card']};
    color: {t['text']};
    border: 1px solid {t['border']};
    border-radius: 8px;
    padding: 6px 10px;
    font-size: 12px;
}}
QComboBox QAbstractItemView {{
    background-color: {t['card']};
    color: {t['text']};
    selection-background-color: {ACCENT};
}}
"""


STRINGS = {
    "cs": {
        "subtitle": "Nastav si obrázek nebo video jako pozadí plochy",
        "screen_unknown": "🖥 Obrazovka: nezjištěna",
        "screen_one": "🖥 Obrazovka: {w} × {h}",
        "screen_multi": "🖥 Obrazovky ({n}): {parts} | primární {w} × {h}",
        "drop_hint": "Přetáhni sem obrázek nebo video\nnebo klikni pro výběr",
        "mute": "Ztlumit zvuk videa",
        "apply": "Nastavit jako tapetu",
        "measure": "Změřit obrazovku znovu",
        "stop": "Zastavit / obnovit původní",
        "ready": "Připraveno.",
        "file_selected": "Soubor vybrán. Klikni na „Nastavit jako tapetu“.",
        "yt_placeholder": "Vlož YouTube odkaz…",
        "yt_button": "Stáhnout a nastavit",
        "yt_downloading": "Stahuji z YouTube… {p}",
        "yt_done": "Video staženo, nastavuji jako tapetu…",
        "yt_error": "Stažení selhalo: {e}",
        "yt_invalid_url": "Neplatný nebo nepodporovaný YouTube odkaz.",
        "lang_label": "Jazyk:",
        "theme_label": "Motiv:",
        "theme_dark": "Tmavý",
        "theme_light": "Světlý",
        "warn_unsupported_t": "Nepodporovaný formát",
        "warn_unsupported_m": "Tento typ souboru není podporovaný.",
        "warn_nofile_t": "Chybí soubor",
        "warn_nofile_m": "Nejdřív vyber obrázek nebo video.",
        "img_set": "Obrázek upraven na {w} × {h} a nastaven.",
        "vid_running": "Video tapeta běží na pozadí ({w} × {h}).",
        "vid_started_msg": "Video tapeta byla spuštěna.",
        "restored": "Původní tapeta byla obnovena.",
        "hidden_tray_msg": "Aplikace běží na pozadí v systémové liště.",
        "measured": "Obrazovka změřena: {w} × {h}.",
        "open_tray": "Otevřít",
        "quit_tray": "Ukončit",
        "vid_fail_t": "Video tapeta",
        "vid_fail_m": "Nepodařilo se vložit video na plochu (WorkerW nenalezeno).\nDetail v souboru {log}",
        "vid_decode_t": "Video se nepodařilo přehrát",
        "vid_decode_m": "Z videa nepřišel ani jeden snímek (pravděpodobně nepodporovaný kodek, např. AV1/VP9 bez zvuku ve vysokém rozlišení). Zkus jiné video, ideálně H264 mp4 do 1080p.",
        "app_name": "Live Wallpaper",
    },
    "en": {
        "subtitle": "Set an image or video as your desktop background",
        "screen_unknown": "🖥 Display: not detected",
        "screen_one": "🖥 Display: {w} × {h}",
        "screen_multi": "🖥 Displays ({n}): {parts} | primary {w} × {h}",
        "drop_hint": "Drag & drop an image or video here\nor click to browse",
        "mute": "Mute video sound",
        "apply": "Set as wallpaper",
        "measure": "Re-measure display",
        "stop": "Stop / restore original",
        "ready": "Ready.",
        "file_selected": "File selected. Click “Set as wallpaper”.",
        "yt_placeholder": "Paste a YouTube link…",
        "yt_button": "Download & set",
        "yt_downloading": "Downloading from YouTube… {p}",
        "yt_done": "Video downloaded, setting as wallpaper…",
        "yt_error": "Download failed: {e}",
        "yt_invalid_url": "Invalid or unsupported YouTube link.",
        "lang_label": "Language:",
        "theme_label": "Theme:",
        "theme_dark": "Dark",
        "theme_light": "Light",
        "warn_unsupported_t": "Unsupported format",
        "warn_unsupported_m": "This file type is not supported.",
        "warn_nofile_t": "No file",
        "warn_nofile_m": "Pick an image or video first.",
        "img_set": "Image fitted to {w} × {h} and set.",
        "vid_running": "Video wallpaper running ({w} × {h}).",
        "vid_started_msg": "Video wallpaper started.",
        "restored": "Original wallpaper restored.",
        "hidden_tray_msg": "The app keeps running in the system tray.",
        "measured": "Display measured: {w} × {h}.",
        "open_tray": "Open",
        "quit_tray": "Quit",
        "vid_fail_t": "Video wallpaper",
        "vid_fail_m": "Could not embed the video into the desktop (WorkerW not found).\nSee {log}",
        "vid_decode_t": "Could not play the video",
        "vid_decode_m": "No frames arrived from the video (likely an unsupported codec, e.g. AV1/VP9-only high-resolution file). Try a different video, ideally H264 mp4 up to 1080p.",
        "app_name": "Live Wallpaper",
    },
}

LANGS = {"cs": "Čeština", "en": "English"}

CURRENT_THEME = dict(THEMES["dark"])


def T(key: str) -> str:
    """Aktualni barva motivu (po apply_theme se prepne dark/light)."""
    return CURRENT_THEME.get(key, THEMES["dark"].get(key, "#000000"))


# --------------------------------------------------------------------------
# Nastaveni statickeho obrazku jako tapety (Windows API)
# --------------------------------------------------------------------------
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


# --------------------------------------------------------------------------
# Vyhledani WorkerW okna, kam se vlozi video
# --------------------------------------------------------------------------
def get_workerw_handle():
    progman = win32gui.FindWindow("Progman", None)
    if progman == 0:
        return 0

    def best_workerw_from_list(handles):
        """Z kandidatů vyber viditelné s největší plochou a bez DefView (to jsou ikony)."""
        best = 0
        best_area = 0
        for h in handles:
            try:
                if win32gui.FindWindowEx(h, 0, "SHELLDLL_DefView", None) != 0:
                    continue  # tohle je nosič ikon, ne pozadí
                rect = win32gui.GetWindowRect(h)
                area = max(0, rect[2] - rect[0]) * max(0, rect[3] - rect[1])
                vis = win32gui.IsWindowVisible(h)
                # preferuj viditelné, ale ber i neviditelné když nic jiného není
                score = area + (100_000_000 if vis else 0)
                if score > best_area and area > 100:
                    best_area = score
                    best = h
            except Exception:
                continue
        return best

    def progman_child_workerws():
        out = []
        try:
            h = win32gui.FindWindowEx(progman, 0, "WorkerW", None)
            while h != 0:
                out.append(h)
                h = win32gui.FindWindowEx(progman, h, "WorkerW", None)
        except Exception:
            pass
        return out

    # 1) Nejdriv zkus znovu pouzit existujici fullscreen WorkerW pod Progmanem
    #    (na Win10/11 je video-WorkerW typicky CHILD Progmanu, ne top-level!)
    found = best_workerw_from_list(progman_child_workerws())
    if found != 0:
        return found

    # 2) Jinak popros Explorer o vytvoreni a chvili pockej
    try:
        win32gui.SendMessageTimeout(progman, 0x052C, 0, 0, win32con.SMTO_NORMAL, 1000)
    except Exception:
        pass

    try:
        import time as _time
        _time.sleep(0.5)
    except Exception:
        pass

    # 3) Znovu zkontroluj children Progmanu (typicka cesta)
    found = best_workerw_from_list(progman_child_workerws())
    if found != 0:
        return found

    # 4) Starsi Windows: WorkerW je top-level okno hned za nosicem ikon
    workerw = [0]

    def enum_windows_callback(hwnd, _):
        try:
            shell_view = win32gui.FindWindowEx(hwnd, 0, "SHELLDLL_DefView", None)
        except Exception:
            return True
        if shell_view != 0:
            try:
                candidate = win32gui.FindWindowEx(0, hwnd, "WorkerW", None)
            except Exception:
                candidate = 0
            if candidate != 0:
                workerw[0] = candidate
        return True

    try:
        win32gui.EnumWindows(enum_windows_callback, None)
    except Exception:
        pass
    if workerw[0] != 0:
        return workerw[0]

    # 5) Posledni zachrana: nejvetsi viditelne top-level WorkerW bez DefView
    cands = []
    try:
        def collect(hwnd, _):
            try:
                if win32gui.GetClassName(hwnd) == "WorkerW":
                    cands.append(hwnd)
            except Exception:
                pass
            return True

        win32gui.EnumWindows(collect, None)
    except Exception:
        pass
    return best_workerw_from_list(cands)


def get_virtual_screen_rect():
    """Vrati (x, y, w, h) pres vsechny monitory. Fallback na primarni."""
    try:
        screens = QApplication.screens()
        if screens:
            left = min(s.geometry().left() for s in screens)
            top = min(s.geometry().top() for s in screens)
            right = max(s.geometry().right() + 1 for s in screens)
            bottom = max(s.geometry().bottom() + 1 for s in screens)
            return left, top, right - left, bottom - top
    except Exception:
        pass
    # Fallback pres WinAPI
    try:
        x = win32api.GetSystemMetrics(76)  # SM_XVIRTUALSCREEN
        y = win32api.GetSystemMetrics(77)  # SM_YVIRTUALSCREEN
        w = win32api.GetSystemMetrics(78)  # SM_CXVIRTUALSCREEN
        h = win32api.GetSystemMetrics(79)  # SM_CYVIRTUALSCREEN
        return x, y, w, h
    except Exception:
        pass
    geo = QApplication.primaryScreen().geometry()
    return geo.x(), geo.y(), geo.width(), geo.height()


# --------------------------------------------------------------------------
# Mereni obrazovky a prizpusobeni pozadi jejimu rozmeru
# --------------------------------------------------------------------------
def measure_screens() -> dict:
    """Zmeri vsechny pripojene obrazovky.

    Vrati dict:
      screens: [{index, x, y, width, height (logicke),
                 physical_width, physical_height (skutecne pixely),
                 primary, dpr}]
      virtual: (x, y, w, h) pres vsechny monitory (logicke souradnice)
      primary: (w, h) fyzicke pixely primarni obrazovky
    """
    info = {"screens": [], "virtual": (0, 0, 0, 0), "primary": (0, 0)}
    try:
        app = QApplication.instance()
        screens = app.screens() if app is not None else []
        for i, s in enumerate(screens):
            g = s.geometry()
            try:
                dpr = float(s.devicePixelRatio() or 1.0)
            except Exception:
                dpr = 1.0
            is_primary = False
            try:
                is_primary = (s == app.primaryScreen())
            except Exception:
                pass
            info["screens"].append({
                "index": i,
                "x": g.x(), "y": g.y(),
                "width": g.width(), "height": g.height(),
                "physical_width": int(round(g.width() * dpr)),
                "physical_height": int(round(g.height() * dpr)),
                "primary": is_primary,
                "dpr": dpr,
            })
        if info["screens"]:
            prim = next((x for x in info["screens"] if x["primary"]), info["screens"][0])
            info["primary"] = (prim["physical_width"], prim["physical_height"])
            left = min(x["x"] for x in info["screens"])
            top = min(x["y"] for x in info["screens"])
            right = max(x["x"] + x["width"] for x in info["screens"])
            bottom = max(x["y"] + x["height"] for x in info["screens"])
            info["virtual"] = (left, top, right - left, bottom - top)
            return info
    except Exception:
        pass
    # Fallback pres WinAPI (vraci fyzicke pixely)
    try:
        x = win32api.GetSystemMetrics(76)  # SM_XVIRTUALSCREEN
        y = win32api.GetSystemMetrics(77)  # SM_YVIRTUALSCREEN
        w = win32api.GetSystemMetrics(78)  # SM_CXVIRTUALSCREEN
        h = win32api.GetSystemMetrics(79)  # SM_CYVIRTUALSCREEN
        pw = win32api.GetSystemMetrics(0)  # SM_CXSCREEN
        ph = win32api.GetSystemMetrics(1)  # SM_CYSCREEN
        info["virtual"] = (x, y, w, h)
        info["primary"] = (pw, ph)
        info["screens"] = [{
            "index": 0, "x": x, "y": y, "width": w, "height": h,
            "physical_width": pw, "physical_height": ph,
            "primary": True, "dpr": 1.0,
        }]
    except Exception:
        pass
    return info


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


# --------------------------------------------------------------------------
# Okno s prehravanym videem, vlozene do WorkerW
# --------------------------------------------------------------------------
class VideoWallpaperWindow(QWidget):
    """Video tapeta vnorena do WorkerW za ikonami plochy.

    Zamerne NEPOUZIVA QVideoWidget: Qt totiz vnoreneho potomka ciziho okna
    povazuje za „neexponovaneho“ a nikdy mu nedoruci paint eventy (ani
    QVideoWidget pak nic nevykresli, zustane cerny). Misto toho bereme
    snimky pres QVideoSink (ty chodi spolehlive) a malujeme je primo pres
    GDI (StretchDIBits) na HDC naseho okna - to funguje bez ohledu na Qt.
    """

    failed = Signal(str)

    def __init__(self, video_path: str, muted: bool = True):
        super().__init__()
        # Bez ramecku, bez focusu, bez aktivace - nesmi krast kliky/focus.
        # Zadny layout ani potomek: cele okno je platno, maluje se pres GDI.
        self.setWindowFlags(
            Qt.FramelessWindowHint
            | Qt.WindowDoesNotAcceptFocus
            | Qt.BypassWindowManagerHint
        )
        self.setAttribute(Qt.WA_ShowWithoutActivating, True)
        # Qt uroveň: ignoruj mys, at kliky padaji na plochu
        self.setAttribute(Qt.WA_TransparentForMouseEvents, True)

        self.video_path = video_path
        self._muted = bool(muted)

        self.player = QMediaPlayer(self)
        self.audio_output = QAudioOutput(self)
        self.audio_output.setVolume(0.0 if self._muted else 1.0)
        self.player.setAudioOutput(self.audio_output)
        self.sink = QVideoSink(self)
        self.player.setVideoOutput(self.sink)
        self.sink.videoFrameChanged.connect(self._on_frame)
        try:
            self.player.errorOccurred.connect(self._on_player_error)
        except Exception:
            pass
        # Nativni nekonecna smycka staci sama. Rucni restart pres
        # mediaStatusChanged by se s ni hadal a zpusoboval vicenasobne
        # otevirani souboru, tak ho tu schvalne nepripojujeme.
        try:
            self.player.setLoops(QMediaPlayer.Loops.Infinite)
        except Exception:
            self.player.mediaStatusChanged.connect(self._loop_video)

        self._hdc = 0
        self._canvas = 0  # nativni HWND platna (potomek WorkerW, zadne Qt okno)
        self._dw = 0  # sirka platna ve fyzickych pixelech (dle WorkerW)
        self._dh = 0
        self._frame_size = (0, 0)
        self._frames = 0
        self._blits_ok = 0
        self._blit_fail = 0
        # mod vykreslovani: "coloroncolor" (rychly) | "halftone" (hezci, 4x pomalejsi)
        self._blit_mode = "coloroncolor"
        self._bmi = None  # kesovane BITMAPINFO pro aktualni src rozmer
        # Ochrana proti zahlceni: zpracuj nejvys ~40 snimku/s, zbytek zahod.
        # Jinak se pri 60fps / pomalem blitu fronta snimku nafukuje do pameti
        # a system muze zamrznout. Bezne 24-30fps video prochazi bez skipu.
        self._proc_min_interval = 0.025
        self._last_proc_t = 0.0
        self._skipped = 0
        # Nejvetsi snimek, jaky jeste malujeme 1:1. Vetsi (4K/8K z YouTube)
        # nejdriv rychle zmensime, at GDI nezahltime desitkami MB na snimek.
        self._max_src_pixels = 2560 * 1440
        self._downscaled = False

    def _loop_video(self, status):
        if status == QMediaPlayer.MediaStatus.EndOfMedia:
            self.player.setPosition(0)
            self.player.play()

    def set_muted(self, muted: bool) -> None:
        """F4: okamzite prepne zvuk, i kdyz video prave hraje."""
        self._muted = bool(muted)
        try:
            self.audio_output.setVolume(0.0 if self._muted else 1.0)
        except Exception:
            pass

    def _on_player_error(self, error, error_string):
        debug_log(f"PLAYER ERROR: {error} | {error_string}")

    def start(self) -> bool:
        """Vytvori nativni platno pro video a spusti prehravani. Vrati True.

        Windows 11 „raised desktop“ (Progman ma WS_EX_NOREDIRECTIONBITMAP,
        DefView je WS_EX_LAYERED): platno musi byt PRIMO potomek Progmanu
        s WS_EX_LAYERED + SetLayeredWindowAttributes(alpha=255), z-orderovane
        POD DefView (ikony) a NAD WorkerW (staticka tapeta). Bez vrstveneho
        okna DWM video neskomponuje a zustane cerno/nic (doporuceni primo
        od Microsoftu, stejne to dela Lively/Wallpaper Engine).

        Starsi Windows (bez raised desktop): platno jako potomek WorkerW.
        Snimky se maluji pres GDI v obou pripadech stejne.
        """
        debug_log(f"START: path={self.video_path}")
        progman = win32gui.FindWindow("Progman", None)
        if progman == 0:
            debug_log("START: Progman nenalezeno")
            return False
        try:
            pex = _USER32.GetWindowLongW(progman, _GWL_EXSTYLE)
        except Exception:
            pex = 0
        raised = bool(pex & _WS_EX_NOREDIRECTIONBITMAP)
        defview = win32gui.FindWindowEx(progman, 0, "SHELLDLL_DefView", None)
        workerw = get_workerw_handle()
        debug_log(
            f"START: progman={progman} raised={raised} "
            f"defview={defview} workerw={workerw}"
        )
        if workerw == 0:
            debug_log("START: WorkerW nenalezeno")
            return False
        try:
            px1, py1, px2, py2 = win32gui.GetWindowRect(progman)
            wx1, wy1, wx2, wy2 = win32gui.GetWindowRect(workerw)
            w, h = max(1, wx2 - wx1), max(1, wy2 - wy1)
            x, y = wx1 - px1, wy1 - py1
        except Exception as e:
            debug_log(f"START: GetWindowRect selhalo: {e!r}")
            return False

        if raised:
            parent = progman
            exstyle = (
                _WS_EX_LAYERED | _WS_EX_NOACTIVATE
                | _WS_EX_TRANSPARENT | _WS_EX_TOOLWINDOW
            )
        else:
            parent = workerw
            x, y = 0, 0
            exstyle = _WS_EX_NOACTIVATE | _WS_EX_TRANSPARENT | _WS_EX_TOOLWINDOW
        try:
            cls_ok = _ensure_canvas_class()
            canvas_cls = _CANVAS_CLASS if cls_ok else "STATIC"
            canvas = _USER32.CreateWindowExW(
                exstyle, canvas_cls, None,
                _WS_CHILD | _WS_VISIBLE | _WS_CLIPCHILDREN,
                x, y, w, h,
                parent, None, None, None,
            )
        except Exception as e:
            debug_log(f"START: CreateWindowEx vyjimka: {e!r}")
            return False
        if not canvas:
            try:
                err = ctypes.windll.kernel32.GetLastError()
            except Exception:
                err = "?"
            debug_log(f"START: CreateWindowEx vratilo 0 (err={err})")
            return False
        self._canvas = int(canvas)
        self._dw, self._dh = int(w), int(h)

        if raised:
            # plne nepruhledne vrstvene okno, jinak DWM neskomponuje
            try:
                _USER32.SetLayeredWindowAttributes(
                    self._canvas, 0, 255, _LWA_ALPHA
                )
            except Exception as e:
                debug_log(f"START: SetLayeredWindowAttributes: {e!r}")
            # Z-order choreografie: platno POD ikony, WorkerW POD platno
            try:
                if defview:
                    win32gui.SetWindowPos(
                        self._canvas, defview, 0, 0, 0, 0,
                        win32con.SWP_NOMOVE | win32con.SWP_NOSIZE
                        | win32con.SWP_NOACTIVATE,
                    )
                win32gui.SetWindowPos(
                    workerw, self._canvas, 0, 0, 0, 0,
                    win32con.SWP_NOMOVE | win32con.SWP_NOSIZE
                    | win32con.SWP_NOACTIVATE,
                )
                debug_log("START: z-order nastaven (WorkerW < canvas < DefView)")
            except Exception as e:
                debug_log(f"START: SetWindowPos z-order: {e!r}")
        else:
            try:
                win32gui.SetWindowPos(
                    self._canvas, win32con.HWND_BOTTOM, 0, 0, 0, 0,
                    win32con.SWP_NOMOVE | win32con.SWP_NOSIZE
                    | win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW,
                )
            except Exception:
                pass

        try:
            self._hdc = win32gui.GetDC(self._canvas)
            sm = _COLORONCOLOR if self._blit_mode == "coloroncolor" else _HALFTONE
            _GDI32.SetStretchBltMode(self._hdc, sm)
            _GDI32.PatBlt(self._hdc, 0, 0, self._dw, self._dh, _BLACKNESS)
            debug_log(f"START: canvas={self._canvas} {w}x{h} hdc={self._hdc}")
        except Exception as e:
            debug_log(f"START: GetDC selhalo: {e!r}")
            self._destroy_canvas()
            return False
        _quiet_ffmpeg()  # pro jistotu znovu pred otevrenim souboru
        self.player.setSource(QUrl.fromLocalFile(self.video_path))
        self.player.play()
        debug_log("START: play() zavolano -> OK")
        # Watchdog: kdyz do 8 s neprijde ani snimek (nedejboze nepodporovany
        # kodek typu AV1 - prehravac se zasekne v bufferingu bez chyby),
        # platno zase zrusime, at nezustane svitit naprazdno.
        try:
            QTimer.singleShot(8000, self._check_progress)
        except Exception:
            pass
        return True

    def _check_progress(self):
        try:
            if self._canvas == 0:
                return  # uz zastaveno, vse OK
            if self._frames > 0:
                return  # hraje, vse OK
            pos = 0
            try:
                pos = self.player.position()
            except Exception:
                pass
            debug_log(f"WATCHDOG: zadny snimek za 8 s (pos={pos}) -> rusim platno")
            self.failed.emit("decode")
            self.stop()
        except Exception as e:
            debug_log(f"WATCHDOG vyjimka: {e!r}")

    def _make_bmi(self, sw: int, sh: int) -> _BitmapInfo:
        bmi = _BitmapInfo()
        bmi.header.biSize = 40
        bmi.header.biWidth = sw
        bmi.header.biHeight = -sh  # top-down
        bmi.header.biPlanes = 1
        bmi.header.biBitCount = 32
        bmi.header.biCompression = 0
        return bmi

    def _blit_stretch(self, img: QImage, sw: int, sh: int) -> int:
        """Cover pres GDI StretchDIBits (rychly COLORONCOLOR rezim).
        Buffer se kopiruje jen jednou (bytes -> c_char_p bez mezikopie),
        BITMAPINFO je kesovane podle rozmeru videa."""
        scale = max(self._dw / sw, self._dh / sh)
        dw, dh = int(sw * scale), int(sh * scale)
        dx, dy = int((self._dw - dw) / 2), int((self._dh - dh) / 2)
        n = img.sizeInBytes()
        raw = bytes(img.constBits())
        if len(raw) < n:
            return 0
        buf = ctypes.c_char_p(raw)
        if self._bmi is None:
            self._bmi = self._make_bmi(sw, sh)
        return _GDI32.StretchDIBits(
            self._hdc, dx, dy, dw, dh, 0, 0, sw, sh,
            buf, ctypes.byref(self._bmi), _DIB_RGB_COLORS, _SRCCOPY,
        )

    def _on_frame(self, frame: QVideoFrame):
        if not self._hdc:
            if self._frames == 0:
                debug_log("FRAME: prvni snimek, ale _hdc==0 (GetDC selhalo?)")
            self._frames += 1
            return
        try:
            mapped = frame.map(QVideoFrame.MapMode.ReadOnly)
        except Exception as e:
            debug_log(f"FRAME: map vyjimka: {e!r}")
            return
        if not mapped:
            debug_log("FRAME: map vratil False")
            return
        try:
            now = time.perf_counter()
            if self._frames > 0 and (now - self._last_proc_t) < self._proc_min_interval:
                # nestihame: snimek zahodit (fronta se nesmi nafukovat)
                self._skipped += 1
                return
            img = frame.toImage()
            if img.isNull():
                debug_log("FRAME: toImage null")
                return
            if img.format() != QImage.Format.Format_RGB32:
                img = img.convertToFormat(QImage.Format.Format_RGB32)
            sw, sh = img.width(), img.height()
            if sw <= 0 or sh <= 0 or self._dw <= 0 or self._dh <= 0:
                debug_log(f"FRAME: spatny rozmer src={sw}x{sh} dst={self._dw}x{self._dh}")
                return
            if sw * sh > self._max_src_pixels:
                # obri snimek (4K/8K): rychle zmensit, jinak OOM/zaseknuti
                f = (self._max_src_pixels / (sw * sh)) ** 0.5
                nw, nh = max(2, int(sw * f)), max(2, int(sh * f))
                img = img.scaled(
                    nw, nh,
                    Qt.AspectRatioMode.IgnoreAspectRatio,
                    Qt.TransformationMode.FastTransformation,
                )
                sw, sh = img.width(), img.height()
                if not self._downscaled:
                    self._downscaled = True
                    debug_log(f"FRAME: downscale na {sw}x{sh} (guard)")
            self._last_proc_t = now
            if self._frames == 0:
                debug_log(
                    f"FRAME: prvni snimek src={sw}x{sh} "
                    f"dst={self._dw}x{self._dh} mode={self._blit_mode}"
                )
            if (sw, sh) != self._frame_size:
                # zmena rozmeru videa -> premazat platno, at nezustanou okraje
                self._frame_size = (sw, sh)
                self._bmi = None
                try:
                    _GDI32.PatBlt(self._hdc, 0, 0, self._dw, self._dh, _BLACKNESS)
                except Exception:
                    pass
            r = self._blit_stretch(img, sw, sh)
            self._frames += 1
            if r > 0:
                self._blits_ok += 1
            else:
                self._blit_fail += 1
                if self._blit_fail <= 3:
                    debug_log(f"FRAME: blit vratil {r}")
            if self._frames == 30:
                debug_log(f"FRAME: 30 snimku OK, blits_ok={self._blits_ok}")
        except Exception:
            import traceback
            debug_log("FRAME VYJIMKA:\n" + traceback.format_exc())
            pass
        finally:
            try:
                frame.unmap()
            except Exception:
                pass

    def _embed_into_desktop_DEPRECATED(self):
        """NEPOUZIVA SE: stary zpusob vnoreni Qt okna (SetParent + konverze
        stylu). Qt takhle vnorene okno nikdy nevykreslovalo, proto start()
        ted vytvari nativni WS_CHILD platno rovnou v WorkerW. Ponechano jen
        pro historii, casem smazat."""
        GWL_STYLE = -16
        GWL_EXSTYLE = -20
        WS_CHILD = 0x40000000
        WS_POPUP = 0x80000000
        WS_VISIBLE = 0x10000000
        WS_CAPTION = 0x00C00000
        WS_THICKFRAME = 0x00040000
        WS_MINIMIZEBOX = 0x00020000
        WS_MAXIMIZEBOX = 0x00010000
        WS_SYSMENU = 0x00080000
        WS_EX_TOPMOST = 0x00000008
        WS_EX_TOOLWINDOW = 0x00000080
        WS_EX_APPWINDOW = 0x00040000
        WS_EX_NOACTIVATE = 0x08000000
        WS_EX_TRANSPARENT = 0x00000020  # click-through

        hwnd = int(self.winId())
        workerw = get_workerw_handle()
        debug_log(f"EMBED: hwnd={hwnd} workerw={workerw}")

        vx, vy, vw, vh = get_virtual_screen_rect()

        if workerw == 0:
            # Kdyz WorkerW neexistuje, aspon posli okno dospodu a zpruhledni pro mys
            try:
                style = win32gui.GetWindowLong(hwnd, GWL_STYLE)
                style = style & ~WS_POPUP & ~WS_CAPTION & ~WS_THICKFRAME
                win32gui.SetWindowLong(hwnd, GWL_STYLE, style)
                ex = win32gui.GetWindowLong(hwnd, GWL_EXSTYLE)
                ex = (ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT) & ~WS_EX_APPWINDOW
                win32gui.SetWindowLong(hwnd, GWL_EXSTYLE, ex)
                win32gui.SetWindowPos(
                    hwnd, win32con.HWND_BOTTOM, vx, vy, vw, vh,
                    win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW | win32con.SWP_ASYNCWINDOWPOS,
                )
                try:
                    x1, y1, x2, y2 = win32gui.GetWindowRect(hwnd)
                    self._dw, self._dh = max(1, x2 - x1), max(1, y2 - y1)
                except Exception:
                    pass
            except Exception:
                pass
            return

        try:
            # 1) predelat z WS_POPUP na WS_CHILD, at je to opravdu potomek plochy
            style = win32gui.GetWindowLong(hwnd, GWL_STYLE)
            style = style & ~WS_POPUP & ~WS_CAPTION & ~WS_THICKFRAME
            style = style & ~WS_MINIMIZEBOX & ~WS_MAXIMIZEBOX & ~WS_SYSMENU
            style = style | WS_CHILD | WS_VISIBLE
            win32gui.SetWindowLong(hwnd, GWL_STYLE, style)

            # 2) ex-style: nikdy nebrat focus, nikdy nebrat kliky
            ex = win32gui.GetWindowLong(hwnd, GWL_EXSTYLE)
            ex = ex & ~WS_EX_TOPMOST & ~WS_EX_APPWINDOW
            ex = ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW
            win32gui.SetWindowLong(hwnd, GWL_EXSTYLE, ex)

            # 3) vnoreni do WorkerW
            win32gui.SetParent(hwnd, workerw)

            # 4) roztahnout pres cele WorkerW ( souradnice potomka jsou relativni
            #    k WorkerW, takze vzdy 0,0 + sirka/vyska WorkerW ).
            #    Zadna virtual-screen matematika - WorkerW uz ma spravnou velikost.
            try:
                wx1, wy1, wx2, wy2 = win32gui.GetWindowRect(workerw)
                w, h = max(1, wx2 - wx1), max(1, wy2 - wy1)
            except Exception:
                _, _, w, h = get_virtual_screen_rect()

            win32gui.SetWindowPos(
                hwnd, win32con.HWND_BOTTOM, 0, 0, w, h,
                win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW | win32con.SWP_ASYNCWINDOWPOS,
            )
            # POZOR: zadny self.resize(w, h)! w/h jsou fyzicke pixely z WinAPI,
            # zatimco Qt pracuje v logickych (pri 150% skalovani je 1920x1080
            # fyzicky = 1280x720 logicky). Nativni velikost uz sedi s Qt
            # geometrii z __init__, dalsi resize by video rozhodil.
            # Rozmer si jen ulozime pro GDI vykreslovani snimku.
            self._dw, self._dh = int(w), int(h)
            debug_log(f"EMBED: canvas={w}x{h}")
        except Exception as e:
            print("Embed selhal:", e)
            debug_log(f"EMBED VYJIMKA: {e!r}")

    def _destroy_canvas(self):
        try:
            if self._hdc and self._canvas:
                try:
                    win32gui.ReleaseDC(self._canvas, self._hdc)
                except Exception:
                    pass
        except Exception:
            pass
        self._hdc = 0
        try:
            if self._canvas:
                _USER32.DestroyWindow(self._canvas)
                debug_log(f"STOP: canvas {self._canvas} zniceno")
        except Exception as e:
            debug_log(f"STOP: DestroyWindow selhalo: {e!r}")
        self._canvas = 0
        self._dw, self._dh = 0, 0

    def stop(self):
        debug_log(f"STOP: frames={self._frames} skipped={self._skipped} blits_ok={self._blits_ok} blit_fail={self._blit_fail}")
        try:
            self.player.stop()
        except Exception:
            pass
        self._destroy_canvas()
        try:
            self.close()
        except Exception:
            pass


# --------------------------------------------------------------------------
# Drag & drop zona pro vyber souboru
# --------------------------------------------------------------------------
class DropZone(QFrame):
    file_dropped = Signal(str)
    clicked = Signal()

    def __init__(self):
        super().__init__()
        self.setAcceptDrops(True)
        self.setFixedHeight(150)
        self.setCursor(Qt.PointingHandCursor)
        self._has_file = False
        self._hint = ""
        self._set_style(T("card"), T("border"))

        layout = QVBoxLayout(self)
        layout.setAlignment(Qt.AlignCenter)

        self.icon_label = QLabel("🖼")
        self.icon_label.setAlignment(Qt.AlignCenter)
        self.icon_label.setStyleSheet("font-size: 30px; background: transparent;")
        layout.addWidget(self.icon_label)

        self.text_label = QLabel()
        self.text_label.setAlignment(Qt.AlignCenter)
        self.text_label.setStyleSheet(f"color: {T('dim')}; font-size: 12px; background: transparent;")
        layout.addWidget(self.text_label)
        self.set_hint(STRINGS["cs"]["drop_hint"])

    def _set_style(self, color, border):
        self.setStyleSheet(f"""
            QFrame {{
                background-color: {color};
                border: 2px dashed {border};
                border-radius: 12px;
            }}
        """)

    def set_hint(self, text: str):
        self._hint = text
        if not self._has_file:
            self.text_label.setText(text)

    def refresh_style(self):
        self.text_label.setStyleSheet(
            f"color: {T('dim')}; font-size: 12px; background: transparent;"
        )
        self.icon_label.setStyleSheet("font-size: 30px; background: transparent;")
        self._set_style(T("card"), ACCENT if self._has_file else T("border"))

    def set_file(self, path: str):
        self._has_file = True
        name = os.path.basename(path)
        ext = os.path.splitext(path)[1].lower()
        if ext in IMAGE_EXTS:
            pix = QPixmap(path)
            if not pix.isNull():
                self.icon_label.setPixmap(
                    pix.scaledToHeight(70, Qt.SmoothTransformation)
                )
            else:
                self.icon_label.setText("🖼")
        else:
            self.icon_label.setText("🎬")
            self.icon_label.setStyleSheet("font-size: 30px; background: transparent;")
        self.text_label.setText(name)
        self._set_style(T("card"), ACCENT)

    def dragEnterEvent(self, event: QDragEnterEvent):
        if event.mimeData().hasUrls():
            event.acceptProposedAction()
            self._set_style(T("card_hover"), ACCENT)

    def dragLeaveEvent(self, event):
        self._set_style(T("card"), T("border"))

    def dropEvent(self, event: QDropEvent):
        urls = event.mimeData().urls()
        if urls:
            path = urls[0].toLocalFile()
            self.file_dropped.emit(path)

    def mousePressEvent(self, event):
        self.clicked.emit()


# --------------------------------------------------------------------------
# Stahovani videa z YouTube (yt-dlp) na pozadi, at nezamrzne UI
# --------------------------------------------------------------------------
YT_DIR = os.path.join(tempfile.gettempdir(), "wallmotion_yt")

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
                "format": (
                    "bv*[vcodec^=avc1][height<=1080][ext=mp4]+ba/"
                    "b[vcodec^=avc1][height<=1080][ext=mp4]/"
                    "bv*[vcodec^=avc1][height<=1080]+ba/"
                    "b[vcodec^=avc1][height<=1080]"
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
            with yt_dlp.YoutubeDL(opts) as ydl:
                info = ydl.extract_info(self.url, download=True)
                path = resolve_downloaded_path(info, ydl)
            if path and os.path.exists(path):
                self.finished.emit(path)
            else:
                self.error.emit("soubor se nenasel")
        except Exception as e:
            self.error.emit(str(e)[:300])


# --------------------------------------------------------------------------
# Hlavni okno aplikace
# --------------------------------------------------------------------------
class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Live Wallpaper")
        self.setFixedSize(430, 640)

        self.video_window = None
        self.selected_path = None
        self.original_wallpaper = get_current_wallpaper()
        self.screen_info = measure_screens()
        self.lang = "cs"
        self.theme = "dark"
        self.yt_worker = None

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)
        layout.setContentsMargins(24, 20, 24, 20)
        layout.setSpacing(10)

        self.title_label = QLabel("Live Wallpaper")
        self.title_label.setObjectName("title")
        layout.addWidget(self.title_label)

        self.subtitle_label = QLabel()
        self.subtitle_label.setObjectName("subtitle")
        layout.addWidget(self.subtitle_label)

        self.screen_label = QLabel()
        self.screen_label.setObjectName("status")
        layout.addWidget(self.screen_label)

        # -- nastaveni: jazyk + motiv -------------------------------------
        settings_row = QHBoxLayout()
        settings_row.setSpacing(8)
        self.lang_label = QLabel()
        settings_row.addWidget(self.lang_label)
        self.lang_combo = QComboBox()
        for code, name in LANGS.items():
            self.lang_combo.addItem(name, code)
        self.lang_combo.currentIndexChanged.connect(self._on_lang_changed)
        settings_row.addWidget(self.lang_combo, 1)
        self.theme_label = QLabel()
        settings_row.addWidget(self.theme_label)
        self.theme_combo = QComboBox()
        self.theme_combo.addItem("Tmavý", "dark")
        self.theme_combo.addItem("Světlý", "light")
        self.theme_combo.currentIndexChanged.connect(self._on_theme_changed)
        settings_row.addWidget(self.theme_combo, 1)
        layout.addLayout(settings_row)

        self.drop_zone = DropZone()
        self.drop_zone.file_dropped.connect(self._on_file_chosen)
        self.drop_zone.clicked.connect(self.browse_file)
        layout.addWidget(self.drop_zone)

        # -- YouTube odkaz -------------------------------------------------
        yt_row = QHBoxLayout()
        yt_row.setSpacing(8)
        self.yt_input = QLineEdit()
        yt_row.addWidget(self.yt_input, 1)
        self.yt_button = QPushButton()
        self.yt_button.setObjectName("secondary")
        self.yt_button.clicked.connect(self.download_youtube)
        yt_row.addWidget(self.yt_button)
        layout.addLayout(yt_row)

        self.mute_checkbox = QCheckBox()
        self.mute_checkbox.setChecked(True)
        self.mute_checkbox.toggled.connect(self._on_mute_toggled)
        layout.addWidget(self.mute_checkbox)

        self.apply_btn = QPushButton()
        self.apply_btn.clicked.connect(self.apply_wallpaper)
        layout.addWidget(self.apply_btn)

        self.measure_btn = QPushButton()
        self.measure_btn.setObjectName("secondary")
        self.measure_btn.clicked.connect(self.remeasure_screen)
        layout.addWidget(self.measure_btn)

        self.stop_btn = QPushButton()
        self.stop_btn.setObjectName("secondary")
        self.stop_btn.clicked.connect(self.stop_wallpaper)
        layout.addWidget(self.stop_btn)

        layout.addStretch()

        self.status_label = QLabel()
        self.status_label.setObjectName("status")
        self.status_label.setWordWrap(True)
        layout.addWidget(self.status_label)

        self._init_tray()
        self._load_config()
        self.apply_theme(self.theme, save=False)
        self.retranslate()

    # -- tray ---------------------------------------------------------
    def _make_app_icon(self) -> QIcon:
        # Primarne ikona ze souboru assets/icon.png (monitor s play).
        try:
            p = _asset_path("icon.png")
            if os.path.exists(p):
                icon = QIcon(p)
                if not icon.isNull():
                    return icon
        except Exception:
            pass
        # Fallback: jednoducha fialova ikona programove, at tray nikdy
        # neni bez ikony (na Windows fromTheme vzdy vrati null).
        try:
            pix = QPixmap(64, 64)
            pix.fill(Qt.transparent)
            from PySide6.QtGui import QPainter, QColor, QFont

            p = QPainter(pix)
            p.setRenderHint(QPainter.Antialiasing)
            p.setBrush(QColor(ACCENT))
            p.setPen(Qt.NoPen)
            p.drawRoundedRect(4, 4, 56, 56, 14, 14)
            p.setPen(QColor("#ffffff"))
            p.setFont(QFont("Segoe UI", 30, QFont.Bold))
            p.drawText(pix.rect(), Qt.AlignCenter, "W")
            p.end()
            return QIcon(pix)
        except Exception:
            try:
                from PySide6.QtWidgets import QStyle

                return self.style().standardIcon(QStyle.StandardPixmap.SP_ComputerIcon)
            except Exception:
                return QIcon()

    def _init_tray(self):
        app_icon = self._make_app_icon()
        try:
            self.setWindowIcon(app_icon)
        except Exception:
            pass
        self.tray = QSystemTrayIcon(self)
        self.tray.setIcon(app_icon)
        menu = QMenu()
        self.tray_show_action = QAction("Otevřít", self)
        self.tray_show_action.triggered.connect(self.showNormal)
        self.tray_quit_action = QAction("Ukončit", self)
        self.tray_quit_action.triggered.connect(self.quit_app)
        menu.addAction(self.tray_show_action)
        menu.addAction(self.tray_quit_action)
        self.tray.setContextMenu(menu)
        self.tray.activated.connect(
            lambda reason: self.showNormal() if reason == QSystemTrayIcon.ActivationReason.DoubleClick else None
        )
        self.tray.show()

    def S(self) -> dict:
        return STRINGS.get(self.lang, STRINGS["cs"])

    # -- config ---------------------------------------------------------
    def _load_config(self):
        if os.path.exists(CONFIG_PATH):
            try:
                with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                    cfg = json.load(f)
                lang = cfg.get("lang", "cs")
                if lang in STRINGS:
                    self.lang = lang
                theme = cfg.get("theme", "dark")
                if theme in THEMES:
                    self.theme = theme
                try:
                    self.mute_checkbox.setChecked(bool(cfg.get("muted", True)))
                except Exception:
                    pass
                path = cfg.get("last_path")
                if path and os.path.exists(path):
                    self.selected_path = path
                    self.drop_zone.set_file(path)
            except Exception:
                pass
        # combo boxy nastavit bez vyvolani signalu
        try:
            self.lang_combo.blockSignals(True)
            self.lang_combo.setCurrentIndex(list(LANGS).index(self.lang))
            self.lang_combo.blockSignals(False)
            self.theme_combo.blockSignals(True)
            self.theme_combo.setCurrentIndex(0 if self.theme == "dark" else 1)
            self.theme_combo.blockSignals(False)
        except Exception:
            pass

    def _save_config(self):
        try:
            with open(CONFIG_PATH, "w", encoding="utf-8") as f:
                json.dump({
                    "last_path": self.selected_path,
                    "lang": self.lang,
                    "theme": self.theme,
                    "muted": self.mute_checkbox.isChecked(),
                }, f)
        except Exception:
            pass

    def _on_mute_toggled(self, checked: bool):
        """F4: ulozit volbu a okamzite prepnpout zvuk bezici tapety."""
        self._save_config()
        try:
            if self.video_window is not None:
                self.video_window.set_muted(bool(checked))
        except Exception:
            pass

    # -- motiv + jazyk ---------------------------------------------------------
    def apply_theme(self, theme: str, save: bool = True):
        if theme not in THEMES:
            theme = "dark"
        self.theme = theme
        CURRENT_THEME.clear()
        CURRENT_THEME.update(THEMES[theme])
        try:
            QApplication.instance().setStyleSheet(build_stylesheet(theme))
        except Exception:
            pass
        try:
            self.drop_zone.refresh_style()
        except Exception:
            pass
        if save:
            self._save_config()

    def _on_theme_changed(self, _index: int):
        theme = self.theme_combo.currentData() or "dark"
        self.apply_theme(theme)
        self.retranslate()

    def _on_lang_changed(self, _index: int):
        lang = self.lang_combo.currentData() or "cs"
        if lang not in STRINGS:
            lang = "cs"
        self.lang = lang
        self._save_config()
        self.retranslate()

    def retranslate(self):
        s = self.S()
        self.subtitle_label.setText(s["subtitle"])
        self.lang_label.setText(s["lang_label"])
        self.theme_label.setText(s["theme_label"])
        # texty v comboboxech motivu
        try:
            self.theme_combo.blockSignals(True)
            self.theme_combo.setItemText(0, s["theme_dark"])
            self.theme_combo.setItemText(1, s["theme_light"])
            self.theme_combo.blockSignals(False)
        except Exception:
            pass
        self.drop_zone.set_hint(s["drop_hint"])
        self.mute_checkbox.setText(s["mute"])
        self.apply_btn.setText(s["apply"])
        self.measure_btn.setText(s["measure"])
        self.stop_btn.setText(s["stop"])
        self.yt_input.setPlaceholderText(s["yt_placeholder"])
        self.yt_button.setText(s["yt_button"])
        if not self.status_label.text():
            self.status_label.setText(s["ready"])
        try:
            self.tray_show_action.setText(s["open_tray"])
            self.tray_quit_action.setText(s["quit_tray"])
        except Exception:
            pass
        self._refresh_screen_label()

    # -- YouTube ---------------------------------------------------------
    def download_youtube(self):
        s = self.S()
        url = self.yt_input.text().strip()
        if not url:
            self.status_label.setText(s["warn_nofile_m"])
            return
        # F1: URL overit driv, nez se preda yt-dlp.
        if not is_valid_youtube_url(url):
            self.status_label.setText(
                s["yt_error"].format(e=s["yt_invalid_url"])
            )
            return
        if self.yt_worker is not None and self.yt_worker.isRunning():
            return
        self.yt_button.setEnabled(False)
        self.status_label.setText(s["yt_downloading"].format(p="0%"))
        debug_log(f"YT: stahuji {url}")
        self.yt_worker = DownloadWorker(url, self)
        self.yt_worker.progress.connect(self._on_yt_progress)
        self.yt_worker.finished.connect(self._on_yt_finished)
        self.yt_worker.error.connect(self._on_yt_error)
        self.yt_worker.finished.connect(lambda _p: self.yt_button.setEnabled(True))
        self.yt_worker.error.connect(lambda _e: self.yt_button.setEnabled(True))
        self.yt_worker.start()

    def _on_yt_progress(self, pct: str):
        self.status_label.setText(self.S()["yt_downloading"].format(p=pct))

    def _on_yt_finished(self, path: str):
        s = self.S()
        debug_log(f"YT: stazeno {path}")
        self.status_label.setText(s["yt_done"])
        self._on_file_chosen(path)
        # po stazeni rovnou nastavit jako tapetu
        self.apply_wallpaper()

    def _on_yt_error(self, err: str):
        debug_log(f"YT CHYBA: {err}")
        self.status_label.setText(self.S()["yt_error"].format(e=err))

    # -- obrazovka ---------------------------------------------------------
    def _refresh_screen_label(self):
        s = self.S()
        screens = self.screen_info.get("screens", [])
        pw, ph = self.screen_info.get("primary", (0, 0))
        if not screens:
            self.screen_label.setText(s["screen_unknown"])
            return
        if len(screens) == 1:
            self.screen_label.setText(s["screen_one"].format(w=pw, h=ph))
        else:
            parts = " + ".join(
                f"{x['physical_width']}×{x['physical_height']}" for x in screens
            )
            self.screen_label.setText(
                s["screen_multi"].format(n=len(screens), parts=parts, w=pw, h=ph)
            )

    def remeasure_screen(self):
        self.screen_info = measure_screens()
        self._refresh_screen_label()
        pw, ph = self.screen_info.get("primary", (0, 0))
        self.status_label.setText(self.S()["measured"].format(w=pw, h=ph))

    # -- UI actions ---------------------------------------------------------
    def browse_file(self):
        s = self.S()
        filters = "Obrázky a videa / Images & videos (*.jpg *.jpeg *.png *.bmp *.gif *.mp4 *.avi *.mkv *.mov *.wmv *.webm)"
        path, _ = QFileDialog.getOpenFileName(self, s["apply"], "", filters)
        if path:
            self._on_file_chosen(path)

    def _on_file_chosen(self, path: str):
        s = self.S()
        ext = os.path.splitext(path)[1].lower()
        if ext not in IMAGE_EXTS and ext not in VIDEO_EXTS:
            QMessageBox.warning(self, s["warn_unsupported_t"], s["warn_unsupported_m"])
            return
        self.selected_path = path
        self.drop_zone.set_file(path)
        self.status_label.setText(s["file_selected"])

    def apply_wallpaper(self):
        s = self.S()
        if not self.selected_path:
            QMessageBox.warning(self, s["warn_nofile_t"], s["warn_nofile_m"])
            return

        if self.video_window is not None:
            self.video_window.stop()
            self.video_window = None

        ext = os.path.splitext(self.selected_path)[1].lower()

        # Pozadi se vzdy prizpusobi zmerenemu rozmeru obrazovky.
        self.screen_info = measure_screens()
        self._refresh_screen_label()
        pw, ph = self.screen_info.get("primary", (0, 0))
        debug_log(f"APPLY: path={self.selected_path} ext={ext} screen={pw}x{ph}")

        if ext in IMAGE_EXTS:
            fitted = fit_image_to_screen(self.selected_path, pw, ph)
            set_static_wallpaper(fitted)
            self.status_label.setText(s["img_set"].format(w=pw, h=ph))
        elif ext in VIDEO_EXTS:
            self.video_window = VideoWallpaperWindow(
                self.selected_path, muted=self.mute_checkbox.isChecked()
            )
            self.video_window.failed.connect(self._on_video_failed)
            if self.video_window.start():
                self.status_label.setText(s["vid_running"].format(w=pw, h=ph))
                self.tray.showMessage(
                    s["app_name"], s["vid_started_msg"],
                    QSystemTrayIcon.MessageIcon.Information, 3000
                )
            else:
                self.video_window.stop()
                self.video_window = None
                self.status_label.setText(s["vid_fail_m"].format(log=DEBUG_LOG))
                QMessageBox.warning(
                    self, s["vid_fail_t"],
                    s["vid_fail_m"].format(log=DEBUG_LOG),
                )
                return
        else:
            return

        self._save_config()

    def _on_video_failed(self, reason: str):
        s = self.S()
        debug_log(f"VIDEO FAILED: {reason}")
        if self.video_window is not None:
            try:
                self.video_window.failed.disconnect(self._on_video_failed)
            except Exception:
                pass
            self.video_window = None
        if reason == "decode":
            self.status_label.setText(s["vid_decode_m"])
            QMessageBox.warning(self, s["vid_decode_t"], s["vid_decode_m"])
        else:
            self.status_label.setText(s["vid_fail_m"].format(log=DEBUG_LOG))

    def stop_wallpaper(self):
        if self.video_window is not None:
            self.video_window.stop()
            self.video_window = None
        if self.original_wallpaper:
            set_static_wallpaper(self.original_wallpaper)
        self.status_label.setText(self.S()["restored"])

    def closeEvent(self, event):
        s = self.S()
        event.ignore()
        self.hide()
        self.tray.showMessage(
            s["app_name"], s["hidden_tray_msg"],
            QSystemTrayIcon.MessageIcon.Information, 2000
        )

    def quit_app(self):
        if self.video_window is not None:
            self.video_window.stop()
        QApplication.quit()


def main():
    # Qt 6 si DPI awareness (Per-Monitor V2) nastavuje samo.
    # Rucni SetProcessDpiAwareness by hazelo chybu "Pristup byl odepren",
    # tak ho tu schvalne NEvolame.
    # Ztiseni ukecanych FFmpeg logu (Input #0, MFT, ...). Nejsou to chyby,
    # jen info o dekodovani, tak je skryjeme, at nezasvinuji konzoli.
    # Qt kategorie (pres QT_LOGGING_RULES) + nativni av_log level (primo ve FFmpegu).
    os.environ.setdefault("QT_LOGGING_RULES", "qt.multimedia.ffmpeg=false")
    try:
        QLoggingCategory.setFilterRules("qt.multimedia.ffmpeg=false")
    except Exception:
        pass
    _quiet_ffmpeg()
    try:
        with open(DEBUG_LOG, "w", encoding="utf-8") as f:
            f.write("=== Live Wallpaper start ===\n")
    except Exception:
        pass
    debug_log("APP start")
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)
    app.setStyleSheet(build_stylesheet("dark"))
    win = MainWindow()
    win.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
