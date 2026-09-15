# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| Latest release | Yes |
| Older releases | No — please update first |

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Use GitHub's private reporting instead:
**Security → Report a vulnerability** on this repository
(GitHub Security Advisories).

Include: affected version, steps to reproduce, and the debug log
(`%TEMP%\live_wallpaper_debug.log`) if relevant.

You can expect an acknowledgement within a few days. This is a hobby
project maintained in spare time — thank you for your patience.

## Scope notes

- The app downloads video via `yt-dlp` from validated YouTube URLs only
  (scheme + domain allowlist, no raw IPs). If you find a way to bypass
  that validation, that is a security issue — please report it.
- The PyInstaller exe is unsigned; SmartScreen warnings are expected and
  are **not** a security vulnerability. Distribution via official
  [Releases](../../releases) only.
