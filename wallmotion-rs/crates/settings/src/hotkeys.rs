//! Global media hotkeys.
//!
//! Windows-only: a dedicated thread owns the hotkey registrations and
//! pumps its own message queue, so the shortcuts work even while the
//! settings window is hidden in the tray. The UI thread drains the
//! channel from [`Hotkeys::drain`] once per frame.
//!
//! Combinations are fixed for now (Ctrl+Alt + arrows/Space/M) - they
//! are rare enough not to collide with common apps. `registered()`
//! reports how many took; another app may already own one.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// One physical shortcut press, reduced to what the app should do.
///
/// On non-Windows targets nothing ever constructs these (the hotkey
/// worker is a stub), but the UI still matches on them.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Next,
    Prev,
    Pause,
    Mute,
}

/// Receiver side owned by the app; the worker thread lives detached.
pub struct Hotkeys {
    rx: Receiver<HotkeyAction>,
    ctx: Arc<Mutex<Option<egui::Context>>>,
    registered: usize,
}

impl Hotkeys {
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let ctx: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
        let registered = spawn(tx, Arc::clone(&ctx));
        Self {
            rx,
            ctx,
            registered,
        }
    }

    /// Give the worker a context so a press wakes a hidden window.
    pub fn attach_ctx(&self, ctx: &egui::Context) {
        if let Ok(mut slot) = self.ctx.lock() {
            *slot = Some(ctx.clone());
        }
    }

    pub fn drain(&self) -> Vec<HotkeyAction> {
        self.rx.try_iter().collect()
    }

    /// How many of the shortcuts the OS accepted (0 = none, e.g. taken).
    pub fn registered(&self) -> usize {
        self.registered
    }
}

#[cfg(windows)]
fn spawn(tx: Sender<HotkeyAction>, ctx: Arc<Mutex<Option<egui::Context>>>) -> usize {
    let (ready_tx, ready_rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("wallmotion-hotkeys".into())
        .spawn(move || run(tx, ctx, ready_tx))
        .is_ok();
    if !spawned {
        return 0;
    }
    ready_rx.recv().unwrap_or(0)
}

#[cfg(windows)]
fn run(tx: Sender<HotkeyAction>, ctx: Arc<Mutex<Option<egui::Context>>>, ready: Sender<usize>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

    // Virtual-key codes (winuser.h) kept literal to avoid pulling in
    // extra feature flags for four constants.
    const VK_LEFT: u32 = 0x25;
    const VK_RIGHT: u32 = 0x27;
    const VK_SPACE: u32 = 0x20;
    const VK_M: u32 = 0x4D;

    let combos = [
        (1, VK_RIGHT, HotkeyAction::Next),
        (2, VK_LEFT, HotkeyAction::Prev),
        (3, VK_SPACE, HotkeyAction::Pause),
        (4, VK_M, HotkeyAction::Mute),
    ];
    let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
    let mut registered = 0usize;
    for (id, vk, _) in combos {
        if unsafe { RegisterHotKey(None, id, mods, vk) }.is_ok() {
            registered += 1;
        }
    }
    let _ = ready.send(registered);
    if registered == 0 {
        return;
    }

    unsafe {
        let mut msg = MSG::default();
        loop {
            // 0 = WM_QUIT, -1 = error; either way stop pumping.
            if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                break;
            }
            if msg.message != WM_HOTKEY {
                continue;
            }
            let action = match msg.wParam.0 as i32 {
                1 => HotkeyAction::Next,
                2 => HotkeyAction::Prev,
                3 => HotkeyAction::Pause,
                4 => HotkeyAction::Mute,
                _ => continue,
            };
            if tx.send(action).is_err() {
                break;
            }
            if let Ok(slot) = ctx.lock() {
                if let Some(ctx) = slot.as_ref() {
                    ctx.request_repaint();
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn spawn(_tx: Sender<HotkeyAction>, _ctx: Arc<Mutex<Option<egui::Context>>>) -> usize {
    0
}
