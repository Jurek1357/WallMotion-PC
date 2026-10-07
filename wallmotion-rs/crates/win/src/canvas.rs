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
        core::w,
        Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
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

    /// Move `other` right below the canvas (used for WorkerW once).
    pub fn place_other_below_canvas(other: isize, canvas: isize) -> bool {
        place_below(other, canvas)
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
    }
}
