# WallMotion-PC — Stack Analysis

Analysis of `github.com/Jurek1357/WallMotion-PC` (cloned 2026-09-15, commit `66e0c1f`).

**Scope note:** the maintainer wants Linux support, not Windows-only. That materially changes the recommendation — see "The Linux question" and the updated verdict at the end.

## What the project actually is

A Windows 10/11 live-wallpaper utility — a lightweight Wallpaper Engine / Lively alternative:

- Static image wallpaper, auto-fitted to measured screen resolution (`SystemParametersInfo`)
- Video wallpaper rendered behind desktop icons via the classic `Progman`/`WorkerW` trick
- YouTube download (yt-dlp, H.264-only, <=1080p, <=500 MB, audio required)
- Tray-resident PySide6 settings UI (430x640), CZ/EN, dark/light themes
- Distributed as a ~65 MB PyInstaller `--onefile` exe

**Current stack:** Python 3.9+ / PySide6 (Qt Multimedia decode) / pywin32 + ctypes (Win32) / yt-dlp / imageio-ffmpeg / PyInstaller.

**Codebase shape:** one `main.py`, 2011 lines, all comments and most identifiers in Czech. No tests, no CI, no `pyproject.toml`, no license. Community health: 28 % (README only).

## Assessment of the current stack

The stack is better than it looks. The author made several competent choices:

- **Qt Multimedia + `QVideoSink` + raw GDI blit** — deliberately avoids `QVideoWidget` (which cannot paint into a reparented foreign window). Frames are pulled via `QVideoSink` and painted with `StretchDIBits` (~6 ms/frame at 1080p). This is the same architecture Lively uses conceptually, and it is the correct call inside Qt.
- **Win11 "raised desktop" handling** — detects `WS_EX_NOREDIRECTIONBITMAP` on Progman and creates a `WS_EX_LAYERED` child z-ordered between `WorkerW` and `SHELLDLL_DefView`, falling back to classic `WorkerW` on Win10. That is non-trivial platform knowledge done right.
- **SSRF-aware URL validation** (`is_valid_youtube_url`) — scheme allowlist, IP/localhost rejection, domain allowlist, re-validated inside the download thread. Rarely seen in hobby projects.
- **Sane guards** — frame throttle (~40 fps), 4K/8K downscale, no-frames watchdog, 500 MB cap, ffmpeg presence detection with a fallback yt-dlp format ladder.

### Where the stack hurts

| Pain point | Severity | Notes |
|---|---|---|
| 2011-line single file | **High** | Biggest contributor friction. Nothing else is close. |
| PyInstaller exe (~65 MB) | Medium | SmartScreen/AV flagging, slow start, but standard for this class. |
| Qt Multimedia codec ceiling | Medium | No AV1/VP9 on this backend — author already works around it by pinning H.264 in the yt-dlp format selector. Correct mitigation, but it is a ceiling. |
| Czech-only code comments | Medium | README is bilingual (fine); the code itself is not contributor-readable for non-Czech speakers. |
| No tests / no CI / manual release | High | v1.0.0 exe was built and uploaded by hand. |
| No license | **Blocker** | Legally not open source at all right now. |

## The Linux question

Cross-platform ambition changes the analysis, because **"video wallpaper" is not one feature on Linux — it is four**:

| Linux target | Mechanism | Feasibility |
|---|---|---|
| X11 sessions (XFCE, Cinnamon, MATE, KDE/GNOME on X11) | Reparent a window onto the root/desktop window — the `xwinwrap` approach | Straightforward, 20 years of precedent |
| wlroots compositors (Sway, Hyprland, river, …) | `wlr-layer-shell` surface at the background layer | Clean, standard protocol — this is exactly what `mpvpaper` (video) and `swww` (images) do |
| KDE Plasma Wayland | KWin implements `wlr-layer-shell`; the official path is a **Plasma wallpaper plugin** (QML/C++) | Both routes work |
| GNOME Wayland | No layer-shell, no root window. Requires a **shell extension** rendering into Mutter/Clutter (the Hanabi-extension approach) | Hardest target; realistically a separate component |

Two consequences:

1. **None of the Win32 code transfers.** `Progman`/`WorkerW` has no Linux equivalent; each display stack needs its own backend. What *does* transfer is most of the codebase — UI, download pipeline, config, i18n, themes are all already portable. Roughly 60–70 % is platform-neutral; only the canvas/wallpaper layer is OS-specific.
2. **On Linux, do not render it yourself — delegate.** The proven renderers already exist: `mpvpaper` (wlroots), `xwinwrap` + `mpv` (X11), `swww` (static images on Wayland), Plasma plugins (KDE). The pragmatic architecture is: the app stays the controller (select, download, preview, settings) and spawns the right renderer per environment. This is the ponytail answer — the Linux wallpaper ecosystem is a solved problem; the product gap is a friendly UI + YouTube pipeline on top.

