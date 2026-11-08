# Linux hardware test checklist

This checklist is for the native Rust app (`wallmotion-settings`) v1.4.0
and later. It covers a real desktop session - not a nested compositor or
a headless smoke test.

Use one issue per desktop/session so results stay comparable. The issue
form is at **New issue -> Linux hardware test report**.

## What to test

| Session | Image backend | Video backend |
|---|---|---|
| X11 | in-process canvas | in-process libmpv |
| GNOME Wayland | `gsettings` | Hanabi extension |
| KDE Plasma Wayland | `plasma-apply-wallpaperimage` | not supported |
| wlroots Wayland (sway, Hyprland, ...) | `swww` | `mpvpaper` |

The app detects the session from `XDG_SESSION_TYPE`,
`XDG_CURRENT_DESKTOP`, `WAYLAND_DISPLAY`, `DISPLAY` and compositor
environment variables. The UI banner shows the detected backend.

## Before you start

1. Save or note your current wallpaper. Stop restores it only on GNOME,
   wlroots and X11; KDE Wayland has no snapshot to restore.
2. Install the release build:

   ```bash
   sudo apt install ./wallmotion_1.4.0_amd64.deb
   wallmotion-settings --version
   ```

   Or use the AppImage:

   ```bash
   chmod +x WallMotion-x86_64.AppImage
   ./WallMotion-x86_64.AppImage --version
   ```

   Replace `1.4.0` with the version under test. For the AppImage, use
   `./WallMotion-x86_64.AppImage` wherever the commands below say
   `wallmotion-settings`.

3. Check that the tools for your desktop are present:

   ```bash
   command -v gsettings gnome-extensions
   command -v plasma-apply-wallpaperimage
   command -v swww swww-daemon mpvpaper
   ```

4. Capture the environment (paste this into the issue):

   ```bash
   {
     echo "WallMotion: $(wallmotion-settings --version)"
     . /etc/os-release && echo "OS: $PRETTY_NAME"
     echo "Kernel: $(uname -r)"
     echo "XDG_SESSION_TYPE=$XDG_SESSION_TYPE"
     echo "XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP"
     echo "XDG_SESSION_DESKTOP=$XDG_SESSION_DESKTOP"
     echo "WAYLAND_DISPLAY=$WAYLAND_DISPLAY"
     echo "DISPLAY=$DISPLAY"
     gnome-shell --version 2>/dev/null
     plasmashell --version 2>/dev/null
     sway --version 2>/dev/null
     hyprctl version 2>/dev/null
     swww --version 2>/dev/null
     mpvpaper --version 2>/dev/null
   } | tee /tmp/wallmotion-env.txt
   ```

5. Prepare test media:

   - a local PNG or JPG image,
   - a short H.264 MP4 (10-15 seconds is enough).

   If you have ffmpeg, you can generate a video:

   ```bash
   ffmpeg -f lavfi -i testsrc2=size=1280x720:rate=30 -t 10 \
     -c:v libx264 -pix_fmt yuv420p /tmp/wallmotion-test.mp4
   ```

## Core checks

- [ ] `wallmotion-settings --version` prints the release version.
- [ ] The app window opens and the Linux banner names the correct
      desktop/backend.
- [ ] Static image applies from the UI and stays visible after a few
      seconds.
- [ ] Static image applies from the CLI:
      `wallmotion-settings --set /path/to/image.png`
- [ ] Video applies from the UI and plays smoothly.
- [ ] Video applies from the CLI:
      `wallmotion-settings --set /path/to/video.mp4`
- [ ] Pause/resume works from the UI button (global hotkeys are
      Windows-only, so test the Pause button on Linux).
- [ ] Mute and unmute work:
      `wallmotion-settings --mute`, then `--unmute`.
- [ ] Volume changes work while unmuted:
      `wallmotion-settings --volume 55`.
- [ ] Switching video -> image stops the renderer and shows the image.
- [ ] Switching image -> video stops the image backend and starts the
      video renderer.
- [ ] Stop behaves as described for your backend (see below).
- [ ] A second launch or CLI command reaches the running instance
      instead of starting a parallel app.
- [ ] `~/.local/state/wallmotion/debug.log` exists and contains the
      operations above.
- [ ] The tray icon and settings window work (on GNOME this needs an
      AppIndicator/KStatusNotifier extension; its absence is not a bug).

## Backend-specific checks

### GNOME Wayland

Files use `gsettings`; video uses the Hanabi extension. The native app
does not install Hanabi for you.

- [ ] `gsettings get org.gnome.desktop.background picture-uri` changes
      after applying an image.
