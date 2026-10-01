"""Linux backends: static image + video via external renderers.

Design (see docs/STACK_ANALYSIS.md appendix): instead of re-implementing
rendering, orchestrate proven tools - feh/swww/gsettings/
/// plasma-apply-wallpaperimage for images, mpvpaper or xwinwrap+mpv
for video. Session capability comes from $XDG_SESSION_TYPE /
$XDG_CURRENT_DESKTOP.

This module is Qt-free (stdlib only) so it stays unit-testable headless.
"""

from __future__ import annotations

import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time


def debug_log(msg: str) -> None:
    """wallmotion.utils.debug_log without a hard dependency cycle.

    utils already guards all failures; a lazy import keeps this module
    Qt-free and importable on its own.
    """
    try:
        from wallmotion.utils import debug_log as _log
        _log(msg)
    except Exception:
        pass

# Wayland compositors we drive with mpvpaper/swww.
WLROOTS_DESKTOPS = frozenset({
    "sway", "hyprland", "wlroots", "wayfire", "river", "dwl",
    "labwc", "niri",
})

MPV_IPC_SOCKET_NAME = "wallmotion-mpv.sock"


def bundled_bin_dirs() -> list:
    """Directories that may contain WallMotion-bundled renderer tools.

    Two sources, both optional:
    - the AppImage: PyInstaller onefile unpacks next to itself, so
      dirname(sys.executable) inside a running AppImage is
      $APPDIR/usr/bin where CI deposits mpv/feh/xwinwrap/swww/mpvpaper;
    - ~/.local/share/wallmotion/bin: user-level drop-in for tools that
      are not bundled or were installed by the app itself.
    """
    dirs = []
    try:
        if getattr(sys, "frozen", False):
            dirs.append(os.path.dirname(os.path.abspath(sys.executable)))
    except Exception:
        pass
    try:
        data_home = os.environ.get(
            "XDG_DATA_HOME", os.path.join(os.path.expanduser("~"),
                                          ".local", "share"))
        dirs.append(os.path.join(data_home, "wallmotion", "bin"))
    except Exception:
        pass
    return [d for d in dirs if os.path.isdir(d)]


def tool_path() -> str:
    """PATH string with bundled dirs prepended (for shutil.which)."""
    return os.pathsep.join(bundled_bin_dirs() +
                           [os.environ.get("PATH", "") or os.defpath])


def tool_env() -> dict:
    """os.environ copy with bundled dirs prepended to PATH.

    Child processes of our tools do their own PATH lookups (xwinwrap
    execs `mpv`), so the spawned environment must carry the override -
    shutil.which(name, path=...) alone does not reach grandchildren.
    """
    env = dict(os.environ)
    env["PATH"] = tool_path()
    return env


# Canonical binary dirs, searched before PATH for session-integrated
# tools. User-owned prefixes (Linuxbrew, Conda, ~/.local/bin scripts)
# can shadow system tools with incompatible builds - observed on
# Ubuntu: brew's `gsettings` uses the keyfile backend, so every
# `gsettings set` vanished into ~/.config/glib-2.0/settings/keyfile
# and the desktop wallpaper never changed. /usr/bin wins; PATH remains
# the fallback for systems without these dirs (NixOS etc.).
SYSTEM_BIN_DIRS = ("/usr/bin", "/bin", "/usr/local/bin")


def session_tool(name: str) -> str | None:
    """Resolved path for a tool that must talk to the real user
    session, or None. Renderers stay bundled-first (that is the point
    of bundling them); gsettings/dconf/KDE/X11 helpers must be the
    distro's binaries or they silently target the wrong store."""
    for d in SYSTEM_BIN_DIRS:
        p = os.path.join(d, name)
        if os.path.isfile(p) and os.access(p, os.X_OK):
            return p
    return _which(name)