Bonus insight: adopting **libmpv** as the decode engine fixes the H.264-only ceiling *and* makes Linux trivial — `mpvpaper` is literally mpv + layer-shell. On Windows, libmpv can render into the existing native canvas via its OpenGL/D3D render API, replacing Qt Multimedia as the decoder while keeping the WorkerW trick.

## Stack options, honestly compared (cross-platform)

### Option A — Keep Python + PySide6, add a platform backend seam (recommended)

The app's hard parts already work on Windows; for Linux, orchestrate proven renderers instead of re-implementing them.

- Split `main.py` into a package — now with an explicit platform interface:
  `wallmotion/platform/` → `windows.py` (existing GDI/WorkerW canvas, moves verbatim), `x11.py` (spawn `xwinwrap` + `mpv`, or Xlib reparent via `python-xlib`), `wlroots.py` (spawn `mpvpaper`), `kde.py` (layer-shell or plugin docs), `gnome.py` (extension stub / documented limitation).
- Ship static-image wallpaper on Linux first via `swww`/`feh` — trivially easy, covers the feature's core.
- Video on Linux = spawn `mpvpaper` (Wayland) or `xwinwrap` + `mpv` (X11); detect session via `$XDG_SESSION_TYPE` / `$XDG_CURRENT_DESKTOP`.
- Qt UI, yt-dlp, config, themes: untouched — already cross-platform.
- Add `pyproject.toml`, ruff, pytest for the pure functions (`is_valid_youtube_url`, image-fit math, format selectors).
- CI: build exe on `windows-latest`; Linux packaging via **AppImage** (Flatpak later — sandboxing fights wallpaper access).
- Optional: **Nuitka** instead of PyInstaller — compiles to C, fewer AV false-positives.
- Optional codec upgrade: **libmpv/python-mpv** widens decode far beyond Qt Multimedia (AV1, VP9, hw decode) and unifies the engine with what the Linux renderers use.

**Cost:** days-to-weeks. **Risk:** low — Windows keeps working while Linux lands backend-by-backend. GNOME-Wayland stays a documented gap (extension required).

### Option B — C# / .NET 8 + Avalonia

- What Lively uses on Windows; **Avalonia** (not WPF/WinUI) is the cross-platform UI — runs on Linux X11/Wayland and Windows.
- But the renderers still need per-platform native code: WorkerW on Windows, layer-shell/X11 on Linux. Avalonia gives you the settings window, not the wallpaper surface.
- Best Store/MSIX story on Windows; weakest fit for wlroots protocols (no mature .NET layer-shell client).

**Cost:** full rewrite + per-DE native backends anyway. Justified only if Windows Store distribution or a large wallpaper-library UI is the real goal.

### Option C — Rust + libmpv (the strongest rewrite candidate now)

- **One render engine everywhere:** libmpv handles decode/render on all targets — kills the codec ceiling permanently.
- Linux: `smithay-client-toolkit` layer-shell client (wlroots + KWin) or an `xwinwrap`-style X11 window; this is `swww`/`mpvpaper` territory — the ecosystem is already Rust/C.
- Windows: `windows-rs` reparents an HWND under WorkerW exactly as today; mpv render-API paints into it.
- ~10 MB binary, no runtime, no AV/SmartScreen drama, packaging via cargo.
- UI: `egui`/`iced` (simple settings window is all this needs) or Tauri if a web UI is wanted — though Tauri's webview still can't be the wallpaper surface.

**Cost:** real rewrite — the Win32 choreography, Qt UI, and yt-dlp orchestration all get re-expressed. **Verdict:** now genuinely defensible (cross-platform + codec ceiling + binary size), but it is still a rewrite of a working app. The rational path is Option A first; if the project takes off and the Python glue becomes the ceiling, the backend interface designed in Option A becomes the port map for Rust.

## Recommendation

1. **Do not rewrite yet.** Keep Python + PySide6. Cross-platform does not force a rewrite — it forces a **platform abstraction**, which the codebase needs anyway.
2. **Modularize around a `platform/` backend interface.** That single refactor is both the contribution enabler and the Linux port map. Windows backend = existing code, moved verbatim.
3. **Linux v1: static images via `swww`/`feh`, video via `mpvpaper`/`xwinwrap`+mpv.** Detect session type, spawn the right tool, keep the same UI + download pipeline. Cover wlroots + X11 + KDE first; document GNOME-Wayland as needing an extension.
4. **Translate code comments to English** during the split.
5. **Adopt libmpv** when the codec ceiling actually hurts (AV1 YouTube content is increasingly common) — it doubles as the Linux render path.
6. Keep C#/Avalonia and Rust+libmpv as documented migration targets; revisit only if Store distribution (→ C#) or binary size/performance (→ Rust) becomes a real requirement.

