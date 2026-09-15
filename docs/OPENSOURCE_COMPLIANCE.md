# WallMotion-PC — Open-Source Compliance & GitHub Setup Guide

Goal: turn the repo from "public code dump" (community health 28 %) into a properly open-sourced, contribution-ready project. Ordered by impact — the first three items are blockers, the rest is polish.

---

## 1. License — the actual blocker

**Status: missing. Without a `LICENSE` file the code is "all rights reserved" — nobody may legally use, fork, or contribute, regardless of the repo being public.**

Recommendation: **MIT**. Permissive, standard for desktop utilities, matches how Lively-adjacent tooling ships. If the author wants derivatives to stay open, use **GPL-3.0** — but MIT maximizes contribution.

```bash
# add LICENSE with MIT text, copyright "Jurek1357 (or legal name) 2026"
```

One caveat to flag in README: the bundled **ffmpeg binary** (via `imageio-ffmpeg`) is LGPL/GPL-licensed itself — fine to redistribute, but the LICENSE should note the app code is MIT while bundled ffmpeg keeps its own license. Same note for yt-dlp (Unlicense — no conflict).

## 2. Split `main.py` — the contribution enabler

2011 lines in one file is the #1 reason nobody will send a PR. Target layout:

```
wallmotion/
  __main__.py        # entry: main()
  app.py             # QApplication setup, tray, theme/lang plumbing
  ui/
    main_window.py   # MainWindow
    drop_zone.py     # DropZone
    theme.py         # THEMES, build_stylesheet
    strings.py       # STRINGS, LANGS, T()
  canvas/
    video_window.py  # VideoWallpaperWindow (Qt side)
    gdi.py           # _BitmapInfo, StretchDIBits plumbing, blit
  win32/
    desktop.py       # Progman/WorkerW/DefView discovery, z-order
    screens.py       # measure_screens, virtual rect, DPI
  wallpaper.py       # set_static_wallpaper, fit_image_to_screen
  download.py        # DownloadWorker, is_valid_youtube_url, format ladders
  config.py          # CONFIG_PATH load/save
  assets/            # icon.ico, icon.png
main.py              # 3-line shim: from wallmotion.__main__ import main
```

While splitting: **translate code comments/docstrings to English.** Keep the README bilingual — that's good and already done — but code must be English to be contribute-able.

## 3. CI/CD — stop hand-building the exe

Currently `dist\WallMotion.exe` is built locally and uploaded to Releases by hand. Two workflows fix everything:

**`.github/workflows/ci.yml`** — on every push/PR:

```yaml
name: CI
on: [push, pull_request]
jobs:
  check:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with: { python-version: "3.12" }
      - run: pip install ruff pytest -r requirements.txt
      - run: ruff check .
      - run: pytest
```

**`.github/workflows/release.yml`** — on `v*` tags: build exe with PyInstaller on `windows-latest`, attach to a GitHub Release (`softprops/action-gh-release` or `gh release create`). The README already documents the exact PyInstaller flags — move them into the workflow verbatim.

Also fix `.gitignore`: it currently ignores `*.spec`, so no build recipe is versioned. Either commit a `WallMotion.spec` or keep flags in CI — not neither.

## 4. Community files (all standard, all cheap)

| File | Content |
|---|---|
| `LICENSE` | MIT (see §1) |
| `CONTRIBUTING.md` | Setup steps (`pip install -r requirements.txt`, run from source), branch/PR rules, "comments in English", how to test |
| `CODE_OF_CONDUCT.md` | Contributor Covenant 2.1 — GitHub generates it via the community wizard |
| `SECURITY.md` | One paragraph: report vulns via GitHub private advisory, not public issues |
| `CHANGELOG.md` | Keep a Changelog format, backfill v1.0.0 |
| `.github/ISSUE_TEMPLATE/bug_report.yml` | Version, Windows build, repro steps, `%TEMP%\live_wallpaper_debug.log` attachment field — the debug log already exists, use it |
| `.github/ISSUE_TEMPLATE/feature_request.yml` | Standard |
| `.github/pull_request_template.md` | Summary / test plan / screenshots |
| `.github/dependabot.yml` | `pip` + `github-actions` ecosystems, weekly |

GitHub's **Settings → Community standards** wizard scaffolds most of these; don't write CoC by hand.

## 5. README polish — biggest visibility win

The README is already solid technically. What's missing for an open-source repo:

- **A screenshot or GIF at the top.** It's a wallpaper app with no visuals — this alone probably explains the 2 stars. A 5-second GIF of video-behind-icons does more than any feature list.
- **Badges:** license, release version, CI status, downloads count.
- Download section pointing at Releases **before** the build-from-source section.
- English section first (already done — keep).

## 6. Repo settings (2 minutes in Settings)

- **Topics** (currently empty): `wallpaper`, `live-wallpaper`, `windows`, `pyside6`, `yt-dlp`, `desktop-app`, `wallpaper-engine-alternative`
- Description is set — fine. Add `https://` homepage only if a site exists; otherwise point it at Releases.
- **Branch protection on `main`:** require PR, require CI green, no force-push. Even for a solo repo — it makes releases deliberate.
- Wiki: disable it (unused) or document build internals there.
- Releases: enable "auto-generated release notes" via `.github/release.yml` to categorize PRs by label.

## 7. Dependency & release hygiene

- `requirements.txt` uses `>=` ranges — acceptable for dev installs, but **release builds need pinned versions** or exe behavior drifts. Add `requirements-lock.txt` via `pip-compile` (pip-tools) or switch to `uv lock`.
- Single `__version__` in one module; tag releases `v1.x.y` (semver); conventional commits make auto-changelogs work.
- **Code signing reality check:** unsigned PyInstaller exes get SmartScreen warnings. Free path for OSS: SignPath.io certificate. Cheap path: accept it and note it in README ("click More info → Run anyway").

## 8. Testing floor

GUI is hard to test; the pure logic is not. After the split, cover first:

- `is_valid_youtube_url` — it is already a well-factored pure function with clear accept/reject tables.
- `fit_image_to_screen` crop math.
- yt-dlp format selector strings (assert `vcodec^=avc1` and `height<=1080` invariants).
- `measure_screens` — smoke-level on CI (Windows runner has a real desktop session).

## Priority checklist for the maintainer

- [ ] `LICENSE` (MIT) — **do this first, nothing else counts until it exists**
- [ ] Split `main.py` into `wallmotion/` package + English comments
- [ ] `release.yml` — tag → CI-built exe → GitHub Release
- [ ] `ci.yml` — ruff + pytest on windows-latest
- [ ] Screenshot/GIF in README + badges
- [ ] `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue/PR templates, `dependabot.yml`
- [ ] Topics, branch protection, disable unused wiki
- [ ] `SECURITY.md`, `CHANGELOG.md`, pinned lock file

## What a good first PR from you looks like

Easiest high-value contributions, in order:

1. **`LICENSE` PR** — one file, unblocks legality, shows intent.
2. **Package split** — mechanical, reviewable, transformative for the project.
3. **CI workflow** — needs the split only for pytest to matter; ruff works today.
4. **English comment translation** — pairs naturally with the split.

Each is a separate, small PR — easier to review, easier to merge.
