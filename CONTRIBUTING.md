# Contributing to WallMotion

Thanks for your interest in contributing. This document covers how to set up
the project, the workflow for pull requests, and the conventions used.

## Development setup

Requires Python 3.9+ on Windows (the app itself is Windows-only for now;
Linux support is planned).

```bash
git clone https://github.com/Jurek1357/WallMotion-PC.git
cd WallMotion-PC
python -m venv .venv
.venv\Scripts\activate          # Windows
# source .venv/bin/activate     # Linux (CI/tooling only)
pip install -r requirements.txt -r requirements-dev.txt
python main.py
```

The module imports cleanly on Linux too (Win32 calls are platform-guarded),
so linting and the unit tests run everywhere.

## Workflow

1. Fork the repository and create a branch from `main`:
   `git checkout -b fix/short-description` or `feat/short-description`
2. Make your change. Keep PRs small and focused — one concern per PR.
3. Before pushing, run:

   ```bash
   ruff check .
   pytest
   ```

4. Open a pull request against `main` and fill in the template.
   CI must be green before merge.

## Conventions

- **Comments and docstrings in English.** Older parts of the code are still
  in Czech and are being translated — new code must be English, and
  translating a nearby comment while you touch it is welcome.
- Commit messages: short imperative subject; mention *why*, not just *what*.
  The project history uses Czech — English is fine and preferred for
  open-source collaboration.
- No emojis in code, UI strings, or docs.
- Don't commit secrets. `downloads/`, `dist/`, `build/`, `.venv/` are
  gitignored.
- Python 3.9-compatible syntax (`from __future__ import annotations` is
  already in place for `X | None` annotations).

## Testing

- `pytest` covers the pure logic (URL validation, format selectors, image
  fitting). If you add logic that can run without a display, add a test.
- GUI and the video canvas are verified manually on Windows: describe your
  manual test steps in the PR template.

## Reporting bugs

Use the *Bug report* issue template. Always attach
`%TEMP%\live_wallpaper_debug.log` if the video wallpaper misbehaves —
it exists specifically to make remote debugging possible.

## Where to help

- **Linux support** is the main roadmap item: the plan is a
  `wallmotion/platform/` backend layer (Windows canvas + `mpvpaper` /
  `xwinwrap` on Linux). Discussion in issues before large changes, please.
- Translations, packaging, docs and tests are always welcome.