def detect_session(env=None) -> dict:
    """Describe the graphical session. Pure function, unit-tested.

    Returns {session, desktops, is_gnome, is_kde, is_wlroots} where
    session is "x11" | "wayland" | "tty" | "unknown".
    """
    env = env if env is not None else os.environ
    try:
        session = str(env.get("XDG_SESSION_TYPE", "") or "").lower()
    except Exception:
        session = ""
    if not session and env.get("WAYLAND_DISPLAY"):
        session = "wayland"
    try:
        raw = str(env.get("XDG_CURRENT_DESKTOP", "") or "").lower()
    except Exception:
        raw = ""
    desktops = [d for d in raw.replace(";", ":").split(":") if d]
    return {
        "session": session or "unknown",
        "desktops": desktops,
        "is_gnome": "gnome" in desktops,
        "is_kde": "kde" in desktops or "plasma" in desktops,
        "is_wlroots": bool(set(desktops) & set(WLROOTS_DESKTOPS)),
    }


def candidate_backends(info: dict) -> list:
    """Backend names in priority order for the detected session."""
    session = info.get("session", "unknown")
    if session == "x11":
        return ["x11"]
    if session == "wayland":
        if info.get("is_kde"):
            return ["kde"]
        if info.get("is_wlroots"):
            return ["wlroots"]
        if info.get("is_gnome"):
            return ["gnome"]
        return ["wlroots", "kde", "gnome"]
    if session == "tty":
        return []
    return ["x11", "wlroots", "kde", "gnome"]


def _which(name: str) -> str | None:
    try:
        return shutil.which(name, path=tool_path())
    except Exception:
        return None


def file_uri(path: str) -> str:
    """Absolute file:// URI for gsettings."""
    try:
        return "file://" + os.path.abspath(path)
    except Exception:
        return "file://" + path


def run_command(cmd: list, timeout: int = 15) -> tuple:
    """Run a command, never raise. Returns (ok, stdout, stderr)."""
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout,
            env=tool_env(),
        )
        return (proc.returncode == 0, proc.stdout or "", proc.stderr or "")
    except Exception as e:
        return (False, "", repr(e))


# --- command builders (pure, unit-tested) -------------------------------

def feh_command(path: str) -> list:
    return ["feh", "--bg-fill", path]


def swww_command(path: str) -> list:
    return ["swww", "img", "--resize", "crop", path]


def gsettings_commands(path: str, exe: str = "gsettings") -> list:
    uri = file_uri(path)
    base = [exe, "set", "org.gnome.desktop.background"]
    return [base + ["picture-uri", uri], base + ["picture-uri-dark", uri]]


def plasma_command(path: str, exe: str = "plasma-apply-wallpaperimage") -> list:
    return [exe, path]


def mpv_options(muted: bool, volume: float) -> str:
    """mpv options forwarded by mpvpaper -o (and used for plain mpv).

    Muted start must use `mute=yes`, not `no-audio`: `no-audio` never
    opens an audio stream, so a later `set_property mute no` over JSON
    IPC has nothing to unmute. `mute=yes` keeps the stream and is
    reversible at runtime.
    """
    try:
        vol = max(0, min(100, int(round(float(volume) * 100))))
    except Exception:
        vol = 30
    if muted:
        return f"loop-file=inf mute=yes volume={vol}"
    return f"loop-file=inf volume={vol} mute=no"


def ipc_socket_path() -> str:
    """Where the mpv JSON IPC socket lives."""
    base = os.environ.get("XDG_RUNTIME_DIR", "") or tempfile.gettempdir()
    return os.path.join(base, MPV_IPC_SOCKET_NAME)


def clear_ipc_socket() -> None:
    """Unlink a leftover mpv IPC socket before a new spawn.

    mpv removes the socket on clean exit, but a killed player (SIGKILL,
    dead parent, crashed compositor) leaves the file behind and the
    next spawn can silently lose its IPC channel.
    """
    try:
        os.unlink(ipc_socket_path())
    except Exception:
        pass


