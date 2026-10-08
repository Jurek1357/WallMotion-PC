//! App theme: light/dark + follow-the-OS toggle.
//!
//! Mirrors `wallmotion/systemtheme.py`: on Windows the OS choice lives in
//! `HKCU\...\Themes\Personalize\AppsUseLightTheme` (1 = light). The UI
//! polls it every few seconds while follow-mode is on; the manual sun
//! button picks a fixed theme and switches follow off (same as Python).

/// Light or dark visuals. Same `"light"`/`"dark"` codes as the Python app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppTheme {
    Light,
    #[default]
    Dark,
}

impl AppTheme {
    pub fn code(self) -> &'static str {
        match self {
            AppTheme::Light => "light",
            AppTheme::Dark => "dark",
        }
    }

    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("light") {
            AppTheme::Light
        } else {
            AppTheme::Dark
        }
    }

    /// Sun/moon glyph for the header toggle button (text, not emoji).
    pub fn button_glyph(self) -> &'static str {
        match self {
            AppTheme::Light => "☀",
            AppTheme::Dark => "☾",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            AppTheme::Light => AppTheme::Dark,
            AppTheme::Dark => AppTheme::Light,
        }
    }
}

/// Current OS theme, or `None` when unknown/unsupported.
pub fn read_system_theme() -> Option<AppTheme> {
    #[cfg(windows)]
    {
        read_windows_theme()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn read_windows_theme() -> Option<AppTheme> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_DWORD};
    let mut data = [0u8; 4];
    let mut len = 4u32;
    unsafe {
        let ok = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_DWORD,
            None,
            Some(data.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut len),
        )
        .is_ok();
        if !ok {
            return None;
        }
    }
    Some(if u32::from_le_bytes(data) == 1 {
        AppTheme::Light
    } else {
        AppTheme::Dark
    })
}

/// Poll interval while follow-mode is on (mirrors the Python 15 s timer).
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_roundtrip() {
        assert_eq!(AppTheme::from_code("light"), AppTheme::Light);
        assert_eq!(AppTheme::from_code("dark"), AppTheme::Dark);
        assert_eq!(AppTheme::from_code("??"), AppTheme::Dark);
        assert_eq!(AppTheme::Light.code(), "light");
        assert_eq!(AppTheme::Dark.toggle(), AppTheme::Light);
    }

    #[test]
    fn system_probe_never_panics() {
        let _ = read_system_theme();
    }
}
