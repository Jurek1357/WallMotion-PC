"""System theme probe: dark/light as chosen in the OS settings.

Windows: HKCU...\\Personalize\\AppsUseLightTheme registry value.
Linux: GNOME color-scheme (best effort, other DEs report None).
Pure parsers are unit-tested; readers never raise.
"""

from __future__ import annotations

import sys


def parse_windows_value(value) -> str | None:
    """AppsUseLightTheme (0/1) -> 'dark'/'light'. Pure, unit-tested."""
    try:
        return "light" if int(value) == 1 else "dark"
    except Exception:
        return None


def parse_gnome_value(output: str) -> str | None:
    """gsettings color-scheme output -> 'dark'/'light'. Pure, tested."""
    try:
        text = str(output or "").strip().strip("'\"").lower()
        if "dark" in text:
            return "dark"
        if text in ("default", "prefer-light", "no-preference"):
            return "light"
    except Exception:
        pass
    return None


def read_system_theme() -> str | None:
    """Current OS theme, or None when unknown/unsupported."""
    try:
        if sys.platform == "win32":
            try:
                import winreg
            except Exception:
                return None
            try:
                key = winreg.OpenKey(
                    winreg.HKEY_CURRENT_USER,
                    r"SOFTWARE\Microsoft\Windows\CurrentVersion"
                    r"\Themes\Personalize")
                value, _kind = winreg.QueryValueEx(key, "AppsUseLightTheme")
                try:
                    winreg.CloseKey(key)
                except Exception:
                    pass
                return parse_windows_value(value)
            except Exception:
                return None
        if sys.platform.startswith("linux"):
            try:
                import shutil
                import subprocess
                if not shutil.which("gsettings"):
                    return None
                proc = subprocess.run(
                    ["gsettings", "get",
                     "org.gnome.desktop.interface", "color-scheme"],
                    capture_output=True, text=True, timeout=5)
                if proc.returncode != 0:
                    return None
                return parse_gnome_value(proc.stdout)
            except Exception:
                return None
    except Exception:
        pass
    return None