def mpvpaper_command(path: str, muted: bool, volume: float,
                     output: str = "*", ipc_socket: str | None = None) -> list:
    """mpvpaper on all outputs (*) or one named output (e.g. DP-1)."""
    opts = mpv_options(muted, volume)
    if ipc_socket:
        opts += f" input-ipc-server={ipc_socket}"
    return ["mpvpaper", "-o", opts, output, path]


def xwinwrap_geometry(monitor: dict | None) -> str | None:
    """'WxH+X+Y' for one monitor, or None for fullscreen. Pure."""
    try:
        if not monitor:
            return None
        w, h = max(1, int(monitor["w"])), max(1, int(monitor["h"]))
        x, y = int(monitor["x"]), int(monitor["y"])
        return f"{w}x{h}+{x}+{y}"
    except Exception:
        return None


def xwinwrap_command(path: str, muted: bool, volume: float,
                     ipc_socket: str | None = None,
                     geometry: str | None = None) -> list:
    """xwinwrap fullscreen (-fs) or on one monitor (-g WxH+X+Y), plus mpv
    (WID is substituted by xwinwrap). -b -ni -nf keep the wallpaper
    below the desktop, input-transparent and unfocusable - without -b
    the window renders on top of the icons (r00tdaemon fork README)."""
    mpv_cmd = ["mpv", "-wid", "WID", "--loop-file=inf", "--no-osc",
               "--no-input-default-bindings"]
    if muted:
        mpv_cmd.append("--mute=yes")
    else:
        try:
            vol = max(0, min(100, int(round(float(volume) * 100))))
        except Exception:
            vol = 30
        mpv_cmd.append(f"--volume={vol}")
    if ipc_socket:
        mpv_cmd.append(f"--input-ipc-server={ipc_socket}")
    mpv_cmd.append(path)
    if geometry:
        return ["xwinwrap", "-g", geometry, "-b", "-ni", "-nf", "-ov",
                "-fdt", "--"] + mpv_cmd
    return ["xwinwrap", "-fs", "-b", "-ni", "-nf", "-ov", "-fdt",
            "--"] + mpv_cmd


def mpv_ipc_message(prop: str, value) -> bytes:
    """One JSON IPC line for mpv (set_property)."""
    import json
    return (json.dumps({"command": ["set_property", prop, value]}) + "\n").encode()


def mpv_ipc_set(sock_path: str, prop: str, value, timeout: float = 2.0) -> bool:
    """Live set of an mpv property via JSON IPC. Never raises."""
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            sock.settimeout(timeout)
            sock.connect(sock_path)
            sock.sendall(mpv_ipc_message(prop, value))
            return True
        finally:
            try:
                sock.close()
            except Exception:
                pass
    except Exception:
        return False


