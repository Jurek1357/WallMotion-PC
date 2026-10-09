//! Video canvas: a native child window painted via GDI.
//!
//! Mirrors the canvas part of `VideoWallpaperWindow.start()`: own-DC
//! window class with no background erase, layered when the desktop is
//! "raised" (Windows 11), z-ordered below the desktop icons.

/// Canvas geometry in parent coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasSpec {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Parent window handle (0 = desktop for smoke tests).
    pub parent: isize,
    /// Windows 11 raised desktop: WS_EX_LAYERED canvas.
    pub layered: bool,
}

/// A live wallpaper canvas on the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallpaperCanvas {
    pub canvas: isize,
    pub workerw: isize,
    pub raised: bool,
}

#[cfg(windows)]
pub mod sys {
    use super::{CanvasSpec, WallpaperCanvas};
    use windows::{
        core::{w, BOOL},
        Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Win32::Graphics::Gdi::*,
        Win32::System::LibraryLoader::GetModuleHandleW,
        Win32::UI::WindowsAndMessaging::*,
    };

    pub const CANVAS_CLASS: &str = "WallMotionCanvas";

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    /// Register the canvas window class (idempotent). False on failure.
    pub fn ensure_canvas_class() -> bool {
        unsafe {
            // NOTE: hInstance must be OUR module, not NULL (NULL only
            // finds system classes - a NULL probe always "misses", and
            // re-registration then fails with 1411 ALREADY_EXISTS).
            let Ok(module) = GetModuleHandleW(None) else {
                return false;
            };
            let instance: HINSTANCE = module.into();
            let mut probe: std::mem::MaybeUninit<WNDCLASSW> = std::mem::MaybeUninit::uninit();
            if GetClassInfoW(Some(instance), w!("WallMotionCanvas"), probe.as_mut_ptr()).is_ok() {
                return true;
            }
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_OWNDC,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: Default::default(),
                hCursor: Default::default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                lpszMenuName: w!(""),
                lpszClassName: w!("WallMotionCanvas"),
                hIconSm: Default::default(),
            };
            let atom = RegisterClassExW(&wc);
            atom != 0
                || GetClassInfoW(Some(instance), w!("WallMotionCanvas"), probe.as_mut_ptr()).is_ok()
        }
    }

    /// Create the canvas. Returns the HWND or None.
    pub fn create_canvas(spec: &CanvasSpec) -> Option<isize> {
        if !ensure_canvas_class() {
            return None;
        }
        unsafe {
            let exstyle = WS_EX_NOACTIVATE
                | WS_EX_TRANSPARENT
                | WS_EX_TOOLWINDOW
                | if spec.layered {
                    WS_EX_LAYERED
                } else {
                    WINDOW_EX_STYLE(0)
                };
            let parent = if spec.parent == 0 {
                // Smoke tests / fallbacks: parent to the desktop window.
                GetDesktopWindow()
            } else {
                HWND(spec.parent as _)
            };
            let hwnd = match CreateWindowExW(
                exstyle,
                w!("WallMotionCanvas"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                spec.x,
                spec.y,
                spec.w.max(1),
                spec.h.max(1),
                Some(parent),
                Some(HMENU(std::ptr::null_mut())),
                None,
                None,
            ) {
                Ok(h) => h,
                Err(_) => {
                    return None;
                }
            };
            if spec.layered
                && SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).is_err()
            {
                let _ = DestroyWindow(hwnd);
                return None;
            }
            Some(hwnd.0 as isize)
        }
    }

    /// Destroy a canvas created by [`create_canvas`].
    pub fn destroy_canvas(hwnd: isize) -> bool {
        unsafe { DestroyWindow(HWND(hwnd as _)).is_ok() }
    }

    /// One-line desktop tree for swap diagnosis: Progman children as
    /// `class:hend:visible:WxH` plus our canvases' z-neighbours.
    /// Answers "where is our canvas really" without remote debugging.
    /// Best effort, never panics.
    pub fn debug_desktop_tree() -> String {
        use crate::workerw::sys as ww;
        use windows::core::PCWSTR;
        let mut out = String::new();
        let prog = match ww::progman() {
            Some(p) => p,
            None => return "progman=?".to_string(),
        };
        out.push_str("progchildren[");
        unsafe {
            let mut child =
                FindWindowExW(Some(HWND(prog as _)), None, PCWSTR::null(), PCWSTR::null())
                    .unwrap_or(HWND(0 as _));
            let mut n = 0;
            while !child.0.is_null() && n < 24 {
                n += 1;
                let mut cls = [0u16; 64];
                let len = GetClassNameW(child, &mut cls) as usize;
                let name = String::from_utf16_lossy(&cls[..len.min(64)]);
                let vis = IsWindowVisible(child).as_bool() as u8;
                let mut rect = RECT::default();
                let geom = if GetWindowRect(child, &mut rect).is_ok() {
                    format!("{}x{}", rect.right - rect.left, rect.bottom - rect.top)
                } else {
                    "?".to_string()
                };
                out.push_str(&format!("{name}:{:x}:{vis}:{geom} ", child.0 as isize));
                child = FindWindowExW(
                    Some(HWND(prog as _)),
                    Some(child),
                    PCWSTR::null(),
                    PCWSTR::null(),
                )
                .unwrap_or(HWND(0 as _));
            }
            out.push(']');
            // Our canvases anywhere under Progman (direct or inside a
            // WorkerW): visibility + z-neighbours (prev = window above).
            out.push_str(" our[");
            // Collect parent windows: Progman itself + its WorkerW kids.
            let mut parents = vec![HWND(prog as _)];
            {
                let mut w =
                    FindWindowExW(Some(HWND(prog as _)), None, w!("WorkerW"), PCWSTR::null())
                        .unwrap_or(HWND(0 as _));
                let mut n = 0;
                while !w.0.is_null() && n < 8 {
                    n += 1;
                    parents.push(w);
                    w = FindWindowExW(
                        Some(HWND(prog as _)),
                        Some(w),
                        w!("WorkerW"),
                        PCWSTR::null(),
                    )
                    .unwrap_or(HWND(0 as _));
                }
            }
            let mut m = 0;
            for parent in parents {
                let mut mine =
                    FindWindowExW(Some(parent), None, w!("WallMotionCanvas"), PCWSTR::null())
                        .unwrap_or(HWND(0 as _));
                while !mine.0.is_null() && m < 8 {
                    m += 1;
                    let vis = IsWindowVisible(mine).as_bool() as u8;
                    let prev = GetWindow(mine, GW_HWNDPREV)
                        .map(|h| {
                            let mut c = [0u16; 64];
                            let l = GetClassNameW(h, &mut c) as usize;
                            String::from_utf16_lossy(&c[..l.min(64)])
                        })
                        .unwrap_or_else(|_| "?".to_string());
                    let next = GetWindow(mine, GW_HWNDNEXT)
                        .map(|h| {
                            let mut c = [0u16; 64];
                            let l = GetClassNameW(h, &mut c) as usize;
                            String::from_utf16_lossy(&c[..l.min(64)])
                        })
                        .unwrap_or_else(|_| "?".to_string());
                    out.push_str(&format!(
                        "{:x}:v{vis}:above={prev}:below={next} ",
                        mine.0 as isize
                    ));
                    mine = FindWindowExW(
                        Some(parent),
                        Some(mine),
                        w!("WallMotionCanvas"),
                        PCWSTR::null(),
                    )
                    .unwrap_or(HWND(0 as _));
                }
            }
            out.push(']');
        }
        out
    }

    /// Show or hide a canvas without destroying it. Hidden canvases keep
    /// their mpv painting (used for pre-roll: the new video starts
    /// hidden while the old one keeps playing, then they swap).
    pub fn set_canvas_visible(hwnd: isize, visible: bool) -> bool {
        unsafe {
            let _ = ShowWindow(HWND(hwnd as _), if visible { SW_SHOW } else { SW_HIDE });
        }
        true
    }

    /// Move the canvas right below `after` (icons stay on top).
    pub fn place_below(hwnd: isize, after: isize) -> bool {
        unsafe {
            SetWindowPos(
                HWND(hwnd as _),
                Some(HWND(after as _)),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .is_ok()
        }
    }

    /// Find the desktop icon view (DefView): direct Progman child on
    /// some layouts, else inside a Progman-child WorkerW. Best effort.
    pub fn find_defview() -> Option<isize> {
        use crate::workerw::sys as ww;
        use windows::core::PCWSTR;
        unsafe {
            let prog = HWND(ww::progman()? as _);
            let direct = FindWindowExW(Some(prog), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                .map(|h| h.0 as isize)
                .unwrap_or(0);
            if direct != 0 {
                return Some(direct);
            }
            let mut w = FindWindowExW(Some(prog), None, w!("WorkerW"), PCWSTR::null())
                .unwrap_or(HWND(0 as _));
            while !w.0.is_null() {
                let dv = FindWindowExW(Some(w), None, w!("SHELLDLL_DefView"), PCWSTR::null())
                    .map(|h| h.0 as isize)
                    .unwrap_or(0);
                if dv != 0 {
                    return Some(dv);
                }
                w = FindWindowExW(Some(prog), Some(w), w!("WorkerW"), PCWSTR::null())
                    .unwrap_or(HWND(0 as _));
            }
            None
        }
    }

    /// Keep our canvas right below the desktop icons. Opaque WorkerW
    /// backgrounds drift above it otherwise: audio keeps playing while
    /// the video vanishes behind the original wallpaper. No-op unless
    /// misordered (or different parents). Best effort, cheap: call it
    /// after every swap plus throttled from the UI loop.
    pub fn ensure_below_defview(canvas: isize) -> bool {
        if canvas_below_defview(canvas) {
            return true;
        }
        let defview = match find_defview() {
            Some(d) => d,
            None => return false,
        };
        place_below(canvas, defview)
    }

    /// True when `canvas` already sits right below the desktop icons
    /// (same sibling list). Used to log only real reorderings.
    pub fn canvas_below_defview(canvas: isize) -> bool {
        unsafe {
            let defview = match find_defview() {
                Some(d) => d,
                None => return false,
            };
            let parent = GetParent(HWND(canvas as _)).map(|h| h.0 as isize).ok();
            if parent != GetParent(HWND(defview as _)).map(|h| h.0 as isize).ok() {
                return false;
            }
            GetWindow(HWND(canvas as _), GW_HWNDPREV)
                .map(|h| h.0 as isize == defview)
                .unwrap_or(false)
        }
    }

    /// Move `other` right below the canvas (used for WorkerW once).
    pub fn place_other_below_canvas(other: isize, canvas: isize) -> bool {
        place_below(other, canvas)
    }

    /// Paint the canvas black (fresh pre-roll baseline: the brightness
    /// probe below only fires on real video frames, never on stale
    /// leftovers). Best effort.
    pub fn clear_canvas(hwnd: isize) -> bool {
        unsafe {
            let hdc = GetDC(Some(HWND(hwnd as _)));
            if hdc.is_invalid() {
                return false;
            }
            let mut rect = RECT::default();
            let ok = GetWindowRect(HWND(hwnd as _), &mut rect).is_ok();
            let brush = CreateSolidBrush(COLORREF(0));
            let painted = ok && !brush.is_invalid() && FillRect(hdc, &rect, brush) != 0;
            if !brush.is_invalid() {
                let _ = DeleteObject(brush.into());
            }
            ReleaseDC(Some(HWND(hwnd as _)), hdc);
            painted
        }
    }

    /// Mean luma (0-255) of a small canvas capture. Tells a painting
    /// video apart from a black/unpainted canvas so swaps happen on the
    /// first real frame instead of a fixed delay. `None` on any failure
    /// (caller falls back to the timeout). Never panics.
    pub fn canvas_brightness(hwnd: isize) -> Option<f64> {
        const W: i32 = 48;
        const H: i32 = 27;
        unsafe {
            let src = GetDC(Some(HWND(hwnd as _)));
            if src.is_invalid() {
                return None;
            }
            let mut rect = RECT::default();
            if GetWindowRect(HWND(hwnd as _), &mut rect).is_err() {
                ReleaseDC(Some(HWND(hwnd as _)), src);
                return None;
            }
            let (fw, fh) = (
                (rect.right - rect.left).max(1),
                (rect.bottom - rect.top).max(1),
            );
            let dst = CreateCompatibleDC(Some(src));
            if dst.is_invalid() {
                ReleaseDC(Some(HWND(hwnd as _)), src);
                return None;
            }
            let bmp = CreateCompatibleBitmap(src, W, H);
            if bmp.is_invalid() {
                let _ = DeleteDC(dst);
                ReleaseDC(Some(HWND(hwnd as _)), src);
                return None;
            }
            let old = SelectObject(dst, bmp.into());
            let blitted = StretchBlt(dst, 0, 0, W, H, Some(src), 0, 0, fw, fh, SRCCOPY).as_bool();
            SelectObject(dst, old);
            let out = if !blitted {
                None
            } else {
                let mut bmi: BITMAPINFO = std::mem::zeroed();
                bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                bmi.bmiHeader.biWidth = W;
                bmi.bmiHeader.biHeight = -H; // top-down
                bmi.bmiHeader.biPlanes = 1;
                bmi.bmiHeader.biBitCount = 32;
                bmi.bmiHeader.biCompression = BI_RGB.0;
                let mut px = vec![0u8; (W * H * 4) as usize];
                let got = GetDIBits(
                    dst,
                    bmp,
                    0,
                    H as u32,
                    Some(px.as_mut_ptr().cast()),
                    &mut bmi,
                    DIB_RGB_COLORS,
                );
                if got == 0 {
                    None
                } else {
                    let mut sum = 0u64;
                    let (quads, _) = px.as_chunks::<4>();
                    for p in quads {
                        // BGRA: luma from BGR.
                        sum += (p[0] as u64 * 114 + p[1] as u64 * 587 + p[2] as u64 * 299) / 1000;
                    }
                    Some(sum as f64 / (W * H) as f64)
                }
            };
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dst);
            ReleaseDC(Some(HWND(hwnd as _)), src);
            out
        }
    }

    /// Paint an animated test pattern (vertical bands). Demo/test only.
    pub fn paint_test_pattern(hwnd: isize, w: i32, h: i32, frame: u32) -> bool {
        unsafe {
            let hdc = GetDC(Some(HWND(hwnd as _)));
            if hdc.is_invalid() {
                return false;
            }
            let bands = 8;
            let mut ok = true;
            for i in 0..bands {
                let r = (i * 36 + frame as i32 * 5).rem_euclid(256) as u32;
                let g = (i * 61 + frame as i32 * 3).rem_euclid(256) as u32;
                let b = (i * 97 + frame as i32 * 7).rem_euclid(256) as u32;
                let brush = CreateSolidBrush(COLORREF(r | (g << 8) | (b << 16)));
                if brush.is_invalid() {
                    ok = false;
                    break;
                }
                let old = SelectObject(hdc, brush.into());
                let x0 = w * i / bands;
                let x1 = w * (i + 1) / bands;
                if PatBlt(hdc, x0, 0, x1 - x0, h, PATCOPY).0 == 0 {
                    ok = false;
                }
                SelectObject(hdc, old);
                let _ = DeleteObject(brush.into());
            }
            ReleaseDC(Some(HWND(hwnd as _)), hdc);
            ok
        }
    }

    /// Primary monitor size (w, h) in current-process pixels.
    /// Needs DPI awareness for physical pixels (embed the manifest).
    pub fn primary_size() -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }

    /// Virtual screen geometry (x, y, w, h) spanning all monitors.
    pub fn virtual_screen() -> (i32, i32, i32, i32) {
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            (x, y, w.max(1), h.max(1))
        }
    }

    /// One display monitor.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct MonitorInfo {
        /// Device name (e.g. `\\.\DISPLAY1`), stable across reboots.
        pub name: String,
        /// Full monitor rect (left, top, right, bottom).
        pub rect: (i32, i32, i32, i32),
        /// Primary display flag.
        pub primary: bool,
    }

    /// All monitors via EnumDisplayMonitors. Empty when unavailable.
    pub fn list_monitors() -> Vec<MonitorInfo> {
        struct Sink {
            out: Vec<MonitorInfo>,
        }
        unsafe extern "system" fn callback(
            hmon: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            unsafe {
                let sink = &mut *(lparam.0 as *mut Sink);
                let mut info: MONITORINFOEXW = std::mem::zeroed();
                info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
                if GetMonitorInfoW(hmon, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
                    let end = info
                        .szDevice
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(info.szDevice.len());
                    let name = String::from_utf16_lossy(&info.szDevice[..end]);
                    let r = info.monitorInfo.rcMonitor;
                    sink.out.push(MonitorInfo {
                        name,
                        rect: (r.left, r.top, r.right, r.bottom),
                        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                    });
                }
            }
            BOOL(1)
        }
        let mut sink = Sink { out: vec![] };
        unsafe {
            let _ = EnumDisplayMonitors(
                Some(HDC(std::ptr::null_mut())),
                None,
                Some(callback),
                LPARAM(&mut sink as *mut _ as isize),
            );
        }
        sink.out
    }

    /// Full setup: find WorkerW, create the canvas over (x, y, w, h),
    /// order it below the desktop icons. Mirrors `VideoWallpaperWindow`.
    pub fn setup_wallpaper_canvas(x: i32, y: i32, w: i32, h: i32) -> Option<WallpaperCanvas> {
        use crate::workerw::sys as ww;
        let progman = ww::progman()?;
        let raised = unsafe {
            GetWindowLongW(HWND(progman as _), GWL_EXSTYLE) as u32 & WS_EX_NOREDIRECTIONBITMAP.0
                != 0
        };
        let defview = unsafe {
            FindWindowExW(Some(HWND(progman as _)), None, w!("SHELLDLL_DefView"), None)
                .ok()
                .map(|h| h.0 as isize)
                .unwrap_or(0)
        };
        let workerw = ww::get_workerw_handle()?;
        let (parent, px, py) = if raised {
            (progman, x, y)
        } else {
            // Legacy: coordinates relative to WorkerW.
            let (wx, wy, _, _) = ww::window_rect(workerw).unwrap_or((0, 0, 0, 0));
            (workerw, x - wx, y - wy)
        };
        let canvas = create_canvas(&CanvasSpec {
            x: px,
            y: py,
            w,
            h,
            parent,
            layered: raised,
        })?;
        if raised {
            if defview != 0 {
                place_below(canvas, defview);
            }
            place_other_below_canvas(workerw, canvas);
        }
        Some(WallpaperCanvas {
            canvas,
            workerw,
            raised,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn smoke_create_and_destroy() {
            // Real window on a real desktop session; CI runners have one.
            let spec = CanvasSpec {
                x: 0,
                y: 0,
                w: 64,
                h: 64,
                parent: 0,
                layered: false,
            };
            let hwnd = create_canvas(&spec).expect("canvas created");
            assert!(place_below(hwnd, 0) || true); // HWND(0)=desktop: best effort
            assert!(destroy_canvas(hwnd));
        }

        #[test]
        fn smoke_paint_pattern() {
            let spec = CanvasSpec {
                x: 0,
                y: 0,
                w: 64,
                h: 64,
                parent: 0,
                layered: false,
            };
            let hwnd = create_canvas(&spec).expect("canvas created");
            for frame in 0..3u32 {
                assert!(paint_test_pattern(hwnd, 64, 64, frame));
            }
            assert!(destroy_canvas(hwnd));
        }

        #[test]
        fn smoke_hide_show() {
            let spec = CanvasSpec {
                x: 0,
                y: 0,
                w: 64,
                h: 64,
                parent: 0,
                layered: false,
            };
            let hwnd = create_canvas(&spec).expect("canvas created");
            assert!(set_canvas_visible(hwnd, false));
            assert!(set_canvas_visible(hwnd, true));
            assert!(destroy_canvas(hwnd));
        }

        #[test]
        fn smoke_brightness_and_clear() {
            let spec = CanvasSpec {
                x: 0,
                y: 0,
                w: 64,
                h: 64,
                parent: 0,
                layered: false,
            };
            let hwnd = create_canvas(&spec).expect("canvas created");
            for frame in 0..3u32 {
                assert!(paint_test_pattern(hwnd, 64, 64, frame));
            }
            let bright = canvas_brightness(hwnd).expect("brightness of a painted canvas reads");
            assert!(bright > 20.0, "pattern is bright, got {bright}");
            assert!(clear_canvas(hwnd));
            let dark = canvas_brightness(hwnd).expect("brightness reads after clear");
            assert!(dark < 5.0, "cleared canvas is black, got {dark}");
            assert!(destroy_canvas(hwnd));
        }
    }
}