## Quick reference — what is already good and should survive any refactor

- `VideoWallpaperWindow.start()` — raised-desktop detection + layered-window z-order dance.
- `is_valid_youtube_url()` — keep verbatim, add tests.
- The yt-dlp format ladders (`_YT_FORMAT_MERGED` / `_YT_FORMAT_SINGLE`) — encode real domain knowledge.
- Frame throttle + watchdog + 4K downscale guards.

---

## Appendix: Linux implementation plan (detailed)

Written to be executed directly — every mechanism, command, and edge case is spelled out. No design decisions left open except the ones marked **[DECIDE]**.

### 1. Environment detection

Wallpaper capability is decided by two env vars, both readable at startup:

```python
session = os.environ.get("XDG_SESSION_TYPE", "").lower()      # "x11" | "wayland" | "tty"
desktop = os.environ.get("XDG_CURRENT_DESKTOP", "").lower()   # "kde", "gnome", "sway",
                                                              # "hyprland", "xfce", "x-cinnamon"…
```

- `XDG_CURRENT_DESKTOP` may be a colon list (`"ubuntu:GNOME"`) — check with `in`, not `==`.
- Hyprland also exports `HYPRLAND_INSTANCE_SIGNATURE`; sway exports `SWAYSOCK`. Fallback detection, not primary.
- On Wayland, `WAYLAND_DISPLAY` non-empty is a usable fallback when `XDG_SESSION_TYPE` is unset.

### 2. Backend selection matrix

| Session / desktop | Static image | Video | Confidence |
|---|---|---|---|
| `x11` (any DE) | `feh --bg-fill <file>` | `xwinwrap` + `mpv` | High |
| `wayland` + `kde` | `plasma-apply-wallpaperimage <file>` | `mpvpaper` | High |
| `wayland` + sway/hyprland/wlroots | `swww img <file>` | `mpvpaper` | High |
| `wayland` + `gnome` | `gsettings set org.gnome.desktop.background picture-uri(-dark) file://…` | **unsupported in v1** (needs GNOME Shell extension — see §6) | Documented gap |
| `wayland` + unknown | try `swww`, fall back to reporting unsupported | try `mpvpaper`, fail with a clear message | Best effort |

### 3. External tools — the renderers we delegate to

| Tool | Purpose | Install | Invocation |
|---|---|---|---|
| `mpv` | Decode engine for all video paths | every distro repo | via xwinwrap's `-wid` |
| `mpvpaper` | Video wallpaper on wlroots layer-shell | AUR / most repos / cargo-build | `mpvpaper -o "loop no-audio" '*' file.mp4` (`*` = all outputs; or an output name like `DP-1`) |
| `swww` | Static image on wlroots | repos / `cargo install swww` | needs `swww-daemon` running; then `swww img --resize crop file.png` |
| `xwinwrap` | Sticky desktop-level window on X11 | build from source (mmhobi7 fork) | `xwinwrap -g 1920x1080+0+0 -ov -fdt -- mpv -wid WID --loop=inf --no-audio file.mp4` (`WID` is the literal placeholder xwinwrap substitutes) |
| `feh` | Static image on X11 | every distro repo | `feh --bg-fill file.png` |
| `gsettings` | GNOME image wallpaper | preinstalled | `gsettings set org.gnome.desktop.background picture-uri file:///abs/path` (also set `picture-uri-dark`) |
| `plasma-apply-wallpaperimage` | KDE image wallpaper | preinstalled on Plasma | `plasma-apply-wallpaperimage /abs/path.png` |

Dependency check at startup: `shutil.which(name)` per tool; the backend reports what's missing with the install hint instead of failing cryptically. This mirrors what the app already does for ffmpeg.

### 4. Backend interface (the seam the split creates)

```python
# wallmotion/platform/base.py
from typing import Protocol

class WallpaperBackend(Protocol):
    name: str
    @staticmethod
    def available() -> bool: ...          # tools present + session supported
    def missing_tools(self) -> list[str]: ...
    def set_image(self, path: str) -> bool: ...
    def set_video(self, path: str, muted: bool, volume: float) -> bool: ...
    def stop(self) -> None: ...
    def set_muted(self, muted: bool) -> None: ...
    def set_volume(self, volume: float) -> None: ...
```

