//! Graphical session detection for the Linux backends.
//!
//! Mirrors `detect_session()` in `wallmotion/platform/linux.py`:
//! capability comes from `$XDG_SESSION_TYPE` / `$XDG_CURRENT_DESKTOP`.

/// Wayland compositors driven with mpvpaper/swww.
pub const WLROOTS_DESKTOPS: &[&str] = &[
    "sway", "hyprland", "wlroots", "wayfire", "river", "dwl", "labwc", "niri",
];

/// Session type: "x11" | "wayland" | "tty" | "unknown".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session: String,
    pub desktops: Vec<String>,
    pub is_gnome: bool,
    pub is_kde: bool,
    pub is_wlroots: bool,
}

/// Parse a session from explicit env values (pure, unit-tested).
pub fn detect_session_from(
    session_type: Option<&str>,
    wayland_display: Option<&str>,
    current_desktop: Option<&str>,
) -> SessionInfo {
    let mut session = session_type.unwrap_or("").to_lowercase();
    if session.is_empty() && wayland_display.is_some_and(|v| !v.is_empty()) {
        session = "wayland".to_string();
    }
    let raw = current_desktop.unwrap_or("").to_lowercase();
    let desktops: Vec<String> = raw
        .replace(';', ":")
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .collect();
    let has = |name: &str| desktops.iter().any(|d| d == name);
    SessionInfo {
        session: if session.is_empty() {
            "unknown".to_string()
        } else {
            session
        },
        is_gnome: has("gnome"),
        is_kde: has("kde") || has("plasma"),
        is_wlroots: desktops
            .iter()
            .any(|d| WLROOTS_DESKTOPS.contains(&d.as_str())),
        desktops,
    }
}

/// Describe the current process session from the real environment.
pub fn detect_session() -> SessionInfo {
    detect_session_from(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x11_xfce() {
        let info = detect_session_from(Some("x11"), None, Some("XFCE"));
        assert_eq!(info.session, "x11");
        assert_eq!(info.desktops, vec!["xfce"]);
        assert!(!info.is_gnome);
    }

    #[test]
    fn wayland_kde() {
        let info = detect_session_from(Some("wayland"), None, Some("KDE"));
        assert_eq!(info.session, "wayland");
        assert!(info.is_kde);
    }

    #[test]
    fn gnome_colon_list() {
        let info = detect_session_from(Some("wayland"), None, Some("ubuntu:GNOME"));
        assert!(info.is_gnome);
    }

    #[test]
    fn wayland_fallback_via_display() {
        let info = detect_session_from(None, Some("wayland-0"), None);
        assert_eq!(info.session, "wayland");
    }

    #[test]
    fn empty_is_unknown() {
        let info = detect_session_from(None, None, None);
        assert_eq!(info.session, "unknown");
        assert!(info.desktops.is_empty());
    }

    #[test]
    fn case_insensitive() {
        let info = detect_session_from(Some("X11"), None, None);
        assert_eq!(info.session, "x11");
    }
}
