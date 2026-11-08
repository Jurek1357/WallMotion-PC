# WallMotion — Roadmap / TODO

The **native Rust app** (`wallmotion-rs/`, shipped as
`WallMotion-Settings.exe`) is the active product. The Python app in
`wallmotion/` (`WallMotion.exe`) is **legacy**: it stays in the sources
and in older releases and is no longer developed.

Priorities: **P2** = next, **P3** = later / when there's demand.

## P2 — Core gaps of the native app

- [x] **Drag & drop a file onto the window.** Dropped videos/images are
  copied into the library folder (media only, never overwriting, name
  clashes get a ` (2)` suffix), the gallery refreshes and the Library
  tab opens. A full-window hint shows while a file hovers.
- [x] **Linux render backend for the native binary.** X11 uses the
  override-redirect desktop canvas + mpv `wid`, RandR monitor list and
  X11 fullscreen sensor (images play through mpv). Wayland lives in
  `settings/src/linux_desktop.rs`: GNOME (`gsettings` + Hanabi), KDE
  images (`plasma-apply-wallpaperimage`, no video), wlroots (`swww` +
  `mpvpaper` with mpv JSON IPC for pause/mute/volume), plus restore on
  Stop, startup dialogs and persistent logging. Linux build, clippy and
  unit tests pass; hardware runs on GNOME/KDE/wlroots are still wanted
  (checklist: [docs/LINUX_HARDWARE_TEST.md](docs/LINUX_HARDWARE_TEST.md)).

## P3 — Bigger ideas

- [ ] **Wallpaper workshop / sharing platform** — Wallpaper Engine's
  killer feature. Suggested phases: (1) local library folder UI, (2) a
  plain JSON index on GitHub Pages listing community wallpapers +
  preview thumbnails, (3) a real submission flow (PR-based to start —
  cheap and reviewable). Don't build user accounts/backend infra in v1.
- [ ] **Code signing (Windows)** — Smart App Control blocks the unsigned
  `.exe` on fresh machines. Options: paid code-signing certificate,
  and/or submit each release `.exe` to Microsoft malware analysis
  (free, builds SmartScreen reputation over time). `sign.yml` +
  `docs/SIGNING.md` are ready; only the SignPath secrets/cert are
  missing.

## Done — native app

- [x] **Core:** paths, session detection, rotation queue, autopause
  rules, volume memory, YouTube pure logic (URL/playlist/progress/
  errors) — all unit-tested, no-dependency `wallmotion-core`.
- [x] **Windows render:** WorkerW discovery, GDI canvas, cover-fit
  static images, per-monitor + virtual-screen targets,
  fullscreen/battery sensors.
- [x] **Playback:** in-process libmpv (`wallmotion-mpv` crate, `wid`
  embed, pause/mute/volume as properties, EndOfFile/PlaybackRestart
  events) — no sidecar processes, no IPC pipes, no brightness probe.
  GIF wallpaper support, AV1 download codec.
- [x] **Settings window (egui):** tabs Settings / Wallpaper library
  (Lively-style grid, search, favorites, detail, double-click to set),
  CZ/EN (shared `locales/*.json`), light/dark + follow-system, brand
  styling (logo, pill tabs, accent CTAs), responsive rows, hide-to-tray
  on X with taskbar button, tray icon + menu, single instance + remote
  CLI, rotation UI, volume memory wiring.
- [x] **YouTube in Rust:** standalone yt-dlp sidecar with one-click
  install/update, one-click ffmpeg setup, playlist picker + queue,
  progress bar, Cancel, localized errors, auto-install on first launch.
- [x] **Autostart** at logon with `--minimized`.
- [x] **Global media hotkeys:** Ctrl+Alt+←/→ previous/next rotation
  wallpaper, Ctrl+Alt+Space pause, Ctrl+Alt+M mute — registered on a
  background thread, so they work with the window hidden in the tray.
- [x] **In-app updater:** checks GitHub releases, downloads and installs
  the new exe (+ libmpv DLL) and restarts.
- [x] **Packaging & releases:** tag pipeline builds + publishes
  `WallMotion-Settings.exe` + `libmpv-2.dll` + Windows installer
  (`WallMotion-Setup-*.exe`) + `wallmotion_*.deb` + AppImage; release
  notes are composed from `CHANGELOG.md` automatically.
- [x] **Explorer integration:** right-click "Set as wallpaper
  (WallMotion)" verbs registered by the installer.

## Done — shared / repo

- [x] MIT license, contributing guide, CoC, security policy
- [x] CI (Rust on Windows + Linux; Python ruff/pytest kept for the
  legacy tree), tag-driven release builds
- [x] Branch protection on `main` (no force-push/deletion, CI required)
- [x] Dependabot for GitHub Actions + Python deps, kept green

## Legacy — Python app (frozen, not developed)

The Python tree (`wallmotion/`, `locales/`, `assets/`,
`WallMotion.spec`, `installer/`) is kept buildable and tested in CI so
old releases and the sources stay usable, but new work happens only in
Rust. Everything that used to live on this roadmap (packaging split,
platform seam, XDG paths, locale JSON, Linux backends, YouTube
playlist, pause/resume, per-monitor selection, Web UI, rotation) is
already implemented there.
