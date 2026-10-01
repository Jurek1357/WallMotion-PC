# WallMotion — Roadmap / TODO

Priorities: **P1** = do first (unblocks everything else), **P2** = next,
**P3** = later / when there's demand. ⭐ = recommended by reviewer
(see `docs/STACK_ANALYSIS.md` for the full reasoning).

## P1 — Foundation

- [x] ⭐ **Split `main.py` into a package.** `main.py` is a 10-line shim,
  the app lives in `wallmotion/`. Zero behavior change — pure move.
- [x] ⭐ **Platform backend seam** (`wallmotion/platform/`): `WindowsBackend`
  wraps the GDI/WorkerW code; `get_backend()` returns the Linux session
  backend on Linux. Interface sketch: `docs/STACK_ANALYSIS.md` appendix §4.
- [x] **Translate code comments to English** during the split (README stays
  bilingual; code becomes English).
- [x] **XDG paths helper** (`paths.py`) — `~/.config`, `~/.local/share`,
  `~/.local/state` on Linux; keep current paths on Windows.
  Spec: `docs/STACK_ANALYSIS.md` appendix §7.
- [x] ⭐ **Extract `STRINGS` into locale JSON files** (`locales/cs.json`,
  `locales/en.json`) — makes the i18n actually extensible and the split cleaner.

## P2 — Platforms & core features

- [x] ⭐ **Linux support v1 (code)** — backends in
  `wallmotion/platform/linux.py` (x11/wlroots/kde/gnome), session detection
  from `$XDG_SESSION_TYPE`/`$XDG_CURRENT_DESKTOP`, static images
  (`feh`/`swww`/`gsettings`/`plasma-apply-wallpaperimage`), video via
  `mpvpaper` / `xwinwrap`+`mpv`, live volume/mute/pause over mpv JSON IPC,
  sysfs battery sensor for autopause, UI wiring with missing-tool messages,
  30+ headless tests. GNOME-Wayland video stays a documented gap (Hanabi).
- [~] **Linux hardware testing matrix** — verify image + video + IPC on
  real sessions, record results in `docs/` and fix fallout.
  GNOME-Wayland verified on the real desktop (image apply/restore,
  CLI, IPC socket, bundled renderers); X11 + wlroots video verified in
  nested compositors (Xephyr, sway) — see `docs/LINUX_TESTING.md`.
  Still open: bare-metal X11, KDE-Wayland, Sway/Hyprland logins.
- [x] **Linux per-monitor outputs** — X11 video targets the chosen monitor
  via `xwinwrap -g WxH+X+Y` (Qt-based monitor list where WinAPI is absent);
  mpvpaper stays all-outputs `*` (output names need hardware enumeration).
- [x] **YouTube playlist support.** Currently `noplaylist: True` in
  `DownloadWorker`. Plan: accept playlist URLs, fetch `extract_flat` entries,
  show a picker dialog, download selected item(s). Keep the 500 MB cap and
  H.264/1080p filters per item. URL validator already allows `/playlist`.
- [x] ⭐ **Pause/resume rules** — pause video when a fullscreen app runs
  (games) or on battery. Lively does this; big perceived-quality win, small
  code (poll foreground window / `GetSystemPowerStatus`).
- [x] **Multi-monitor selection** — per-monitor wallpaper choice. The
  measurement code already detects all monitors.
- [x] **Linux packaging:** AppImage via CI (`appimage.yml`: PyInstaller
  onefile + `appimagetool`, attaches to tag Releases next to the .exe).
  Flatpak is a bad fit (sandbox can't reach host `mpv`).

## P3 — Bigger ideas

- [x] **Web UI for the library** — localhost panel (stdlib `http.server`):
  grid of downloaded videos with ffmpeg thumbnails, one-click apply,
  stop, status. Tray action opens it in the browser.
- [ ] **Wallpaper workshop / sharing platform** — Wallpaper Engine's killer
  feature. Suggested phases: (1) local library folder UI, (2) a plain JSON
  index on GitHub Pages listing community wallpapers + preview thumbnails,
  (3) a real submission flow (PR-based to start — cheap and reviewable).
  Don't build user accounts/backend infra in v1 — a curated index repo is
  enough to start.
- [ ] **Replace emoji in UI strings with `QIcon` icons** — e.g. 🖥 in the
  display label. Emoji render inconsistently across Windows themes and won't
  survive Linux at all; ship SVG/PNG icons in `assets/` instead. Also matches
  the project rule: no emojis in UI.
- [x] **Wallpaper rotation/scheduling** — playlist of local images/videos,
  interval switcher with shuffle, persisted queue.
- [ ] **libmpv decode engine** — kills the H.264-only ceiling (AV1/VP9) and
  unifies Windows + Linux render paths. Evaluate when AV1 YouTube content
  becomes annoying.
- [x] **CLI interface** — `wallmotion --set file.mp4`, `--stop`, `--mute`.
  Single-instance forwarding to the running app, weekly update check
  with tray notice.
- [x] **Per-wallpaper volume memory** — remember mute/volume per file.

## Done

- [x] MIT license, contributing guide, CoC, security policy
- [x] CI (ruff + pytest, Windows + Linux), tag-driven release builds
- [x] Tests for YouTube URL validation and format-selector invariants
- [x] `main.py` imports cleanly on non-Windows (platform guards)
- [x] XDG platform paths (`wallmotion/paths.py`), locale JSON files
  (`locales/cs.json`, `locales/en.json`)
- [x] Pause/resume rules (fullscreen / battery), YouTube playlist picker,
  per-monitor selection
- [x] Branch protection on `main` (no force-push/deletion, CI required)
