# Per-monitor wallpapers

Status: phase 1 (UI, config, Windows/X11 video) and phase 2 (Windows
static images) are implemented in the native app; Wayland wlroots
per-output support is still open. This document describes the behavior,
data model, backend work and remaining phases.

## Goal

Before this feature the UI had one monitor selector (`All monitors` or
one display) and one `file`. The selection was global: it decided which
monitor(s) received the next wallpaper, but it was not a mapping of
monitor -> wallpaper. Video already ran one canvas/player per selected
monitor, but every canvas played the same file.

The goal is a first-class per-monitor mode:

- every monitor can have its own image or video,
- different monitors can show different files at the same time,
- the assignment survives restarts,
- unsupported desktops degrade explicitly instead of silently applying
  the wrong content.

Non-goals for the first delivery:

- per-monitor rotation queues,
- mixed image/video on desktops whose API cannot express it,
- hotplug handling beyond a manual refresh,
- GNOME/KDE per-output images where the desktop API is global.

## Current state

Key integration points:

- Monitor enumeration: `crates/win/src/canvas.rs` (Windows),
  `crates/win/src/x11.rs` (X11 RandR). No Wayland enumeration.
- Single global target: `App.monitor` and `App.file` in
  `crates/settings/src/main.rs`.
- Geometry choke points: `selected_rect()`, `legacy_targets()` and
  `per_monitor_targets()` in `crates/settings/src/main.rs`.
- Legacy Windows static images still use `SystemParametersInfoW`
  globally; per-monitor mode uses `IDesktopWallpaper` through
  `set_static_wallpapers()` in `crates/win/src/wallpaper.rs`.
- Windows/X11 video already creates one canvas and player per target
  rect (`set_video`, `begin_preroll`, `commit_preroll`).
- Wayland backends are all-output: `swww img` without `--outputs`,
  `mpvpaper ... "*"`, GNOME `gsettings`, KDE
  `plasma-apply-wallpaperimage`.

## UX proposal

Add a `Per-monitor` toggle to the Source card, above the monitor combo.

When disabled (default), behavior stays exactly as today: one file and
one monitor/all-monitors target.

When enabled:

- the single file row is replaced by one row per discovered monitor,
- each row shows the monitor label, the assigned file name and two
  actions: `Assign current file` and `Clear`,
- the existing file picker/library still chooses the "current file",
- a monitor without an assignment is simply left unchanged,
- `Apply` walks the assignments and applies each monitor independently,
- `Stop` tears video down; Windows static images stay in place (same as
  the legacy path).

This keeps the UI small: no second file browser per monitor, and the
library remains the single source of files. A future version can add a
per-monitor file picker directly in the row.

The monitor combo stays for the legacy single-target mode. In
per-monitor mode it is hidden.

## Data model

Keep the legacy fields for backward compatibility:

```rust
pub struct AppConfig {
    // existing: global default and single target
    pub monitor: String,
    pub last_path: String,
    // new: optional per-monitor assignments
    #[serde(default)]
    pub per_monitor: bool,
    #[serde(default)]
    pub monitor_files: BTreeMap<String, String>,
}
```

`monitor_files` maps a monitor key to a media path. The key is the same
name the current code already stores:

- Windows: `\\.\DISPLAY1`,
- X11: RandR monitor name,
- wlroots: output name (e.g. `eDP-1`, `DP-1`).

Names are not perfectly stable when cables/monitors are rearranged. The
first version accepts that; a later version can add an EDID/geometry
based key. If a key is missing at apply time, the row is skipped and the
debug log records it.

Pure resolution logic lives in `wallmotion-core` so it is testable
without a display:

```rust
pub struct MonitorDescriptor {
    pub name: String,
    pub rect: (i32, i32, i32, i32),
}

pub struct MonitorTarget {
    pub name: String,
    pub rect: (i32, i32, i32, i32),
    pub path: String,
}

pub fn resolve_targets(
    monitors: &[MonitorDescriptor],
    assignments: &BTreeMap<String, String>,
) -> Vec<MonitorTarget>;
```

Rules:

1. return only connected monitors with a non-empty assignment,
2. ignore assignments for disconnected monitors,
3. keep the monitor-list order, not map order,
4. legacy mode keeps its own path (one selected monitor or all).

## Backend matrix

| Backend | Per-monitor image | Per-monitor video | Notes |
|---|---|---|---|
| Windows | `IDesktopWallpaper::SetWallpaper` | one canvas/player per monitor | implemented; monitor rects bridge `\\.\DISPLAYx` to COM device paths |
| X11 | canvas + mpv | one canvas/player per monitor | already one rect per monitor; pass a path per target |
| wlroots | `swww img --outputs <name>` | `mpvpaper <name> <file>` | enumerate outputs via `swww query`/`wlr-randr` |
| GNOME | global `gsettings` | global Hanabi | per-monitor mode disabled with an explanation |
| KDE | global `plasma-apply-wallpaperimage` | unsupported | per-monitor mode disabled with an explanation |

### Windows static images

`SPI_SETDESKWALLPAPER` cannot target a monitor, so per-monitor mode uses
`IDesktopWallpaper`:

