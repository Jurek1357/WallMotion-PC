//! WorkerW discovery: find the desktop window to embed video into.
//!
//! Mirrors `get_workerw_handle()` in `wallmotion/win32.py`:
//! reuse an existing Progman-child WorkerW, otherwise ask Explorer
//! to spawn one (message 0x052C), otherwise scan top-level windows.

/// One WorkerW candidate for scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    /// Window handle as integer (0 = none).
    pub hwnd: isize,
    /// Window rect (left, top, right, bottom).
    pub rect: (i32, i32, i32, i32),
    /// IsWindowVisible result.
    pub visible: bool,
    /// Whether it hosts SHELLDLL_DefView (the icons - skip those).
    pub has_defview: bool,
}

/// Pick the best WorkerW: visible, largest area, no icon host.
/// Mirrors `best_workerw_from_list()`. Pure, unit-tested.
pub fn best_workerw(cands: &[Candidate]) -> Option<isize> {
    let mut best: Option<isize> = None;
    let mut best_score: i64 = 0;
    for c in cands {
        if c.hwnd == 0 || c.has_defview {
            continue;
        }
        let (l, t, r, b) = c.rect;
        let area = (r.max(l) - l).max(0) as i64 * (b.max(t) - t).max(0) as i64;
        let mut score = area;
        if c.visible {
            score += 100_000_000;
        }
        if score > best_score && area > 100 {
            best_score = score;
            best = Some(c.hwnd);
        }
    }
    best
}

/// Compute a canvas rect (x, y, w, h) relative to the parent window.
/// Mirrors `place_canvas()`. Pure, unit-tested.
pub fn place_canvas(
    parent: (i32, i32, i32, i32),
    monitor: Option<(i32, i32, i32, i32)>,
) -> (i32, i32, i32, i32) {
    let (px1, py1, px2, py2) = parent;
    let (mx, my, mw, mh) = match monitor {
        Some((x, y, w, h)) => (x, y, w.max(1), h.max(1)),
        None => return (0, 0, (px2 - px1).max(1), (py2 - py1).max(1)),
    };
    (mx - px1, my - py1, mw, mh)
}

#[cfg(windows)]
pub mod sys {
    use windows::{core::w, Win32::Foundation::HWND, Win32::UI::WindowsAndMessaging::*};

    /// Find Progman, the top-level desktop shell window.
    pub fn progman() -> Option<isize> {
        unsafe {
            let hwnd = FindWindowW(w!("Progman"), None).unwrap_or(HWND(0 as _));
            if hwnd.0.is_null() {
                None
            } else {
                Some(hwnd.0 as isize)
            }
        }
    }

    /// All direct Progman-child WorkerW handles.
    pub fn progman_child_workerws(progman: isize) -> Vec<isize> {
        let mut found = vec![];
        unsafe {
            let mut hwnd = FindWindowExW(Some(HWND(progman as _)), None, w!("WorkerW"), None)
                .unwrap_or(HWND(0 as _));
            while !hwnd.0.is_null() {
                found.push(hwnd.0 as isize);
                hwnd = FindWindowExW(Some(HWND(progman as _)), Some(hwnd), w!("WorkerW"), None)
                    .unwrap_or(HWND(0 as _));
            }
        }
        found
    }

    /// Ask Explorer to spawn the WorkerW (message 0x052C), then wait.
    pub fn spawn_workerw(progman: isize) {
        unsafe {
            let mut _result = 0usize;
            let _ = SendMessageTimeoutW(
                HWND(progman as _),
                0x052C,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
                SMTO_NORMAL,
                1000,
                Some(&mut _result),
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    /// Window rect (left, top, right, bottom), if readable.
    pub fn window_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
        unsafe {
            let mut rect = windows::Win32::Foundation::RECT::default();
            GetWindowRect(HWND(hwnd as _), &mut rect).ok()?;
            Some((rect.left, rect.top, rect.right, rect.bottom))
        }
    }

    /// Whether the window hosts the desktop icons.
    pub fn has_defview(hwnd: isize) -> bool {
        unsafe {
            FindWindowExW(Some(HWND(hwnd as _)), None, w!("SHELLDLL_DefView"), None)
                .map(|h| !h.0.is_null())
                .unwrap_or(false)
        }
    }

    /// Whether the window is visible.
    pub fn is_visible(hwnd: isize) -> bool {
        unsafe { IsWindowVisible(HWND(hwnd as _)).as_bool() }
    }

    /// Full discovery: existing child, spawn, re-check.
    /// Returns the WorkerW handle or None (mirrors the Python steps 1-3;
    /// the top-level legacy scan stays in Python for now).
    pub fn get_workerw_handle() -> Option<isize> {
        let prog = progman()?;
        let score = |list: &[isize]| {
            let cands: Vec<super::Candidate> = list
                .iter()
                .filter_map(|&h| {
                    window_rect(h).map(|rect| super::Candidate {
                        hwnd: h,
                        rect,
                        visible: is_visible(h),
                        has_defview: has_defview(h),
                    })
                })
                .collect();
            super::best_workerw(&cands)
        };
        if let Some(h) = score(&progman_child_workerws(prog)) {
            return Some(h);
        }
        spawn_workerw(prog);
        score(&progman_child_workerws(prog))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(hwnd: isize, rect: (i32, i32, i32, i32), visible: bool, defview: bool) -> Candidate {
        Candidate {
            hwnd,
            rect,
            visible,
            has_defview: defview,
        }
    }

    #[test]
    fn skips_icon_host() {
        let cands = vec![
            cand(1, (0, 0, 3840, 1080), true, true),
            cand(2, (0, 0, 1920, 1080), true, false),
        ];
        assert_eq!(best_workerw(&cands), Some(2));
    }

    #[test]
    fn prefers_visible_and_large() {
        let cands = vec![
            cand(1, (0, 0, 100, 100), true, false),
            cand(2, (0, 0, 1920, 1080), false, false),
        ];
        // Invisible but far larger still loses to the visible bonus math:
        // visible 100x100 scores 10000+100M; hidden 1080p scores ~2M.
        assert_eq!(best_workerw(&cands), Some(1));
    }

    #[test]
    fn ignores_zero_and_tiny() {
        let cands = vec![
            cand(0, (0, 0, 9999, 9999), true, false),
            cand(3, (0, 0, 5, 5), true, false),
        ];
        assert_eq!(best_workerw(&cands), None);
    }

    #[test]
    fn place_full_parent() {
        assert_eq!(place_canvas((0, 0, 3840, 1080), None), (0, 0, 3840, 1080));
    }

    #[test]
    fn place_second_monitor() {
        assert_eq!(
            place_canvas((0, 0, 3840, 1080), Some((1920, 0, 1920, 1080))),
            (1920, 0, 1920, 1080)
        );
    }

    #[test]
    fn place_negative_offset() {
        assert_eq!(
            place_canvas((-1920, 0, 1920, 1080), Some((-1920, 0, 1920, 1080))),
            (0, 0, 1920, 1080)
        );
    }
}
