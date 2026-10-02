# Changelog

All notable changes to this project will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.15] - 2026-10-02

### Added
- In-app library v2: Wallpaper-Engine-style two panes (thumbnail
  grid, live preview, search filter), native dialog instead of the
  browser.

## [1.0.14] - 2026-10-02

### Added
- In-app wallpaper library (no browser needed): thumbnail grid with
  live preview pane, search filter, one-click apply.

## [1.0.13] - 2026-10-01

### Added
- Library button in the main window (next to the videos folder).

## [1.0.12] - 2026-10-01

### Fixed
- Multi-monitor z-order: mirror canvases chain below the previous
  canvas, WorkerW moves only once (second monitor stayed black).

## [1.0.11] - 2026-10-01

### Fixed
- Canvas window class registration falls back to kernel32 when the
  user32 lookup fails in the frozen build (was logging an error and
  using the plain STATIC class).

## [1.0.10] - 2026-10-01

Linux hardware-testing fallout: fixes from the first real-hardware pass
(GNOME Wayland, Ubuntu 26.04) and self-contained AppImage renderers.
Full report: [docs/LINUX_TESTING.md](docs/LINUX_TESTING.md).

### Added
- The AppImage now bundles `feh`, `mpv`, `xwinwrap`, `mpvpaper`,
  `swww` and `swww-daemon` built from pinned sources; no manual
  renderer installs needed. Renderer lookup order: bundled dir,
  `~/.local/share/wallmotion/bin`, then `$PATH`.
- `stop` restores the previous Linux wallpaper (GNOME `gsettings`
  snapshot, X11 `~/.fehbg` snapshot, wlroots `swww query` snapshot).
- Fullscreen auto-pause probe on X11 (`xprop`/`xwininfo`/`xrandr`).
- GNOME Wayland video wallpaper via the Hanabi extension: the app
  offers to install the upstream-built zip bundled in `assets/`
  (GPL-3.0, license text shipped alongside), queues it in
  `enabled-extensions`, and drives its `video-path`/`mute`/`volume`
  keys — stop/pause disable the extension to actually halt the
  renderer (clearing `video-path` alone leaves it playing). First
  activation needs one sign-out/in (GNOME Wayland cannot load a new
  extension into the running session).
- `assets/VERSION` version file written by CI; `--version` reports the
  release tag instead of a static dev string.

### Fixed
- Session-integrated tools (`gsettings`, `plasma-apply-wallpaperimage`,
  X11 autopause probes) are resolved against `/usr/bin`, `/bin`,
  `/usr/local/bin` before `$PATH`: a Linuxbrew/Conda shadow binary can
  use a different settings backend (keyfile), so wallpaper writes
  silently never reached GNOME's dconf store.
- `debug.log` is no longer truncated by every CLI invocation; it
  accumulates across forwarded commands.
- A stale `wallmotion-mpv.sock` left behind by a killed player no
  longer dead-ends the next video's IPC channel; it is unlinked before
  the new spawn.
- Muted video start on Linux used `no-audio`, permanently dropping the
  audio stream; now uses `mute=yes` so IPC unmute works.
- Static images on Linux were re-encoded to a BMP in `/tmp`; the source
  file is applied directly (all Linux renderers scale natively).
- `_quiet_ffmpeg` now handles Linux `libavutil.so.N` library names.

## [1.0.9] - 2026-10-01

### Added
- Duplicate video wallpaper: "All monitors" on 2+ screens plays the
  same video fullscreen on each screen (audio from the primary,
  mirrors muted).
- Pause/Resume button (window + tray): freezes the video frame without
  touching the wallpaper; autopause never overrides a manual pause.

### Fixed
- Frame stride guard: decoder frames with padded scanlines are
  repacked before GDI painting (used to paint sheared bands).

## [1.0.8] - 2026-10-01

### Added
- Web library: localhost panel (`http://127.0.0.1:8765/`) with a grid
  of downloaded videos (ffmpeg thumbnails) and images, one-click
  apply and stop; tray action opens it in the browser.

### Fixed
- Light theme left the window background dark; content background now
  follows the theme. Language/theme toggles moved into the title row.

