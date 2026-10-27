# Changelog

All notable changes to this project will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.3] - 2026-10-27

### Changed
- The Linux `.deb` is now fully self-contained: it installs the app under
  `/opt/wallmotion` and bundles libmpv, ffmpeg, libxdo and the GTK runtime
  (the same AppDir as the AppImage). The package no longer needs distro
  `libmpv2`, `ffmpeg` or `libxdo3`, so it works on a clean desktop without
  multimedia packages; the download is correspondingly larger.
- The `.deb` and AppImage builds share `installer/linux/build-appdir.sh`,
  so both bundle exactly the same libraries.

## [1.3.2] - 2026-10-27

### Fixed
- Linux: a stale single-instance lock (`/tmp/wallmotion-settings.lock`)
  left behind by a crashed or killed instance silently blocked every
  later launch (the app thought it was already running). The lock is
  now checked against `/proc` and removed on a clean exit, so a dead
  owner is taken over instead of blocking forever.
- Launching the app while it is already running (for example clicking
  the desktop icon after hiding the window to the tray) now brings the
  existing window back instead of exiting silently.

### Changed
- A Wayland session now opens the settings window with a persistent
  banner explaining that wallpaper playback needs an X11 session
  ("Ubuntu on Xorg"), instead of looking like a failed start.

## [1.3.1] - 2026-10-27

### Fixed
- Linux `.deb`: declare the `libxdo3` runtime dependency. The binary
  links `libxdo.so.3` (via the tray/menu stack) but the package did not
  require it, so on a clean Ubuntu the app installed and then refused
  to start. `apt` now pulls it in automatically.
- Linux packages are built on `ubuntu-24.04` instead of
  `ubuntu-latest`, so a runner migration cannot silently raise the
  glibc requirement above 2.39 (Ubuntu 24.04).

### Changed
- The Linux AppImage is now built from the native Rust app instead of
  the frozen Python/PyInstaller bundle, matching the 1.3.0 X11 render
  backend. libmpv, ffmpeg, libxdo and the GTK runtime are bundled, and
  a `--version` smoke test runs on every build.

## [1.3.0] - 2026-10-26

### Added
- Linux render backend (X11): the native app now sets the wallpaper
  behind desktop icons on X11 sessions, using an override-redirect
  desktop window painted by the in-process mpv player. Includes RandR
  multi-monitor discovery, seamless preroll switching and X11
  fullscreen autopause. Images are displayed through mpv so interval
  rotation keeps working.

### Notes
- Wayland is not supported yet: the app detects a Wayland session and
  reports it clearly instead of drawing a floating window. Use an
  "Xorg" session on Linux for now.

## [1.2.9] - 2026-10-23

### Fixed
- Linux CI: `cargo clippy -D warnings` failed because the global
  hotkey actions are never constructed off Windows (the worker is a
  stub). The enum now allows dead code on non-Windows targets.

## [1.2.8] - 2026-10-23

### Added
- Global media hotkeys: Ctrl+Alt+←/→ skips to the previous/next
  rotation wallpaper, Ctrl+Alt+Space toggles pause and Ctrl+Alt+M
  toggles mute. They are registered on a background thread, so they
  also work while the window is hidden in the tray. A "Shortcuts"
  button in the header opens a cheat-sheet window listing them.

## [1.2.7] - 2026-10-19

### Added
- Drag & drop: drop video/image files from Explorer onto the window
  and they are copied into the wallpaper library (media only, no
  overwrite — name clashes get a ` (2)` suffix), the gallery refreshes
  and the Library tab opens. A full-window hint shows while dragging.

### Changed
- GitHub release notes are now composed automatically at tag time:
  static platform blurb + the matching version section from this
  `CHANGELOG.md` (no more manual `gh release edit` afterwards).

## [1.2.6] - 2026-10-16

### Added
- Animated GIF wallpapers: GIFs play through mpv (looped single,
  rotation-aware) instead of freezing on the first frame. The
  rotation card warns when the playlist holds a GIF (no timeline —
  rotation may stall on one).
