"""Auto-pause rules: pause video wallpaper during fullscreen apps / on battery.

Same idea as Lively: a fullscreen game should get the GPU back, and a
laptop on battery should not waste power on an invisible wallpaper.
Sensors: WinAPI on Windows; on Linux, sysfs battery plus an X11-only
fullscreen probe (xprop/xwininfo/xrandr) - Wayland exposes no
compositor-neutral "focused window geometry" protocol, so there the
rule is a documented no-op. Decision logic is pure and unit-tested.
"""

from __future__ import annotations

import ctypes
import re
import sys

if sys.platform == "win32":
    import win32api
    import win32gui
else:
    win32api = None
    win32gui = None

_MONITOR_DEFAULTTONEAREST = 2

# Shell windows that are never a "fullscreen app" (desktop itself).
_SHELL_CLASSES = {"Progman", "WorkerW", "SHELLDLL_DefView"}

# Poll interval (ms) and clean polls required before resuming, so that
# alt-tabbing does not flap pause/play every second.
POLL_INTERVAL_MS = 2000
RESUME_AFTER_CLEAN_POLLS = 2


class _SystemPowerStatus(ctypes.Structure):
    _fields_ = [
        ("ACLineStatus", ctypes.c_byte),  # 0=battery, 1=AC, 255=unknown
        ("BatteryFlag", ctypes.c_byte),
        ("BatteryLifePercent", ctypes.c_byte),
        ("Reserved1", ctypes.c_byte),
        ("BatteryLifeTime", ctypes.c_ulong),
        ("BatteryFullLifeTime", ctypes.c_ulong),
    ]


def is_fullscreen_rect(window_rect: tuple, monitor_rect: tuple) -> bool:
    """Pure check: does the window cover its whole monitor?"""
    try:
        return tuple(window_rect) == tuple(monitor_rect)
    except Exception:
        return False


def should_pause(
    fullscreen_active: bool,
    on_battery: bool,
    *,
    pause_on_fullscreen: bool,
    pause_on_battery: bool,
) -> bool:
    """Pure decision: pause when any enabled rule fires."""
    try:
        if pause_on_fullscreen and fullscreen_active:
            return True
        if pause_on_battery and on_battery:
            return True
    except Exception:
        pass
    return False


def parse_ac_line_status(ac_line_status: int) -> bool:
    """True when the AC status byte means 'running on battery'."""
    try:
        return int(ac_line_status) == 0
    except Exception:
        return False


def foreground_window_info() -> tuple | None:
    """(hwnd, rect) of the foreground window, or None. Windows-only."""
    if win32gui is None:
        return None
    try:
        hwnd = win32gui.GetForegroundWindow()
        if not hwnd:
            return None
        try:
            cls = win32gui.GetClassName(hwnd)
            if cls in _SHELL_CLASSES:
                return None
        except Exception:
            pass
        rect = win32gui.GetWindowRect(hwnd)
        return (hwnd, rect)
    except Exception:
        return None


def monitor_rect_for_window(hwnd) -> tuple | None:
    """Monitor rect (left, top, right, bottom) nearest to hwnd, or None."""
    if win32api is None or win32gui is None:
        return None
    try:
        monitor = win32api.MonitorFromWindow(hwnd, _MONITOR_DEFAULTTONEAREST)
        info = win32api.GetMonitorInfo(monitor)
        return tuple(info["Monitor"])
    except Exception:
        return None


def parse_xprop_active_window(output: str) -> int | None:
    """Window id from `xprop -root _NET_ACTIVE_WINDOW`. Pure, tested.

    Output: `_NET_ACTIVE_WINDOW(WINDOW): window id # 0x3e00007`
    """
    try:
        m = re.search(r"#\s*(0x[0-9a-fA-F]+|\d+)", output or "")
        return int(m.group(1), 0) if m else None
    except Exception:
        return None


def parse_xwininfo_geometry(output: str) -> tuple | None:
    """(left, top, right, bottom) from `xwininfo -id ... -stats`. Pure."""
    try:
        def field(name: str) -> int:
            return int(re.search(rf"{name}:\s*(-?\d+)", output).group(1))

        x = field(r"Absolute upper-left X")
        y = field(r"Absolute upper-left Y")
        w = field("Width")
        h = field("Height")
        if w <= 0 or h <= 0:
            return None
        return (x, y, x + w, y + h)
    except Exception:
        return None


def parse_xrandr_monitors(output: str) -> list:
    """[(left, top, right, bottom)] from `xrandr --listmonitors`. Pure.

    Lines: ` 0: +*eDP-1 1920/344x1080/194+0+0  eDP-1`
    (pixel size, then physical mm, then +x+y offsets).
    """
    monitors = []
    try:
        for m in re.finditer(
                r"^\s*\d+:.*?(\d+)/\d+x(\d+)/\d+([+-]\d+)([+-]\d+)",
                output or "", re.MULTILINE):
            w, h, x, y = (int(m.group(i)) for i in range(1, 5))
            if w > 0 and h > 0:
                monitors.append((x, y, x + w, y + h))
    except Exception:
        pass
    return monitors


def is_fullscreen_app_active_x11() -> bool:
    """X11 sensor: does _NET_ACTIVE_WINDOW exactly cover a monitor?

    Uses xprop/xwininfo/xrandr (x11-utils, near-universal on X11).
    Wayland deliberately reports False - there is no compositor-neutral
    protocol to read the focused window's geometry.
    """
    try:
        from wallmotion.platform.linux import detect_session, run_command, session_tool
        if detect_session().get("session") != "x11":
            return False
        tools = {t: session_tool(t) for t in ("xprop", "xwininfo", "xrandr")}
        if not all(tools.values()):
            return False
        ok, out, _ = run_command(
            [tools["xprop"], "-root", "_NET_ACTIVE_WINDOW"])
        if not ok:
            return False
        wid = parse_xprop_active_window(out)
        if not wid:
            return False
        ok, out, _ = run_command(
            [tools["xwininfo"], "-id", hex(wid), "-stats"])
        if not ok:
            return False
        rect = parse_xwininfo_geometry(out)
        if not rect:
            return False
        ok, out, _ = run_command([tools["xrandr"], "--listmonitors"])
        if not ok:
            return False
        return any(rect == mon for mon in parse_xrandr_monitors(out))
    except Exception:
        return False


def is_fullscreen_app_active() -> bool:
    """True when the foreground window covers its whole monitor."""
    if sys.platform.startswith("linux"):
        return is_fullscreen_app_active_x11()
    try:
        info = foreground_window_info()
        if not info:
            return False
        hwnd, rect = info
        monitor_rect = monitor_rect_for_window(hwnd)
        if not monitor_rect:
            return False
        return is_fullscreen_rect(rect, monitor_rect)
    except Exception:
        return False


def is_on_battery() -> bool:
    """True when running on battery power. False on desktops / unknown."""
    if sys.platform == "win32":
        try:
            status = _SystemPowerStatus()
            ok = ctypes.windll.kernel32.GetSystemPowerStatus(ctypes.byref(status))
            if not ok:
                return False
            return parse_ac_line_status(status.ACLineStatus)
        except Exception:
            return False
    if sys.platform.startswith("linux"):
        try:
            from wallmotion.platform.linux import is_on_battery_linux
            return is_on_battery_linux()
        except Exception:
            return False
    return False