- [ ] `picture-uri-dark` is also written when the key exists (switch
      between light and dark style to verify both).
- [ ] `--stop` restores the wallpaper that was active before the app
      changed it.
- [ ] Hanabi is installed and enabled before testing video:

      ```bash
      gnome-extensions install --force \
        /path/to/hanabi-extension@jeffshee.github.io.shell-extension.zip
      gnome-extensions enable hanabi-extension@jeffshee.github.io
      gnome-extensions info hanabi-extension@jeffshee.github.io
      ```

      The zip is in `assets/` in the repository (use the file from the
      matching release tag if possible). The first activation normally
      needs one sign-out/sign-in; that is a GNOME limitation, not an app
      bug.
- [ ] Video plays after Hanabi is enabled.
- [ ] Pause disables Hanabi and resume re-enables it with the same path.
- [ ] Mute and volume update the Hanabi schema keys.
- [ ] Applying an image stops a running Hanabi renderer.
- [ ] `--stop` also disables Hanabi and restores the previous wallpaper.

Expected log lines include `gnome image applied:`,
`hanabi video started:` and `restore(gnome): previous wallpaper restored`.

### KDE Plasma Wayland

Images only. Video is expected to be rejected with a clear banner/error.

- [ ] `command -v plasma-apply-wallpaperimage` succeeds.
- [ ] Applying an image changes the desktop wallpaper.
- [ ] Applying a video shows the "not supported on KDE Wayland" message
      and does not start a renderer.
- [ ] `--stop` leaves the last applied image in place (no snapshot on
      KDE). This is expected, not a restoration bug.

Expected log line: `kde image applied: <path>`.

### wlroots Wayland (sway, Hyprland, Wayfire, ...)

Images use `swww`; video uses `mpvpaper` with mpv JSON IPC. The app will
start `swww-daemon` if it is not running.

- [ ] `swww query` works, or the app starts `swww-daemon` successfully.
- [ ] Applying an image from the UI/CLI changes the wallpaper.
- [ ] `swww query` shows the image path after applying it.
- [ ] `pgrep -af mpvpaper` is empty before starting a video.
- [ ] Applying a video starts `mpvpaper` and video plays on all outputs.
- [ ] Pause/resume changes the mpv `pause` property over the IPC socket
      at `$XDG_RUNTIME_DIR/wallmotion-mpv.sock`.
- [ ] Mute/unmute changes the mpv `mute` property.
- [ ] Volume changes the mpv `volume` property (test while unmuted).
- [ ] Applying an image stops `mpvpaper`.
- [ ] `--stop` stops `mpvpaper` and, when a previous `swww` image was
      captured, restores it.
- [ ] With multiple monitors, image and video cover all outputs. Per-output
      selection is not implemented yet.

Expected log lines include `swww image applied:`,
`mpvpaper started pid=... path=...` and
`restore(wlroots): previous wallpaper restored`.

### X11

The native app draws its own canvas and plays video in-process, so no
external renderer is involved.

- [ ] Applying an image covers the desktop and the icons stay usable.
- [ ] Video plays and can be paused, muted and volume-adjusted from the
      UI.
- [ ] `--stop` closes the canvas and reveals the desktop wallpaper that
      was underneath it. There is no separate restore step on X11.
- [ ] Fullscreen auto-pause works if the rule is enabled (X11 only; it is
      a no-op on Wayland).
- [ ] With multiple monitors, the wallpaper covers the virtual screen.

## Known limitations (do not report as bugs)

- GNOME Wayland video needs Hanabi and normally one sign-out/sign-in for
  the first activation.
- KDE Plasma Wayland supports static images only.
- Wayland has no compositor-neutral fullscreen sensor, so fullscreen
  auto-pause is a no-op there.
- `mpvpaper` targets all outputs (`*`); per-output video selection is not
  implemented.
- The native Linux build does not bundle renderers or install Hanabi.
  Renderers are resolved from `~/.local/share/wallmotion/bin` first, then
  `$PATH`. The self-contained `.deb` and AppImage bundle their own GTK,
  libmpv, ffmpeg and libxdo libraries.
- The tray icon needs an AppIndicator/KStatusNotifier host on GNOME.

## Reporting

Open a **Linux hardware test report** issue and include:

- the environment block from step 4,
- the checklist results (passed/failed),
- exact steps for any failure and whether it also happens after a restart,
- `~/.local/state/wallmotion/debug.log` (attach the full file, or paste
  the last ~200 lines).

Please state if the session was a VM, a nested compositor, or a remote
desktop. Those results are useful, but they do not replace a real
hardware session.
