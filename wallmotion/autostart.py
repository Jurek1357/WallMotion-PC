"""Autostart entries: launch WallMotion with the OS session.

Windows: .lnk shortcut in the Startup folder (WScript via pywin32).
Linux: .desktop file in ~/.config/autostart (reuses platform.linux).
Pure path logic is unit-tested; OS writes are best-effort booleans.
"""

from __future__ import annotations

import os
import sys

APP_NAME = "WallMotion"


def windows_startup_dir() -> str:
    """Per-user Startup folder (Windows only)."""
    try:
        appdata = os.environ.get("APPDATA", "")
        return os.path.join(
            appdata, "Microsoft", "Windows", "Start Menu",
            "Programs", "Startup")
    except Exception:
        return ""


def windows_link_path() -> str:
    return os.path.join(windows_startup_dir(), APP_NAME + ".lnk")


def launch_target() -> tuple:
    """(target, args, workdir) for starting this app again.

    Frozen exe: the exe itself. Source run: python + main.py.
    """
    try:
        if getattr(sys, "frozen", False):
            exe = os.path.abspath(sys.executable)
            return (exe, "", os.path.dirname(exe))
        script = os.path.join(
            os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
            "main.py")
        return (sys.executable, f'"{script}"',
                os.path.dirname(script))
    except Exception:
        return (sys.executable, "", "")


def is_enabled() -> bool:
    """True when an autostart entry currently exists."""
    try:
        if sys.platform == "win32":
            return os.path.exists(windows_link_path())
        if sys.platform.startswith("linux"):
            cfg = os.environ.get(
                "XDG_CONFIG_HOME",
                os.path.join(os.path.expanduser("~"), ".config"))
            return os.path.exists(
                os.path.join(cfg, "autostart", "wallmotion.desktop"))
    except Exception:
        pass
    return False


def set_enabled(enabled: bool) -> bool:
    """Create or remove the autostart entry. Returns success."""
    try:
        if sys.platform == "win32":
            return _set_windows(enabled)
        if sys.platform.startswith("linux"):
            return _set_linux(enabled)
    except Exception:
        pass
    return False


def _set_windows(enabled: bool) -> bool:
    try:
        link = windows_link_path()
        if not enabled:
            try:
                if os.path.exists(link):
                    os.remove(link)
            except Exception:
                pass
            return not os.path.exists(link)
        try:
            import win32com.client
        except Exception:
            return False
        os.makedirs(os.path.dirname(link), exist_ok=True)
        target, args, workdir = launch_target()
        shell = win32com.client.Dispatch("WScript.Shell")
        shortcut = shell.CreateShortCut(link)
        shortcut.TargetPath = target
        shortcut.Arguments = args
        shortcut.WorkingDirectory = workdir
        try:
            shortcut.IconLocation = target
        except Exception:
            pass
        shortcut.save()
        return os.path.exists(link)
    except Exception:
        return False


def _set_linux(enabled: bool) -> bool:
    try:
        cfg = os.environ.get(
            "XDG_CONFIG_HOME",
            os.path.join(os.path.expanduser("~"), ".config"),
        )
        desktop = os.path.join(cfg, "autostart", "wallmotion.desktop")
        if not enabled:
            try:
                if os.path.exists(desktop):
                    os.remove(desktop)
            except Exception:
                pass
            return not os.path.exists(desktop)
        from wallmotion.platform.linux import write_autostart
        target, args, _workdir = launch_target()
        exec_line = target if not args else f"{target} {args}"
        return bool(write_autostart(exec_line))
    except Exception:
        return False
