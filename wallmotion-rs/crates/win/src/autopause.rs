//! Windows autopause sensors: fullscreen foreground window + battery.
//!
//! Thin wrappers over Win32; the pause decision + hysteresis live in
//! `wallmotion-core::autopause` (pure, unit-tested). Mirrors the sensor
//! half of `wallmotion/autopause.py`.

/// Shell windows that are never a "fullscreen app" (desktop itself).
#[cfg(windows)]
const SHELL_CLASSES: [&str; 3] = ["Progman", "WorkerW", "SHELLDLL_DefView"];

/// True when the foreground window exactly covers its monitor.
/// False on any sensor failure (fail-open: keep playing).
#[cfg(windows)]
pub fn is_fullscreen_app_active() -> bool {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowRect,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut cls = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut cls);
        if len > 0 {
            let name = String::from_utf16_lossy(&cls[..len as usize]);
            if SHELL_CLASSES.contains(&name.as_str()) {
                return false;
            }
        }
        let mut rect = Default::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmon.is_invalid() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut info).as_bool() {
            let w = (rect.left, rect.top, rect.right, rect.bottom);
            let m = info.rcMonitor;
            return wallmotion_core::autopause::is_fullscreen_rect(
                w,
                (m.left, m.top, m.right, m.bottom),
            );
        }
        false
    }
}

/// True when running on battery power. False on desktops / unknown.
#[cfg(windows)]
pub fn is_on_battery() -> bool {
    use windows::Win32::System::Power::GetSystemPowerStatus;
    unsafe {
        let mut status = Default::default();
        if GetSystemPowerStatus(&mut status).is_err() {
            return false;
        }
        wallmotion_core::autopause::parse_ac_line_status(status.ACLineStatus)
    }
}