## [1.0.7] - 2026-10-01

### Fixed
- Main window is scrollable/resizable instead of fixed size (widgets
  overlapped on larger font scaling, Stop button unreachable).
- YouTube button text "Download & set" rendered as "Download set"
  (`&` is a Qt mnemonic); now "Download and set".

## [1.0.6] - 2026-10-01

### Changed
- Language and theme dropdowns replaced by toggle buttons (EN/CZ
  and painted sun/moon icon).

## [1.0.5] - 2026-10-01

Linux support v1, CLI, AppImage packaging and all P1/P2 features below.

### Added
- Open-source project files: MIT license, contributing guide, code of
  conduct, security policy, issue/PR templates, CI and release workflows.
- Platform paths helper (`wallmotion/paths.py`): XDG directories
  (`~/.config`, `~/.local/share`, `~/.local/state`) on Linux, legacy
  locations kept on Windows.
- UI strings extracted into locale files (`locales/cs.json`,
  `locales/en.json`) loaded by `wallmotion/i18n.py`.
- Auto-pause rules: video pauses when a fullscreen app runs (games)
  and optionally on battery power; both checkboxes in the UI,
  remembered between launches.
- YouTube playlist support: `/playlist` links open a picker dialog,
  selected videos download sequentially (500 MB cap and H.264/1080p
  guards apply per item).
- Multi-monitor selection: video wallpaper (and image fit) can target
  one monitor or span all; choice remembered between launches.
- Linux support v1: session detection (`XDG_SESSION_TYPE` /
  `XDG_CURRENT_DESKTOP`), image backends (`feh`, `swww`, `gsettings`,
  `plasma-apply-wallpaperimage`), video via `mpvpaper` / `xwinwrap`+`mpv`,
  live volume/mute/pause over mpv JSON IPC, sysfs battery sensor,
  missing-tool messages in the UI. GNOME-Wayland video remains
  unsupported (Hanabi extension needed). Not yet tested on hardware.
- Linux per-monitor video: chosen monitor drives `xwinwrap -g WxH+X+Y`
  on X11 (monitor list from Qt data); mpvpaper stays all-outputs.
- CLI: `wallmotion --set FILE --stop --mute/--unmute --volume N`
  with single-instance forwarding to the running app.
- Update check: weekly GitHub Releases poll with tray notice,
  manual check in the tray menu.
- Linux packaging: AppImage built in CI (`appimage.yml`) and attached
  to tag Releases next to the Windows .exe.
- Per-wallpaper volume memory: mute/volume is remembered for each file
  (last 100) and restored on selection.
- Wallpaper rotation: queue of local images/videos with 1 min–1 h
  interval and shuffle, persisted between launches.
- Code comments translated to English (user-facing strings stay
  Czech/English in `locales/`).

### Fixed
- Linux CI: `wallmotion/screens.py` imports Qt lazily so pure logic
  and tests work headless (no `libEGL` needed).

## [1.0.0] - 2026-09-14

First stable release.

### Added
- Static image wallpaper auto-fitted to measured screen resolution.
- Video wallpaper behind desktop icons (Windows 10 + Windows 11
  "raised desktop"), click-through and focus-safe.
- YouTube download in background (H.264 only, up to 1080p, max 500 MB,
  always with audio) via yt-dlp; bundled ffmpeg via imageio-ffmpeg.
- Mute checkbox and live volume slider.
- Dark/light themes, Czech/English UI, persisted config.
- Multi-monitor measurement (physical pixels, HiDPI aware), system tray,
  debug log at `%TEMP%\live_wallpaper_debug.log`.
- Standalone `WallMotion.exe` (PyInstaller, ~65 MB).

[Unreleased]: https://github.com/Jurek1357/WallMotion-PC/compare/v1.0.15...HEAD
[1.0.15]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.15
[1.0.14]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.14
[1.0.13]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.13
[1.0.12]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.12
[1.0.11]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.11
[1.0.10]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.10
[1.0.9]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.9
[1.0.8]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.8
[1.0.7]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.7
[1.0.6]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.6
[1.0.5]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.5
[1.0.0]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.0