- Explorer right-click "Set as wallpaper (WallMotion)" for video /
  image files (installer registers per-extension verbs, removed on
  uninstall; double-click keeps the user's own defaults).

## [1.2.5] - 2026-10-16

### Fixed
- Windows installer no longer re-downloads yt-dlp/ffmpeg when
  already present (`{app}\tools` first, then user/system PATH —
  the app finds both, so a second copy only wasted bandwidth).

## [1.2.4] - 2026-10-16

### Added
- YouTube download codec choice: H.264 (plays everywhere, default)
  or AV1 (~half the file size at the same quality). AV1 falls back
  to H.264 when a video has no AV1 renditions. The app checks
  `ffmpeg -decoders` once at startup and warns when no AV1 decoder
  is found (download still works, playback may stutter on older
  machines without hardware AV1 decode).

## [1.2.3] - 2026-10-16

### Added
- Seamless video switching: the next video pre-rolls on hidden
  canvases while the old one keeps playing, then swaps on the first
  frame — rotation advances without a gap. A failed pre-roll keeps
  the old video instead of going dark.

### Fixed
- Playlist freezing on video end: a lost mpv EndOfFile event left
  rotation waiting forever (keep-open idle on the last frame). The
  `eof-reached` property is now the fallback signal, and `loop-file`
  is set before `loadfile` so single videos loop reliably.
- Rotation cursor drift: manual picks no longer leave the cursor at
  the end of the list (rotation snaps to the playing file when
  enabled, so it continues after it instead of stopping at once).
- Autopause misfires: the maximized Settings window counted as a
  fullscreen app and paused the wallpaper ~2 s after every Play.
  Own-process windows are ignored now, and turning a rule off resumes
  an already-frozen player at once.

## [1.2.2] - 2026-10-15

### Fixed
- Wallpaper flickering to the original wallpaper on multi-monitor
  setups: every canvas fought for the single slot right below the
  desktop icons (z-order watchdog re-pinned every 2 s). Canvases now
  stack in order below the icons and the watchdog runs every 500 ms.
- Flicker on video switch/loop: the canvas was cleared to black with
  GDI, which doesn't stick on Win11 layered canvases (transparent →
  original wallpaper flashed). The previous frame now stays until the
  new first frame arrives; mpv keeps the last frame open (`keep-open`)
  and uses hardware decoding (`hwdec=auto-safe`).

## [1.2.1] - 2026-10-14

