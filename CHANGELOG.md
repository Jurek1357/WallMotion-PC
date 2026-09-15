# Changelog

All notable changes to this project will be documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Open-source project files: MIT license, contributing guide, code of
  conduct, security policy, issue/PR templates, CI and release workflows.

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

[Unreleased]: https://github.com/Jurek1357/WallMotion-PC/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Jurek1357/WallMotion-PC/releases/tag/v1.0.0