1. `Win32_UI_Shell` and `Win32_System_Com` are enabled in the crate,
2. `set_static_wallpapers()` creates the COM object and enumerates
   monitor device paths,
3. the existing `\\.\DISPLAYx` names are matched to COM device paths by
   monitor rect,
4. each image is cover-fitted to its monitor and written with
   `SetWallpaper(monitor_id, path)`.

There is no per-monitor `GetWallpaper` snapshot yet: `Stop` leaves the
static images in place, exactly like the legacy single-image path. If
COM setup or monitor matching fails, the app reports `img_failed` and
keeps the previous playback instead of applying one image everywhere.

### Windows/X11 video

The canvas code builds one runner per target. The implementation
carries a path per target:

- `video_plan()` returns `VideoTarget { name, rect, path }`,
- `set_video_targets()` and `begin_preroll()` take a plan,
- `RunningVideo` records the monitor key in addition to the rect,
- only the first video runner is audible, the rest stay muted (same
  audio rule as today).

Preroll reuse is valid when the monitor keys and rects are unchanged;
the files may differ because the successor warms up on hidden canvases
while the predecessor keeps playing.

### wlroots

- Image: add `--outputs <name>` to `swww img`; the app currently omits
  it, which means all outputs.
- Video: pass the output name instead of `*` to `mpvpaper`.
- Enumerate outputs from `swww query` (already parsed for image paths) or
  `wlr-randr` as a fallback.
- `stop`/restore snapshots must be keyed by output.

### GNOME/KDE

Both APIs are global. The UI shows a warning in per-monitor mode and
`Apply` refuses with an explanation: use X11 or one wallpaper for all
monitors. It never applies the first assignment to every output.

## Apply and stop semantics

- Per-monitor apply touches only assigned, connected monitors.
- Unassigned monitors keep whatever they show.
- Mixed image/video is supported on Windows and X11:
  - Windows sets image targets through `IDesktopWallpaper` first, then
    starts video canvases on the remaining monitors.
  - X11 plays both through the canvas/mpv path.
  - wlroots: swww image + mpvpaper video on different outputs still
    needs per-output state.
- `Stop` behavior:
  - Windows images stay in place (no per-monitor snapshot yet),
  - Windows/X11 video: teardown reveals the desktop or image below,
  - X11: close canvases; no snapshot needed,
  - wlroots: per-output swww snapshot is still open.
- Rotation is disabled while per-monitor mode is on until per-monitor
  cursors exist. Show a one-line explanation in the rotation card.

## Phases

### Phase 1: foundation and video (done)

- [x] config fields + serde defaults + migration tests,
- [x] pure `resolve_targets()` in `wallmotion-core` with tests,
- [x] per-monitor toggle and monitor rows in the UI,
- [x] Windows/X11 per-monitor video,
- [x] per-monitor status text and debug logging.

### Phase 2: Windows static images (done)

- [x] `IDesktopWallpaper` wrapper in `wallmotion-win`,
- [x] monitor-name to device-path matching by rect,
- [x] mixed image/video handling on Windows,
- [ ] per-monitor snapshot/restore.

### Phase 3: wlroots outputs

- [ ] output enumeration,
- [ ] `swww --outputs` and `mpvpaper <output>`,
- [ ] per-output snapshot/restore,
- [ ] manual backend test matrix update.

### Phase 4: polish

- per-monitor rotation cursors,
- hotplug/display-change handling,
- optional EDID-based monitor keys,
- per-monitor file picker in the UI.

## Acceptance criteria (phase 1 + Windows images)

- Legacy mode keeps the current config and behavior.
- Two connected monitors can show two different videos on Windows and
  X11, each on the correct rect.
- Windows can show different static images per monitor.
- Mixed image/video assignments work on Windows and X11.
- The first runner is audible; the others are muted and follow
  pause/mute/volume.
- `Stop` tears video down and leaves Windows static images in place.
- Per-monitor assignments survive a restart.
- Config tests cover old JSON without the new fields.
- `resolve_targets()` tests cover disconnected monitors, empty
  assignments, ordering and degenerate rects.

## Risks

- Monitor identity changes after rearranging displays.
- COM monitor device paths and `\\.\DISPLAYx` names differ; matching is
  rect-based and can fail with overlapping/cloned displays.
- Static image and video have separate lifecycles; stopping one must not
  kill the other.
- Windows `Stop` does not restore prior per-monitor images yet.
- Preroll reuse assumes the same monitor keys and rects.
- Wayland output enumeration differs between compositors.
- GNOME/KDE cannot do per-output images with the chosen tools.

## References

- UI and apply orchestration: `crates/settings/src/main.rs`
- Config: `crates/settings/src/config.rs`
- Windows/X11 canvases: `crates/win/src/canvas.rs`
- X11 monitors: `crates/win/src/x11.rs`
- Windows static wallpaper: `crates/win/src/wallpaper.rs`
- Wayland backends: `crates/settings/src/linux_desktop.rs`
- Rotation: `crates/core/src/rotation.rs`
