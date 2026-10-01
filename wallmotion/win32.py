"""Native Windows layer: GDI constants, video canvas, WorkerW lookup.

Everything Win32-only; on other platforms the module stays importable,
but wallpaper functions are not used (see wallmotion.platform).
"""

from __future__ import annotations

import ctypes
import sys

from wallmotion.utils import debug_log

# Win32-only modules: stay None on Linux/macOS, module remains importable
# (tests, CI, future Linux backend), but wallpaper functions are Windows-only.
if sys.platform == "win32":
    import win32api
    import win32con
    import win32gui
else:
    win32gui = win32con = win32api = None

# --- GDI constants for rendering video directly on the desktop ---
_GDI32 = ctypes.windll.gdi32 if sys.platform == "win32" else None
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


# --- Native Win32 video canvas (instead of Qt window) ---
_USER32 = ctypes.windll.user32 if sys.platform == "win32" else None
_WS_CHILD = 0x40000000
_WS_VISIBLE = 0x10000000
_WS_CLIPCHILDREN = 0x02000000
_WS_EX_NOACTIVATE = 0x08000000
_WS_EX_TRANSPARENT = 0x00000020  # clicks pass through to the desktop
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
    """Register custom canvas class: own DC (OWNDC) and NO background
    erase (NULL brush). Plain STATIC would flash white on every repaint;
    our canvas paints itself fully each frame via GDI."""
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
        try:
            _get_module = _USER32.GetModuleHandleW
        except Exception:
            # Frozen exe: user32 handle lookup can fail; kernel32
            # always exports GetModuleHandleW.
            _get_module = ctypes.windll.kernel32.GetModuleHandleW
        _get_module.argtypes = [ctypes.c_wchar_p]
        _get_module.restype = ctypes.c_void_p
        wc = _WndClassEx()
        wc.cbSize = ctypes.sizeof(_WndClassEx)
        wc.style = _CS_OWNDC
        wc.lpfnWndProc = _USER32.DefWindowProcW
        wc.hInstance = _get_module(None)
        wc.hbrBackground = None  # do not erase background = no white flashing
        wc.lpszClassName = _CANVAS_CLASS
        _USER32.RegisterClassExW.argtypes = [ctypes.POINTER(_WndClassEx)]
        _USER32.RegisterClassExW.restype = ctypes.c_uint16
        atom = _USER32.RegisterClassExW(ctypes.byref(wc))
        return bool(atom)
    except Exception as e:
        debug_log(f"CANVAS CLASS: registrace selhala: {e!r}")
        return False


def get_workerw_handle():
    """Find the WorkerW window where the video is inserted."""
    progman = win32gui.FindWindow("Progman", None)
    if progman == 0:
        return 0

    def best_workerw_from_list(handles):
        """Pick the visible candidate with largest area and no DefView (those are icons)."""
        best = 0
        best_area = 0
        for h in handles:
            try:
                if win32gui.FindWindowEx(h, 0, "SHELLDLL_DefView", None) != 0:
                    continue  # this one hosts icons, not the background
                rect = win32gui.GetWindowRect(h)
                area = max(0, rect[2] - rect[0]) * max(0, rect[3] - rect[1])
                vis = win32gui.IsWindowVisible(h)
                # prefer visible ones, but take invisible if nothing else exists
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

    # 1) First try reusing the existing fullscreen WorkerW under Progman
    #    (on Win10/11 the video WorkerW is typically a CHILD of Progman, not top-level!)
    found = best_workerw_from_list(progman_child_workerws())
    if found != 0:
        return found

    # 2) Otherwise ask Explorer to create one and wait a bit
    try:
        win32gui.SendMessageTimeout(progman, 0x052C, 0, 0, win32con.SMTO_NORMAL, 1000)
    except Exception:
        pass

    try:
        import time as _time
        _time.sleep(0.5)
    except Exception:
        pass

    # 3) Re-check Progman children (typical path)
    found = best_workerw_from_list(progman_child_workerws())
    if found != 0:
        return found

    # 4) Older Windows: WorkerW is the top-level window right behind the icon host
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

    # 5) Last resort: largest visible top-level WorkerW without DefView
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