### Fixed
- Release/CI builds linked the playback library as `mpv-2.dll` while
  the shipped file is `libmpv-2.dll` — the 1.2.0 exe died at startup
  with "mpv-2.dll missing". The import name is verified in CI now.
  (Updating from ≤1.1.7 still needs one manual step: place
  `libmpv-2.dll` from the release next to the exe, or use the
  installer — the old updater doesn't know about the DLL yet.)

## [1.2.0] - 2026-10-14

### Added
- In-process video playback via libmpv (`wallmotion-mpv` crate):
  no mpv install needed, `libmpv-2.dll` ships next to the exe
  (installer includes it; the in-app updater fetches it when missing).
- Linux `.deb` now depends on `libmpv2` from the distro.

### Changed
- Playback backend rewritten: end-of-file and first-frame arrive as
  player events — the sidecar processes, JSON IPC pipes, brightness
  probe and sync thread are gone.
- Players are terminated before teardown (a bare drop left the old
  video playing as a ghost over the new one).

### Removed
- The `wallmotion-player` mpv sidecar crate and the "mpv not found"
  message; `mpv` is no longer searched on PATH.

## [1.1.7] - 2026-10-13

### Added
- Library grid takes the full window width when nothing is selected;
  the detail panel appears only after clicking a wallpaper.
- Oldest-first library sorting (Name / Newest / Oldest).
- Downloads folder is created on first launch, with a one-time
  notice showing its path.

### Changed
- Library detail column capped at one third of the window
  (max 340 px) instead of half.
- Size sorting removed.

## [1.1.6] - 2026-10-12

### Fixed
- Restart after in-app update: no `start` title-parsing, verbatim
  (`\\?\`) prefix stripped at shell boundaries, autostart entry
  tolerates old unquoted paths.
- Released binaries report the correct internal version (workspace
  bump), so the updater no longer re-offers the installed release.

## [1.1.5] - 2026-10-12

### Added
- In-app updates from GitHub Releases (daily silent check, one-click
  download + restart install, skippable versions).
- Windows installer (Inno Setup, per-user): folder picker, optional
  yt-dlp + ffmpeg download, Start Menu entry, autostart, user-PATH task.
- Linux `.deb` package (Debian/Ubuntu, `ffmpeg` dependency).
- Optional SignPath code signing via manual workflow (see
  `docs/SIGNING.md`).

## [1.1.4] - 2026-10-12

### Added
- Library sorting (name/newest/size) and type filter (all/videos/images).
- Detail meta with localized kind, size and relative age.

### Changed
- Bigger library cards and detail preview; readable selected
  dropdown text on the accent fill.

## [1.1.3] - 2026-10-10

### Added
- Multi-monitor playback: Monitor "all" plays the same video fullscreen
  on every screen (first audible, the rest muted); pause, mute, volume,
  rotation and watchdog cover all runners.
- Frame-sync monitor start: frozen mpv spawn (`--pause`) with a
  brightness-gated threaded unpause, so every screen starts together.
- mpv fast-start flags (`--no-config`, `--load-scripts=no`) and a
  multi-argument IPC command (seek) with tests.

### Changed
- Batch canvas setup + back-to-back mpv spawns for parallel warmup.
- Mute is global again (only volume is remembered per file).
- More readable texts across settings and library.

## [1.1.2] - 2026-10-09

### Added
- Rotation advances on video end (no timer); images dwell on the saved
  interval, broken files are skipped, the last video loops.
- Windows autostart checkbox (Run key, hidden `--minimized` resume).
- `--minimized` CLI flag: start hidden to the tray and resume playback.
- Live wallpaper preview card in Settings; controls left / preview
  right on wide windows (stacked when narrow).
- Collapsible Sound / Rotation / YouTube cards; single-instance port
  gate (a second launch exits instead of forking a duplicate).

### Changed
- Video switches reuse the same canvas (no teardown flash); mute is
  global again (only volume is remembered per file).
- Hide-to-tray keeps the UI loop alive off-screen with no taskbar
  button (Wallpaper-Engine style); rotation keeps playing while hidden.
- Library cards share one thumbnail box size; bigger CTA buttons and
  more readable texts; wide default window (940 px).

### Fixed
- Opaque WorkerW backgrounds covering the video (z-order watchdog
  re-pins the canvas below the icons).
- Duplicate app instances fighting over the desktop.

## [1.1.1] - 2026-10-09

### Added
- yt-dlp + ffmpeg auto-install on first launch (background, no clicks
  needed); the Get buttons stay as manual retry, Update stays for yt-dlp.
- Live wallpaper preview card in Settings (video frames via ffmpeg),
  with file name, play state and Set / Pause / Stop actions.
- Windows autostart: "Start with Windows" checkbox (Run registry value
  launching hidden with `--minimized`, resuming the last wallpaper).
- `--minimized` CLI flag: start hidden to the tray and resume playback.
- Wallpaper-Engine style side panels: controls left / preview right in
  Settings, grid left / detail right in the library (stacked under
  560 px); Sound / Rotation / YouTube cards are collapsible.

### Changed
- Settings reorganized into cards (Preview, Source, Sound & behavior,
  Rotation, YouTube); file picker shows the name only (full path in
  tooltip), volume slider fills the row, full-width Download / Cancel.
- Library count shows the folder name (full path in tooltip); cards have
  fills, fixed thumbnails and truncated names; single weak status line.

## [1.1.0] - 2026-10-08

### Added
- Native `WallMotion-Settings.exe` beta, shipped next to the classic
  app in every release (no Python, no console popups, logo + tray icon).
- CZ/EN switch, light/dark theme + follow-system toggle, brand styling
  (logo header, pill tabs, accent buttons).
- Tabbed wallpaper library, Lively-style: search, favorites (+filter),
  responsive grid, detail panel (Set / Add to rotation / Delete),
  double-click to set. Shares the folder and config with the Python app.
- YouTube downloads in the native app via a standalone yt-dlp sidecar
  with one-click install/update and one-click ffmpeg setup; playlist
  picker with queue, progress bar, Cancel.
- Per-wallpaper volume memory, per-monitor selection, rotation
  playlist, pause on fullscreen/battery, CLI (`--set/--stop/--mute/
  --volume`), hide-to-tray on X with taskbar button while open.

## [1.0.23] - 2026-10-06

### Fixed
- YouTube: ffmpeg is verified to actually execute (blocked binaries
  fall back to progressive mp4); the merge error maps to a Czech hint.
- Downloads fail fast with timeouts instead of hanging on throttled
  networks; new error hints (sign-in, unavailable, timeout).

## [1.0.22] - 2026-10-06

### Added
- Rotation follows video end (videos play fully, then advance);
  interval stays for images only.
- Repeat toggle for rotation (off = stop on the last wallpaper).
- Play and Skip buttons for the rotation list.
- Library preview pane: Set / Add to rotation / Delete buttons.
- Mute checkbox is the master switch (volume memory never unmutes).
- Download progress bar with percent under the YouTube row; faster
  failure with Czech messages on throttled networks.

### Fixed
- Video window state back in `__init__` (was trapped in a handler,
  broke all playback) + AST regression test.

## [1.0.21] - 2026-10-05

### Fixed
- Single-instance lock: a second launch forwards its command to the
  running app and exits (no more duplicate instances fighting over
  the desktop and config).

## [1.0.20] - 2026-10-05

### Added
- Autostart checkbox (Windows .lnk, Linux .desktop).
- Follow-system-theme checkbox with live polling.
- Favorites in the library (star + filter, persisted).
- Wallpapers auto-apply on launch (autostart friendly).

### Fixed
- Favorites sync between panel and window.
- Tab bar and panes follow the light theme.

## [1.0.19] - 2026-10-02

### Changed
- Volume slider handle is a painted pin (rectangle + triangle tip)
  instead of the default look.

## [1.0.18] - 2026-10-02

### Changed
- Smaller, more compact buttons (padding, font, corners) and toggle
  buttons; initial window 700×800.

## [1.0.17] - 2026-10-02

### Changed
- Library is a tab in the main window (settings | library, grid +
  live preview + search) instead of a separate dialog; tray action
  and button switch to the tab.

## [1.0.16] - 2026-10-02

### Changed
- Library opens as a native in-app dialog (grid + preview + search)
  instead of the external browser.

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

[Unreleased]: https://github.com/Jurek1357/WallMotion-PC/compare/v1.0.23...HEAD
[1.0.23]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.23
[1.0.22]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.22
[1.0.21]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.21
[1.0.20]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.20
[1.0.19]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.19
[1.0.18]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.18
[1.0.17]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.17
[1.0.16]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.16
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
