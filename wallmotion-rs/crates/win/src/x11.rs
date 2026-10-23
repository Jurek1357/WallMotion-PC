//! Minimal X11 plumbing shared by the canvas and the autopause sensor.
//!
//! One `RustConnection` is kept alive for the whole process: X11 destroys
//! every window of a client when its connection closes, so a per-call
//! connection would kill the wallpaper canvases immediately.

use std::sync::{Mutex, OnceLock};

use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Screen, Window};
use x11rb::rust_connection::RustConnection;

pub(crate) struct X11 {
    pub conn: RustConnection,
    pub screen_num: usize,
}

/// Run `f` with the shared connection. `None` when X11 is unavailable
/// (headless session, Wayland-only, connect error).
pub(crate) fn with_conn<T>(f: impl FnOnce(&X11, &Screen) -> Option<T>) -> Option<T> {
    static STATE: OnceLock<Option<Mutex<X11>>> = OnceLock::new();
    let state = STATE
        .get_or_init(|| {
            x11rb::connect(None)
                .ok()
                .map(|(conn, screen_num)| Mutex::new(X11 { conn, screen_num }))
        })
        .as_ref()?;
    let guard = state.lock().ok()?;
    let x11 = &*guard;
    let screen = x11.conn.setup().roots.get(x11.screen_num)?;
    f(x11, screen)
}

pub(crate) fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
    conn.intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

fn root_size(conn: &RustConnection, screen: &Screen) -> (i32, i32) {
    conn.get_geometry(screen.root)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|g| (g.width as i32, g.height as i32))
        .unwrap_or((
            screen.width_in_pixels as i32,
            screen.height_in_pixels as i32,
        ))
}

/// One monitor as reported by RandR (single-screen fallback when the
/// extension is missing or reports nothing).
pub(crate) struct Monitor {
    pub name: String,
    pub rect: (i32, i32, i32, i32),
    pub primary: bool,
}

pub(crate) fn monitors(conn: &RustConnection, screen: &Screen) -> Vec<Monitor> {
    let reply = conn
        .randr_get_monitors(screen.root, true)
        .ok()
        .and_then(|c| c.reply().ok());
    if let Some(reply) = reply {
        let mut out = Vec::new();
        for m in reply.monitors {
            if m.width == 0 || m.height == 0 {
                continue;
            }
            let name = conn
                .get_atom_name(m.name)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.name).into_owned())
                .unwrap_or_default();
            out.push(Monitor {
                name: if name.is_empty() {
                    format!("Monitor{}", out.len() + 1)
                } else {
                    name
                },
                rect: (
                    m.x as i32,
                    m.y as i32,
                    m.x as i32 + m.width as i32,
                    m.y as i32 + m.height as i32,
                ),
                primary: m.primary,
            });
        }
        if !out.is_empty() {
            return out;
        }
    }
    let (w, h) = root_size(conn, screen);
    vec![Monitor {
        name: "Screen 0".to_string(),
        rect: (0, 0, w.max(1), h.max(1)),
        primary: true,
    }]
}

pub(crate) fn virtual_screen(conn: &RustConnection, screen: &Screen) -> (i32, i32, i32, i32) {
    let list = monitors(conn, screen);
    let left = list.iter().map(|m| m.rect.0).min().unwrap_or(0);
    let top = list.iter().map(|m| m.rect.1).min().unwrap_or(0);
    let right = list.iter().map(|m| m.rect.2).max().unwrap_or(1);
    let bottom = list.iter().map(|m| m.rect.3).max().unwrap_or(1);
    (left, top, (right - left).max(1), (bottom - top).max(1))
}

/// True when the active window exactly covers its monitor (same
/// contract as the Windows sensor).
pub(crate) fn fullscreen_active(conn: &RustConnection, screen: &Screen) -> bool {
    let root = screen.root;
    let Some(active_atom) = atom(conn, "_NET_ACTIVE_WINDOW") else {
        return false;
    };
    let win: Window = match conn
        .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().and_then(|mut v| v.next()))
    {
        Some(w) => w,
        None => return false,
    };
    if win == 0 || win == root {
        return false;
    }
    // Own Settings window is never a "fullscreen app" (same rule as the
    // Windows sensor).
    if let Some(pid_atom) = atom(conn, "_NET_WM_PID") {
        if let Some(pid) = conn
            .get_property(false, win, pid_atom, AtomEnum::CARDINAL, 0, 1)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().and_then(|mut v| v.next()))
        {
            if pid == std::process::id() {
                return false;
            }
        }
    }
    let geom = match conn.get_geometry(win).ok().and_then(|c| c.reply().ok()) {
        Some(g) => g,
        None => return false,
    };
    let (x, y) = match conn
        .translate_coordinates(win, root, 0, 0)
        .ok()
        .and_then(|c| c.reply().ok())
    {
        Some(t) => (t.dst_x as i32, t.dst_y as i32),
        None => return false,
    };
    let rect = (x, y, x + geom.width as i32, y + geom.height as i32);
    monitors(conn, screen)
        .iter()
        .any(|m| wallmotion_core::autopause::is_fullscreen_rect(rect, m.rect))
}