- `WindowsBackend` wraps today's `VideoWallpaperWindow` / `set_static_wallpaper` verbatim — zero behavior change, just moved.
- `LinuxProcessBackend` base holds a `subprocess.Popen` handle; `stop()` terminates the process group (`os.killpg` — spawn with `start_new_session=True` so `mpvpaper`'s children die too).
- Backends: `X11Backend` (xwinwrap/feh), `WlrootsBackend` (mpvpaper/swww), `KdeBackend` (mpvpaper/plasma-apply-wallpaperimage), `GnomeBackend` (gsettings only; `set_video` returns False with the Hanabi note).
- `detect_backend()` walks the §2 matrix and returns the first `available()` backend. Cache the choice; re-detect on "Re-measure display".

### 5. Live volume/mute on Linux — via mpv IPC (phase 3, not v1)

`mpvpaper` forwards `-o` options to mpv, so spawn with `-o "volume=60 mute=no"`. For *live* changes without restarting, use mpv's JSON IPC — the same feature the app already has on Windows:

```python
# spawn with: -o "input-ipc-server=/tmp/wallmotion-mpv.sock"
# then:
import json, socket
sock = socket.socket(socket.AF_UNIX)
sock.connect("/tmp/wallmotion-mpv.sock")
sock.sendall(json.dumps({"command": ["set_property", "volume", 60]}) .encode() + b"\n")
```

v1 may ship video-muted-only on Linux if IPC is deferred — acceptable, mark it.

### 6. GNOME Wayland — the honest gap

No public mechanism exists: no layer-shell, no root window. The only working approach is a **GNOME Shell extension** painting video into the background actor — exactly what the **Hanabi** extension (github.com/jeffshee/gnome-ext-hanabi) does. Options, in order of effort:

1. v1: detect GNOME-Wayland → video button disabled + tooltip "GNOME Wayland needs the Hanabi extension" + README note.
2. Later: if Hanabi is installed, write its video path via `gsettings`/`dconf` (it stores `video-path` in its own schema) — cheap interop, no extension code needed.
3. Eventually: ship a minimal own extension. Only if demand shows up.

### 7. Platform paths (do this during the split)

Introduce one helper and use it everywhere — currently `CONFIG_PATH` and `YT_DIR` are hardcoded:

```python
# wallmotion/paths.py
def app_dirs() -> dict:
    if sys.platform == "win32":
        return {
            "config": Path.home() / ".live_wallpaper_config.json",      # keep for compat
            "downloads": Path(_app_base_dir()) / "downloads",           # keep for compat
            "log": Path(tempfile.gettempdir()) / "live_wallpaper_debug.log",
        }
    xdg_cfg = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    xdg_data = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    xdg_state = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
    return {
        "config": xdg_cfg / "wallmotion" / "config.json",
        "downloads": xdg_data / "wallmotion" / "downloads",
        "log": xdg_state / "wallmotion" / "debug.log",
    }
```

### 8. Autostart on Linux

Write `~/.config/autostart/wallmotion.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=WallMotion
Exec=/path/to/wallmotion
X-GNOME-Autostart-enabled=true
```

### 9. Phased rollout

| Phase | Scope | Effort |
|---|---|---|
| 0 | Package split + `platform/` seam + `paths.py` (no Linux code yet) | Small |
| 1 | Static image on Linux: all four image paths from §2 | ~1 day |
| 2 | Video on wlroots (`mpvpaper`) + X11 (`xwinwrap`+`mpv`); KDE included; dep-check UI hints | 2–3 days |
| 3 | mpv IPC volume/mute, GNOME-Hanabi interop, AppImage in CI | ~2 days |

### 10. Edge cases to handle

- **Multi-monitor:** `mpvpaper` accepts an output name or `*`; `xwinwrap -g` takes `WxH+X+Y` per monitor — spawn one instance per output. Start with all-outputs (`*`) / primary only, per-monitor selection later.
- **Compositor/session restart:** mpvpaper dies with the compositor — detect child exit and respawn on next "set".
- **Missing tools:** never traceback — status bar message "install `mpvpaper` for video wallpaper on Wayland".
- **Resolution change:** `swww img` re-apply; mpvpaper handles it itself.
- **Sleep/resume:** mpv survives; verify on resume, respawn if the child died.

### 11. Testing strategy on Linux

- Unit-test `detect_backend()` and `app_dirs()` with `monkeypatch.setenv` — fully testable headless.
- Integration needs real sessions; keep a manual matrix (X11 VM, Sway, KDE-Wayland, GNOME-Wayland) in the PR template. CI can at least assert `import` + detection fallbacks.

### 12. Packaging on Linux

AppImage first (PyInstaller on `ubuntu-latest` produces it via `appimagetool` or `linuxdeploy`). **Flatpak is a bad fit** — the sandbox can't see host binaries like `mpvpaper`/`mpv`, which this design depends on. A native `.deb`/`.rpm` or AUR package is the better eventual target.
