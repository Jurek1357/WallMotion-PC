# Linux hardware testing report

Linux v1 was landed code-reviewed but never run on hardware
(TODO: "Linux hardware testing matrix"). This is the first pass:
the v1.0.5 release AppImage on a real machine, findings, fixes, and a
re-test of a locally rebuilt AppImage with bundled renderers.

> **Headline finding.** On this machine `gsettings` resolved through
> PATH to **Linuxbrew's** `~/.linuxbrew/bin/gsettings`, which uses the
> **keyfile backend** (`~/.config/glib-2.0/settings/keyfile`). Every
> `gsettings set` the app made — apply, snapshot, restore — vanished
> into that shadow store while `gsettings get` echoed it back, so all
> reads looked consistent and nothing ever reached the dconf database
> GNOME actually renders. The wallpaper visibly never changed, and the
> earlier round-trip "verification" was consistent only inside the
> shadow store. Session-integrated tools are now resolved against
> canonical system dirs (`/usr/bin`, `/bin`, `/usr/local/bin`) before
> PATH (`session_tool()`); verification below is through
> `/usr/bin/dconf`, not `gsettings`.

## Environment

| | |
|---|---|
| OS | Ubuntu 26.04.1 LTS |
| Kernel | 7.0.0-34-generic, x86_64 |
| Session | Wayland |
| Desktop | `ubuntu:GNOME` (`XDG_CURRENT_DESKTOP`), Mutter |
| Display | 1920x1200 |
| Build under test | `WallMotion-x86_64.AppImage` v1.0.5 (release), plus a locally rebuilt AppImage from the fix branch |

## Verified working (v1.0.5 + fixed build)

- AppImage boots on Wayland, Qt HiDPI scaling correct.
- Session detection: `session=wayland desktop=ubuntu,gnome → backend=gnome`.
- XDG paths created: `~/.config/wallmotion`, `~/.local/share/wallmotion`,
  `~/.local/state/wallmotion`.
- Static image apply via `gsettings` — both `picture-uri` and
  `picture-uri-dark` written to the real dconf db (verified through
  `/usr/bin/dconf` and visually: desktop turns blue, then back).
- CLI single-instance forwarding: `--set`, `--stop`, `--version`
  forward over `wallmotion.cli.sock` to the running instance.
- `--stop` restores the previous wallpaper (fixed build) — confirmed
  against the user's real custom wallpaper, not a default.
- Video apply on GNOME offers to install the bundled Hanabi
  extension zip (`gnome-extensions install --force`), queues it in
  `enabled-extensions`, and asks for a sign-out/in — verified live:
  files land in `~/.local/share/gnome-shell/extensions`, the uuid is
  queued, and its `video-path`/`mute`/`volume` schema keys write
  through `GSETTINGS_SCHEMA_DIR`. GNOME on Wayland cannot load a new
  extension into the running session — `ReloadExtension` is gone and
  `InstallRemoteExtension` 404s because Hanabi is not on
  extensions.gnome.org.
- Post-relogin live check (Shell 50.1): Hanabi loaded enabled and
  played the queued `video-path`. Clearing `video-path` alone does
  NOT stop a running renderer (its `setFilePath('')` just plays an
  empty URI) — so `stop`/`pause`/`set_image` disable the extension,
  which tears it down instantly. `set_video`/resume write
  `video-path` BEFORE re-enabling, otherwise Hanabi opens its
  preferences window on an empty path. Once the shell has loaded the
  extension once, enable/disable work live without relogin. Verified:
  set → plays, stop → extension disabled and desktop cleared.
- `yt_dlp` and `imageio-ffmpeg` (with bundled ffmpeg) ship inside the
  AppImage — downloads are self-contained.
- Debug log persists across CLI invocations (fixed build).
- Rebuilt AppImage carries `mpv`, `feh`, `xwinwrap`, `mpvpaper`,
  `swww`, `swww-daemon`; each runs from the AppImage mount with its
  bundled libs (`ldd` clean for all but blacklisted `libX11`, which
  resolves against the host as intended).

Sample `debug.log` (fixed build, GNOME Wayland):

```
=== Live Wallpaper start ===
[10:21:29.422] APP start
[10:21:29.810] PLATFORM: session=wayland desktop=ubuntu,gnome backend=gnome
[10:22:08.886] CMD: ['set']
[10:22:08.891] APPLY: path=/tmp/wm-test.png ext=.png screen=1920x1200
[10:22:13.380] CMD: ['stop']
```

`/usr/bin/dconf` round-trip on the fixed build:

```
set  → 'file:///tmp/wm-test.png'                                  (blue, visible)
stop → 'file:///usr/share/backgrounds/osselo-Ask_a_friend.jpg'    (custom, restored)
```

## Bugs found and fixed