def ensure_swww_daemon() -> bool:
    """Make sure swww-daemon runs (needed before swww img)."""
    ok, _out, _err = run_command(["swww", "query"], timeout=5)
    if ok:
        return True
    try:
        subprocess.Popen(["swww-daemon"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True, env=tool_env())
    except Exception:
        return False
    for _ in range(10):
        time.sleep(0.2)
        ok, _out, _err = run_command(["swww", "query"], timeout=5)
        if ok:
            return True
    return False


def read_battery_status(sysfs_root: str = "/sys/class/power_supply") -> bool:
    """True when a battery supply reports Discharging. Never raises."""
    try:
        entries = os.listdir(sysfs_root)
    except Exception:
        return False
    for name in entries:
        type_path = os.path.join(sysfs_root, name, "type")
        status_path = os.path.join(sysfs_root, name, "status")
        try:
            with open(type_path, encoding="utf-8") as f:
                if f.read().strip().lower() != "battery":
                    continue
            with open(status_path, encoding="utf-8") as f:
                if f.read().strip().lower() == "discharging":
                    return True
        except Exception:
            continue
    return False


# --- GNOME video via the Hanabi Shell extension --------------------------
#
# GNOME Wayland has no public video-wallpaper API. Hanabi is the
# de-facto extension for it; when present we drive it through its own
# gsettings schema. Hanabi is not on extensions.gnome.org, so install
# uses the upstream-built zip bundled in assets/ (GPL-3.0, source at
# github.com/jeffshee/gnome-ext-hanabi). On Wayland the shell loads a
# freshly installed extension only after a relogin; enable is queued
# via enabled-extensions so it activates then.

HANABI_UUID = "hanabi-extension@jeffshee.github.io"
HANABI_SCHEMA = "io.github.jeffshee.hanabi-extension"
HANABI_ZIP = f"{HANABI_UUID}.shell-extension.zip"


def _hanabi_dir() -> str:
    data_home = os.environ.get(
        "XDG_DATA_HOME",
        os.path.join(os.path.expanduser("~"), ".local", "share"))
    return os.path.join(
        data_home, "gnome-shell", "extensions", HANABI_UUID)


def _hanabi_gsettings(exe: str) -> list:
    """Command prefix so gsettings finds Hanabi's own schema dir.

    The extension ships its schema inside its install dir, outside the
    default glib search path - writes must point GSETTINGS_SCHEMA_DIR
    at it or the schema is not found.
    """
    return [session_tool("env") or "env",
            f"GSETTINGS_SCHEMA_DIR={os.path.join(_hanabi_dir(), 'schemas')}",
            exe]


def hanabi_state() -> str:
    """'enabled' | 'installed' | 'queued' | 'missing' | 'unknown'.

    `gnome-extensions list` prints one uuid per line; matching must be
    exact, another extension's uuid may share a prefix. A freshly
    installed extension does not appear in `list` until the shell
    rescans (relogin on Wayland); that is 'queued'. 'installed' means
    the shell knows the extension but it is disabled - enabling it
    then works live.
    """
    exe = session_tool("gnome-extensions")
    if not exe:
        return "unknown"
    ok, out, _err = run_command([exe, "list"], timeout=10)
    if not ok:
        return "unknown"
    if HANABI_UUID not in out.splitlines():
        return ("queued" if os.path.isdir(_hanabi_dir()) else "missing")
    ok, out, _err = run_command([exe, "list", "--enabled"], timeout=10)
    if ok and HANABI_UUID in out.splitlines():
        return "enabled"
    return "installed"


def hanabi_install(timeout: int = 180) -> tuple:
    """Install Hanabi from the bundled zip. Returns (ok, text).

    Hanabi is not published on extensions.gnome.org, so GNOME's
    InstallRemoteExtension D-Bus method cannot fetch it. The AppImage
    carries the upstream-built zip in assets/ instead; `gnome-extensions
    install` unpacks it to the user extensions dir with no dialog. On
    Wayland the shell only loads it after a relogin; callers should
    re-check hanabi_state().
    """
    from wallmotion.utils import _asset_path
    zpath = _asset_path(HANABI_ZIP)
    if not os.path.exists(zpath):
        return (False, "bundled extension zip not found")
    exe = session_tool("gnome-extensions")
    if exe:
        ok, out, err = run_command(
            [exe, "install", "--force", zpath], timeout=30)
        if ok:
            return (True, (out or "").strip())
    try:
        import zipfile
        dest = os.path.realpath(_hanabi_dir())
        with zipfile.ZipFile(zpath) as z:
            for member in z.namelist():
                target = os.path.realpath(os.path.join(dest, member))
                if not target.startswith(dest + os.sep):
                    return (False, f"unsafe zip member: {member}")
            z.extractall(dest)
        return (True, "")
    except Exception as e:
        return (False, repr(e))


def hanabi_disable() -> bool:
    """Disable Hanabi live. The only reliable way to stop a running
    renderer - clearing video-path alone leaves it playing."""
    exe = session_tool("gnome-extensions")
    if not exe:
        return False
    ok, _out, _err = run_command([exe, "disable", HANABI_UUID], timeout=10)
    return ok


def hanabi_enable() -> bool:
    """Enable Hanabi, live if possible else queued for next login.

    `gnome-extensions enable` works once the running shell knows the
    extension; a freshly installed one it reports "does not exist".
    Writing enabled-extensions directly queues it to load+activate on
    the next login either way.
    """
    exe = session_tool("gnome-extensions")
    if exe:
        ok, _out, _err = run_command(
            [exe, "enable", HANABI_UUID], timeout=10)
        if ok:
            return True
    gs = session_tool("gsettings")
    if not gs:
        return False
    ok, out, _err = run_command(
        [gs, "get", "org.gnome.shell", "enabled-extensions"], timeout=10)
    if not ok:
        return False
    import ast
    try:
        uuids = ast.literal_eval(out.strip().removeprefix("@as "))
    except Exception:
        return False
    if HANABI_UUID in uuids:
        return True
    uuids.append(HANABI_UUID)
    ok, _out, _err = run_command([
        gs, "set", "org.gnome.shell", "enabled-extensions",
        "[" + ", ".join(repr(u) for u in uuids) + "]"], timeout=10)
    return ok


def hanabi_video_commands(path: str, muted: bool, volume: float,
                          exe: str = "gsettings") -> list:
    """gsettings writes that make Hanabi play `path`.

    video-path triggers playback on change; mute/volume are the app's
    own defaults (the extension defaults to unmuted).
    """
    try:
        vol = max(0, min(100, int(round(float(volume) * 100))))
    except Exception:
        vol = 30
    return [
        _hanabi_gsettings(exe) + ["set", HANABI_SCHEMA, "mute",
                                  "true" if muted else "false"],
        _hanabi_gsettings(exe) + ["set", HANABI_SCHEMA, "volume", str(vol)],
        _hanabi_gsettings(exe) + ["set", HANABI_SCHEMA, "video-path", path],
    ]


def autostart_entry(exec_path: str) -> str:
    """Content of the ~/.config/autostart/wallmotion.desktop file."""
    return (
        "[Desktop Entry]\n"
        "Type=Application\n"
        "Name=WallMotion\n"
        f"Exec={exec_path}\n"
        "X-GNOME-Autostart-enabled=true\n"
    )


def write_autostart(exec_path: str) -> bool:
    """Write the autostart entry. Returns success."""
    try:
        directory = os.path.join(
            os.environ.get("XDG_CONFIG_HOME",
                           os.path.join(os.path.expanduser("~"), ".config")),
            "autostart",
        )
        os.makedirs(directory, exist_ok=True)
        with open(os.path.join(directory, "wallmotion.desktop"),
                  "w", encoding="utf-8") as f:
            f.write(autostart_entry(exec_path))
        return True
    except Exception:
        return False


# --- backends ------------------------------------------------------------

class LinuxProcessBackend:
    """Base: owns one child process group; stop() kills it all."""

    name = "linux-base"
    required_tools: tuple = ()
    # Tools that must be the real system binaries (session store /
    # compositor helpers) - resolved via session_tool, not bare PATH.
    session_tools: tuple = ()

    def __init__(self):
        self._proc = None

    def missing_tools(self) -> list:
        return ([t for t in self.required_tools if not _which(t)]
                + [t for t in self.session_tools if not session_tool(t)])

    def available(self) -> bool:
        return not self.missing_tools()

    def _spawn(self, cmd: list) -> bool:
        self.stop()
        clear_ipc_socket()
        try:
            self._proc = subprocess.Popen(
                cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                start_new_session=True, env=tool_env(),
            )
            return True
        except Exception:
            self._proc = None
            return False

    def _alive(self) -> bool:
        return self._proc is not None and self._proc.poll() is None

    def stop(self) -> None:
        proc, self._proc = self._proc, None
        if proc is None:
            return
        try:
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
        except Exception:
            pass
        try:
            proc.wait(timeout=3)
        except Exception:
            try:
                if proc.poll() is None:
                    os.killpg(proc.pid, signal.SIGKILL)
            except Exception:
                pass

    def set_paused(self, paused: bool) -> None:
        """Pause/resume playback via mpv IPC (autopause rules)."""
        mpv_ipc_set(ipc_socket_path(), "pause", bool(paused))

    def restore(self) -> None:
        """Restore the wallpaper the user had before WallMotion.

        Default no-op: backends that can snapshot their desktop's image
        state override this (gnome via gsettings, x11 via ~/.fehbg,
        wlroots via `swww query`).
        """


class X11Backend(LinuxProcessBackend):
    """X11: feh for images, fullscreen xwinwrap+mpv for video."""

    name = "x11"
    required_tools = ("feh", "xwinwrap", "mpv")

    def __init__(self):
        super().__init__()
        # None = not snapshotted yet; False = no previous wallpaper;
        # bytes = content of the previous ~/.fehbg restore script.
        self._fehbg_backup: bytes | None = None
        self._fehbg_seen: bool | None = None

    @staticmethod
    def _fehbg_path() -> str:
        return os.path.join(os.path.expanduser("~"), ".fehbg")

    def _snapshot_fehbg(self) -> None:
        """Save ~/.fehbg once, before our first `feh --bg-fill` rewrites it."""
        if self._fehbg_seen is not None:
            return
        try:
            with open(self._fehbg_path(), "rb") as f:
                self._fehbg_backup = f.read()
            self._fehbg_seen = True
        except Exception:
            self._fehbg_backup = None
            self._fehbg_seen = False

    def set_image(self, path: str) -> bool:
        if not _which("feh"):
            return False
        self._snapshot_fehbg()
        ok, _out, _err = run_command(feh_command(path))
        return ok

    def restore(self) -> None:
        """Re-run the previous ~/.fehbg script (it re-applies the old
        wallpaper). No previous state -> leave the current image."""
        if not self._fehbg_seen or not self._fehbg_backup:
            debug_log("RESTORE(x11): no captured wallpaper state")
            return
        try:
            path = self._fehbg_path()
            with open(path, "wb") as f:
                f.write(self._fehbg_backup)
            os.chmod(path, 0o755)
            run_command(["/bin/sh", path])
        except Exception:
            pass

    def set_video(self, path: str, muted: bool, volume: float,
                  geometry: str | None = None) -> bool:
        if not (_which("xwinwrap") and _which("mpv")):
            return False
        return self._spawn(xwinwrap_command(path, muted, volume,
                                           ipc_socket=ipc_socket_path(),
                                           geometry=geometry))

    def set_muted(self, muted: bool) -> None:
        mpv_ipc_set(ipc_socket_path(), "mute", bool(muted))

    def set_volume(self, volume: float) -> None:
        try:
            vol = max(0, min(100, int(round(float(volume) * 100))))
        except Exception:
            return
        mpv_ipc_set(ipc_socket_path(), "volume", vol)


class WlrootsBackend(LinuxProcessBackend):
    """wlroots Wayland (sway/hyprland/...): swww for images, mpvpaper video."""

    name = "wlroots"
    required_tools = ("swww", "mpvpaper")

    def __init__(self):
        super().__init__()
        self._swww_seen: bool | None = None
        self._swww_backup: str | None = None

    def _snapshot_swww(self) -> None:
        """Remember the currently displayed image from `swww query`.

        Output lines look like:
        `eDP-1: 1920x1080, scale: 1, currently displaying: image: /p/x.png`
        Per-output restore fidelity is lost (one image goes to all
        outputs on restore) - good enough for a stop button.
        """
        if self._swww_seen is not None:
            return
        self._swww_seen = True
        self._swww_backup = None
        ok, out, _err = run_command(["swww", "query"], timeout=5)
        if not ok:
            return
        for line in out.splitlines():
            marker = "image: "
            if marker in line:
                self._swww_backup = line.split(marker, 1)[1].strip()
                return

    def set_image(self, path: str) -> bool:
        if not _which("swww"):
            return False
        if not ensure_swww_daemon():
            return False
        self._snapshot_swww()
        ok, _out, _err = run_command(swww_command(path))
        return ok

    def restore(self) -> None:
        if self._swww_seen and self._swww_backup:
            run_command(swww_command(self._swww_backup))
        else:
            debug_log("RESTORE(wlroots): no captured wallpaper state")

    def set_video(self, path: str, muted: bool, volume: float,
                  geometry: str | None = None) -> bool:
        # mpvpaper always covers all outputs (*) in v1; per-output names
        # need wlr enumeration on real hardware (see TODO). Geometry is
        # accepted for API parity and ignored.
        if not _which("mpvpaper"):
            return False
        return self._spawn(mpvpaper_command(path, muted, volume,
                                           ipc_socket=ipc_socket_path()))

    def set_muted(self, muted: bool) -> None:
        mpv_ipc_set(ipc_socket_path(), "mute", bool(muted))

    def set_volume(self, volume: float) -> None:
        try:
            vol = max(0, min(100, int(round(float(volume) * 100))))
        except Exception:
            return
        mpv_ipc_set(ipc_socket_path(), "volume", vol)


class KdeBackend(WlrootsBackend):
    """KDE Plasma: own image tool, mpvpaper video (works on KWin Wayland)."""

    name = "kde"
    required_tools = ("mpvpaper",)
    session_tools = ("plasma-apply-wallpaperimage",)

    def set_image(self, path: str) -> bool:
        exe = session_tool("plasma-apply-wallpaperimage")
        if not exe:
            return False
        ok, _out, _err = run_command(plasma_command(path, exe=exe))
        return ok


class GnomeBackend(LinuxProcessBackend):
    """GNOME: gsettings for images; video via the Hanabi Shell
    extension - there is no public video-background API."""

    name = "gnome"
    session_tools = ("gsettings",)

    _BG_SCHEMA = "org.gnome.desktop.background"
    _BG_KEYS = ("picture-uri", "picture-uri-dark")

    def __init__(self):
        super().__init__()
        # {key: raw gsettings value} captured before our first set.
        self._saved_uris: dict | None = None
        # video-path we put back on unpause ('' while paused).
        self._hanabi_paused_path: str | None = None

    def _snapshot_uris(self, exe: str) -> None:
        if self._saved_uris is not None:
            return
        self._saved_uris = {}
        for key in self._BG_KEYS:
            ok, out, _err = run_command(
                [exe, "get", self._BG_SCHEMA, key])
            # Keep the raw GVariant literal ('file:///x' or '') so the
            # restore `gsettings set` can use it verbatim.
            self._saved_uris[key] = out.strip() if ok else "''"

    def set_image(self, path: str) -> bool:
        exe = session_tool("gsettings")
        if not exe:
            return False
        self._snapshot_uris(exe)
        # A running Hanabi renderer would cover the static image -
        # clearing video-path alone leaves it playing, so disable.
        if hanabi_state() == "enabled":
            self._hanabi_set("video-path", "")
            hanabi_disable()
        for cmd in gsettings_commands(path, exe=exe):
            ok, _out, _err = run_command(cmd)
            if not ok:
                return False
        return True

    def restore(self) -> None:
        if not self._saved_uris:
            debug_log("RESTORE(gnome): no captured wallpaper state")
            return
        exe = session_tool("gsettings") or "gsettings"
        for key, raw in self._saved_uris.items():
            if raw:
                ok, _out, err = run_command(
                    [exe, "set", self._BG_SCHEMA, key, raw])
                if not ok:
                    debug_log(f"RESTORE(gnome): set {key} failed: {err}")

    def set_video(self, path: str, muted: bool, volume: float,
                  geometry: str | None = None) -> bool:
        """Video via Hanabi's gsettings.

        When the extension is installed but disabled the keys are
        written BEFORE enabling: Hanabi's renderer reads video-path on
        launch and opens its preferences window when it is empty -
        writing first makes it launch straight into playback.
        'queued' extensions still need a relogin and refuse here.
        """
        state = hanabi_state()
        if state not in ("enabled", "installed"):
            return False
        exe = session_tool("gsettings")
        if not exe:
            return False
        ok_all = True
        for cmd in hanabi_video_commands(path, muted, volume, exe=exe):
            ok, _out, _err = run_command(cmd)
            ok_all = ok and ok_all
        if state == "installed":
            ok_all = hanabi_enable() and ok_all
        if ok_all:
            self._hanabi_paused_path = None
        return ok_all

    def _hanabi_set(self, key: str, value: str) -> bool:
        exe = session_tool("gsettings")
        if not exe:
            return False
        ok, _out, _err = run_command(
            _hanabi_gsettings(exe) + ["set", HANABI_SCHEMA, key, value])
        return ok

    def _hanabi_get(self, key: str) -> str:
        exe = session_tool("gsettings")
        if not exe:
            return ""
        ok, out, _err = run_command(
            _hanabi_gsettings(exe) + ["get", HANABI_SCHEMA, key])
        return out.strip() if ok else ""

    def stop(self) -> None:
        """Stop video: clear Hanabi's video-path and disable the
        extension - the renderer ignores an empty path, only disabling
        tears it down."""
        if hanabi_state() == "enabled" or self._hanabi_paused_path:
            self._hanabi_set("video-path", "")
            hanabi_disable()
        self._hanabi_paused_path = None
        super().stop()

    def set_paused(self, paused: bool) -> None:
        """Pause = stash video-path, clear it, disable the renderer;
        resume writes the path back and re-enables. Hanabi exposes no
        pause key, so this is the only reliable freeze."""
        state = hanabi_state()
        if state not in ("enabled", "installed"):
            return
        if paused:
            raw = self._hanabi_get("video-path")
            path = raw[1:-1] if len(raw) > 1 and raw.startswith("'") else raw
            if path:
                self._hanabi_paused_path = path
            self._hanabi_set("video-path", "")
            if state == "enabled":
                hanabi_disable()
        elif self._hanabi_paused_path:
            # write the path before enabling so the renderer launches
            # straight into playback
            self._hanabi_set("video-path", self._hanabi_paused_path)
            if state == "installed":
                hanabi_enable()
            self._hanabi_paused_path = None

    def set_muted(self, muted: bool) -> None:
        if hanabi_state() == "enabled":
            self._hanabi_set("mute", "true" if muted else "false")

    def set_volume(self, volume: float) -> None:
        try:
            vol = max(0, min(100, int(round(float(volume) * 100))))
        except Exception:
            return
        if hanabi_state() == "enabled":
            self._hanabi_set("volume", str(vol))


_BACKENDS = {
    "x11": X11Backend,
    "wlroots": WlrootsBackend,
    "kde": KdeBackend,
    "gnome": GnomeBackend,
}


def detect_backend(env=None):
    """First available backend for this session, or None.

    GNOME returns its backend even for video-less operation (image works,
    set_video reports False with the Hanabi note).
    """
    info = detect_session(env)
    for name in candidate_backends(info):
        cls = _BACKENDS.get(name)
        if cls is None:
            continue
        backend = cls()
        try:
            if backend.available():
                return backend
        except Exception:
            continue
    # Last resort: a backend whose image tool exists (partial support).
    for name in candidate_backends(info):
        cls = _BACKENDS.get(name)
        if cls is None:
            continue
        backend = cls()
        try:
            total = (len(backend.required_tools)
                     + len(backend.session_tools))
            if not backend.missing_tools() or len(
                    backend.missing_tools()) < total:
                return backend
        except Exception:
            continue
    return None


def describe_session() -> str:
    """One-line session description for logs and status messages."""
    info = detect_session()
    desks = ",".join(info["desktops"]) or "?"
    return f"session={info['session']} desktop={desks}"


def is_on_battery_linux() -> bool:
    """Battery sensor for the autopause rules (sysfs, no dependencies)."""
    if not sys.platform.startswith("linux"):
        return False
    return read_battery_status()
