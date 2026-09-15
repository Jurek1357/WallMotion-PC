# WallMotion — Roadmap / TODO

Priorities: **P1** = do first (unblocks everything else), **P2** = next,
**P3** = later / when there's demand. ⭐ = recommended by reviewer
(see `docs/STACK_ANALYSIS.md` for the full reasoning).

## P1 — Foundation

- [ ] ⭐ **Split `main.py` into a package.** 2000 lines in one file is the
  #1 contributor blocker. Target layout and rationale:
  `docs/OPENSOURCE_COMPLIANCE.md` §2. Zero behavior change — pure move.
- [ ] ⭐ **Platform backend seam** (`wallmotion/platform/`): wrap the existing
  Windows code behind a `WallpaperBackend` interface. This is the Linux port
  map — do it *during* the split, not after. Interface sketch:
  `docs/STACK_ANALYSIS.md` appendix §4.
- [ ] ⭐ **Translate code comments to English** during the split (README stays
  bilingual; code becomes English).
- [ ] **XDG paths helper** (`paths.py`) — `~/.config`, `~/.local/share`,
  `~/.local/state` on Linux; keep current paths on Windows.
  Spec: `docs/STACK_ANALYSIS.md` appendix §7.
- [ ] ⭐ **Extract `STRINGS` into locale JSON files** (`locales/cs.json`,
  `locales/en.json`) — makes the i18n actually extensible and the split cleaner.

## P2 — Platforms & core features

- [ ] ⭐ **Linux support** — full research + step-by-step plan in
  `docs/STACK_ANALYSIS.md` appendix. Summary: delegate rendering to
  `mpvpaper` (Wayland), `xwinwrap`+`mpv` (X11), `swww`/`feh`/`gsettings`/
  `plasma-apply-wallpaperimage` for static images. GNOME-Wayland video is a
  documented gap (needs a shell extension, e.g. Hanabi).
- [ ] **YouTube playlist support.** Currently `noplaylist: True` in
  `DownloadWorker`. Plan: accept playlist URLs, fetch `extract_flat` entries,
  show a picker dialog, download selected item(s). Keep the 500 MB cap and
  H.264/1080p filters per item. URL validator already allows `/playlist`.
- [ ] ⭐ **Pause/resume rules** — pause video when a fullscreen app runs
  (games) or on battery. Lively does this; big perceived-quality win, small
  code (poll foreground window / `GetSystemPowerStatus`).
- [ ] **Multi-monitor selection** — per-monitor wallpaper choice. The
  measurement code already detects all monitors.
- [ ] **Linux packaging:** AppImage via CI (`ubuntu-latest` + PyInstaller +
  `linuxdeploy`). Flatpak is a bad fit (sandbox can't reach host `mpv`).

## P3 — Bigger ideas

- [ ] **Web UI for the library** — localhost panel (FastAPI or a simple
  `http.server` backend + static page): grid of downloaded videos with
  thumbnails (ffmpeg `-frames:v 1`), one-click apply, settings, status.
  Useful for remote control and for a headless/Tray-free mode.
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
- [ ] **Wallpaper rotation/scheduling** — playlist of local images/videos,
  interval switcher. Natural extension once a library exists.
- [ ] **libmpv decode engine** — kills the H.264-only ceiling (AV1/VP9) and
  unifies Windows + Linux render paths. Evaluate when AV1 YouTube content
  becomes annoying.
- [ ] **CLI interface** — `wallmotion --set file.mp4`, `--stop`, `--mute`.
  Scriptable, near-free once the backend seam exists.
- [ ] **Update check** — poll GitHub Releases API, show "new version" in tray.
- [ ] **Per-wallpaper volume memory** — remember mute/volume per file.

## Done

- [x] MIT license, contributing guide, CoC, security policy
- [x] CI (ruff + pytest, Windows + Linux), tag-driven release builds
- [x] Tests for YouTube URL validation and format-selector invariants
- [x] `main.py` imports cleanly on non-Windows (platform guards)