| # | Bug | Fix |
|---|---|---|
| 0 | **`gsettings` resolved via PATH hit a Linuxbrew shadow binary on the keyfile backend — every apply/restore wrote to a shadow store and the wallpaper never changed** | `session_tool()` resolves session-integrated tools (`gsettings`, `plasma-apply-wallpaperimage`, `xprop`/`xwininfo`/`xrandr`) against `/usr/bin`, `/bin`, `/usr/local/bin` before PATH; backends spawn the resolved absolute path |
| 1 | `debug.log` truncated on **every** CLI call — the UI opens it `w+` before deciding it's a forwarder | log opened in append mode, opened only once per process |
| 2 | `--stop` printed "restored" but restored nothing on Linux | `platform/linux` gained `Backend.restore()` (GNOME gsettings snapshot, X11 `~/.fehbg` snapshot, wlroots `swww query` snapshot); `stop_wallpaper` calls it |
| 3 | Static images re-encoded to BMP and copied to `/tmp` on Linux — volatile path + needless re-encode | Linux applies the source path; every Linux renderer scales natively |
| 4 | `mpvpaper`/`xwinwrap+mpv` muted start used `no-audio` — audio stream absent forever, mute toggle unrecoverable | muted start uses `mute=yes`, audio stream kept, IPC `cycle mute` restores |
| 5 | `--version` printed a static dev string | `app_version()` reads `assets/VERSION` (CI writes the tag); falls back to `dev` |
| 6 | `_quiet_ffmpeg` knew only `.dll` names → no-op on Linux | derived `libavutil.so.N` and `libavutil-N.dll` variants added |
| 7 | Fullscreen auto-pause sensor Windows-only | X11 probe via `xprop`/`xwininfo`/`xrandr` (wm-independent, no deps on X11 sessions); Wayland keeps no-op (no compositor-neutral query) |
| 8 | Renderers had to be installed manually — AppImage carried none | CI builds `xwinwrap`/`mpvpaper`/`swww` from pinned sources and packages `mpv`+`feh` via linuxdeploy; backend resolves bundled dir → `~/.local/share/wallmotion/bin` → `$PATH`, and exports the merged PATH to spawned tools (xwinwrap needs it to find mpv) |
| 9 | A stale `wallmotion-mpv.sock` left by a killed player blocked the next spawn's IPC (`ECONNREFUSED` on connect) | `clear_ipc_socket()` unlinks it inside `_spawn` before the new process starts |
| 10 | `xwinwrap` window rendered **on top** of the desktop (opaque overlay covering icons) — the spawn carried no stacking hint | `-b` (below) added to `xwinwrap_command`, plus `-ni` `-nf` (ignore input / no focus); reported via issue, pending the reporter's hardware confirmation |

## Known limitations (unchanged)

- **GNOME Wayland video**: no public shell API for video wallpapers;
  the app installs the bundled Hanabi zip on demand and drives its
  `video-path`/`mute`/`volume` keys. First activation needs one
  sign-out/in — a GNOME platform limit, not a bug.
- **Wayland fullscreen auto-pause**: no compositor-neutral way to ask
  "is the active window fullscreen". Rule is a no-op on Wayland.
- **mpvpaper** targets all outputs (`*`) — per-output selection needs
  compositor-specific enumeration.
- XWayland note: on Wayland `DISPLAY` is set, but `XDG_SESSION_TYPE`
  takes precedence so `xwinwrap` is never selected there — correct.

## Matrix status

| Session | Image | Video | Tested on hardware? |
|---|---|---|---|
| GNOME Wayland | `gsettings` ✓ | Hanabi install + enable + schema writes ✓ (playback needs one relogin — platform limit) | yes — this pass |
| X11 | `feh` ✓ | `xwinwrap`+`mpv` ✓ | yes — nested `Xephyr :99`, app itself spawned the bundled tools; mpv IPC (play/pause/mute/volume) verified; `--stop` kills the process group |
| KDE Wayland | `plasma-apply-wallpaperimage` | `mpvpaper` | pending (no KDE session) |
| Sway/Hyprland/wlroots | `swww` ✓ | `mpvpaper` ✓ | yes — nested `sway` (WLR_BACKENDS=wayland): `swww img`/`query` round-trips, `mpvpaper '*'` plays all outputs, mute/unmute/pause via app IPC socket; `--stop` kills mpvpaper and restores the image snapshot |

Notes on nested-compositor testing:

- `Xephyr`/`sway` nested windows let the app's real backend selection run
  unchanged: env overrides (`DISPLAY=:99 XDG_SESSION_TYPE=x11`, resp.
  `WAYLAND_DISPLAY=wayland-1 XDG_CURRENT_DESKTOP=sway`) picked the `x11`
  resp. `wlroots` backends, and the spawned tools were the bundled ones.
- Weston can NOT host this test: it advertises no
  `zwlr_layer_shell_v1` global, so `mpvpaper`/`swww-daemon` refuse
  ("Missing a required Wayland interface"). That is a weston limitation,
  not a packaging defect — sway/wlroots works.
- During the pass a stale `wallmotion-mpv.sock` left by a killed
  instance blocked the next spawn's IPC (`ECONNREFUSED`). `_spawn` now
  unlinks a leftover socket first (`clear_ipc_socket()`).

## Reproducing

```bash
sudo apt install feh mpv x11-utils          # only needed when running from source
./WallMotion-x86_64.AppImage &              # or: python main.py
./WallMotion-x86_64.AppImage --set /path/to/image.png
gsettings get org.gnome.desktop.background picture-uri
./WallMotion-x86_64.AppImage --stop         # restores previous wallpaper
cat ~/.local/state/wallmotion/debug.log
```
