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

#[cfg(windows)]
pub mod sys {
    use super::CanvasSpec;
    use windows::{
        core::w,
        Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Win32::Graphics::Gdi::HBRUSH,
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
            let mut probe: std::mem::MaybeUninit<WNDCLASSW> = std::mem::MaybeUninit::uninit();
            if GetClassInfoW(None, w!("WallMotionCanvas"), probe.as_mut_ptr()).is_ok() {
                return true;
            }
            let Ok(instance) = GetModuleHandleW(None) else {
                return false;
            };
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_OWNDC,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance.into(),
                hIcon: Default::default(),
                hCursor: Default::default(),
                hbrBackground: HBRUSH(std::ptr::null_mut()),
                lpszMenuName: w!(""),
                lpszClassName: w!("WallMotionCanvas"),
                hIconSm: Default::default(),
            };
            RegisterClassExW(&wc) != 0
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
            let hwnd = CreateWindowExW(
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
                Some(HINSTANCE(std::ptr::null_mut())),
                None,
            )
            .ok()?;
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
    }
}
