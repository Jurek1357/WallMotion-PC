//! Language (CZ/EN) for the native settings window.
//!
//! Strings come from the shared `locales/*.json` (the same files the
//! Python app uses) plus a small Rust-only extra table below, so both
//! apps stay in sync. Placeholders keep the Python `{name}` shapes and
//! are filled with plain `.replace()` (no `format!` brace juggling).

use std::collections::HashMap;
use std::sync::OnceLock;

/// UI language. Default mirrors the Python app (`cs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Cs,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Cs => "cs",
            Lang::En => "en",
        }
    }

    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("en") {
            Lang::En
        } else {
            Lang::Cs
        }
    }

    /// Switch CZ <-> EN (the header button).
    pub fn toggle(self) -> Self {
        match self {
            Lang::Cs => Lang::En,
            Lang::En => Lang::Cs,
        }
    }

    /// Header button label: the language it switches TO
    /// (CZ active → offers EN and vice versa).
    pub fn button_label(self) -> &'static str {
        match self {
            Lang::Cs => "EN",
            Lang::En => "CZ",
        }
    }
}

/// Rust-only strings (native statuses, tool messages). The shared JSONs
/// stay untouched so the Python side never breaks.
fn extras(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::En => &[
            ("status_pick", "Pick a video file, then Set as wallpaper."),
            ("status_stopped", "Stopped."),
            ("status_playing", "Playing."),
            ("status_paused", "Paused."),
            (
                "status_autopaused",
                "Auto-paused (fullscreen app or battery).",
            ),
            ("no_canvas", "No desktop canvas (run on Windows)."),
            ("mpv_missing", "mpv not found (PATH or WALLMOTION_MPV)."),
            ("mpv_failed", "mpv failed to start: {e}"),
            ("yt_url_first", "Paste a YouTube link first."),
            ("yt_need_tool", "yt-dlp not found — click Get yt-dlp below."),
            ("yt_pick", "Pick videos, then Download selected."),
            ("yt_cancelled", "Cancelled."),
            ("yt_dling_ytdlp", "Downloading yt-dlp…"),
            ("yt_updating", "Updating yt-dlp…"),
            ("yt_dling_ffmpeg", "Downloading ffmpeg (~80 MB)…"),
            ("yt_ready", "yt-dlp ready ({v})"),
            ("yt_updated", "yt-dlp updated ({v})"),
            ("yt_ffmpeg_ready", "ffmpeg ready (1080p merges on)"),
            ("btn_get_ytdlp", "Get yt-dlp"),
            ("btn_update", "Update"),
            ("btn_get_ffmpeg", "Get ffmpeg"),
            ("tool_ytdlp_found", "yt-dlp found"),
            ("tool_ytdlp_missing", "yt-dlp missing"),
            ("tool_ffmpeg_missing", "ffmpeg missing (~720p max)"),
            ("tool_ffmpeg_ok", "ffmpeg ok"),
            ("refresh", "Refresh"),
            ("mute_short", "Mute"),
            ("browse", "Browse…"),
            (
                "lib_empty_pick",
                "Pick a wallpaper above (double-click sets it).",
            ),
            ("fav_on", "★ Favorited"),
            ("fav_off", "☆ Favorite"),
            ("mon_name", "Monitor"),
            ("mon_primary", " - primary"),
            ("dl_selected", "Download selected"),
            ("dl_clear", "Clear"),
            ("dl_cancel", "Cancel"),
            ("dl_download", "Download"),
            ("yt_nothing", "Nothing selected."),
            ("pl_title", "Playlist ({n}):"),
            ("yt_title", "YouTube download:"),
            (
                "status_video_where",
                "Playing behind icons ({w}x{h} at {x},{y} on {m}).",
            ),
            ("video_file", "Video file:"),
            ("file_hint", "Pick a video or image…"),
            ("rotation_title", "Rotation playlist:"),
            ("img_failed", "Could not set the image."),
            ("tool_nostart", "cannot start yt-dlp: {e}"),
            ("tool_blocked", "downloaded yt-dlp does not run (blocked?)"),
            ("dl_nofile", "file not found"),
        ],
        Lang::Cs => &[
            (
                "status_pick",
                "Vyber video a klikni na Nastavit jako tapetu.",
            ),
            ("status_stopped", "Zastaveno."),
            ("status_playing", "Přehrává se."),
            ("status_paused", "Pozastaveno."),
            (
                "status_autopaused",
                "Auto-pauza (fullscreen aplikace nebo baterie).",
            ),
            ("no_canvas", "Nenalezeno plátno plochy (spusť ve Windows)."),
            ("mpv_missing", "mpv nenalezeno (PATH nebo WALLMOTION_MPV)."),
            ("mpv_failed", "mpv se nepodařilo spustit: {e}"),
            ("yt_url_first", "Nejřív vlož YouTube odkaz."),
            (
                "yt_need_tool",
                "yt-dlp chybí — klikni níže na Stáhnout yt-dlp.",
            ),
            ("yt_pick", "Vyber videa a klikni Stáhnout vybrané."),
            ("yt_cancelled", "Zrušeno."),
            ("yt_dling_ytdlp", "Stahuji yt-dlp…"),
            ("yt_updating", "Aktualizuji yt-dlp…"),
            ("yt_dling_ffmpeg", "Stahuji ffmpeg (~80 MB)…"),
            ("yt_ready", "yt-dlp připraveno ({v})"),
            ("yt_updated", "yt-dlp aktualizováno ({v})"),
            (
                "yt_ffmpeg_ready",
                "ffmpeg připraveno (slučování 1080p zapnuto)",
            ),
            ("btn_get_ytdlp", "Stáhnout yt-dlp"),
            ("btn_update", "Aktualizovat"),
            ("btn_get_ffmpeg", "Stáhnout ffmpeg"),
            ("tool_ytdlp_found", "yt-dlp nalezeno"),
            ("tool_ytdlp_missing", "yt-dlp chybí"),
            ("tool_ffmpeg_missing", "ffmpeg chybí (max ~720p)"),
            ("tool_ffmpeg_ok", "ffmpeg ok"),
            ("refresh", "Obnovit"),
            ("mute_short", "Ztlumit"),
            ("browse", "Procházet…"),
            ("lib_empty_pick", "Vyber tapetu výše (dvojklik nastaví)."),
            ("fav_on", "★ Oblíbené"),
            ("fav_off", "☆ Oblíbené"),
            ("mon_name", "Monitor"),
            ("mon_primary", " - primární"),
            ("dl_selected", "Stáhnout vybrané"),
            ("dl_clear", "Vymazat"),
            ("dl_cancel", "Zrušit"),
            ("dl_download", "Stáhnout"),
            ("yt_nothing", "Nic nevybráno."),
            ("pl_title", "Playlist ({n}):"),
            ("yt_title", "Stahování z YouTube:"),
            (
                "status_video_where",
                "Hraje za ikonami ({w}x{h} na {x},{y}, {m}).",
            ),
            ("video_file", "Video soubor:"),
            ("file_hint", "Vyber video nebo obrázek…"),
            ("rotation_title", "Rotace tapet:"),
            ("img_failed", "Obrázek se nepodařilo nastavit."),
            ("tool_nostart", "yt-dlp se nepodařilo spustit: {e}"),
            ("tool_blocked", "stažený yt-dlp se nespustí (blokováno?)"),
            ("dl_nofile", "soubor se nenašel"),
        ],
    }
}

fn load_map(lang: Lang) -> HashMap<String, String> {
    let src = match lang {
        Lang::Cs => include_str!("../../../../locales/cs.json"),
        Lang::En => include_str!("../../../../locales/en.json"),
    };
    let mut map: HashMap<String, String> = serde_json::from_str(src).unwrap_or_default();
    for (k, v) in extras(lang) {
        map.insert((*k).to_string(), (*v).to_string());
    }
    map
}

static MAPS: OnceLock<(HashMap<String, String>, HashMap<String, String>)> = OnceLock::new();

fn maps() -> &'static (HashMap<String, String>, HashMap<String, String>) {
    MAPS.get_or_init(|| (load_map(Lang::Cs), load_map(Lang::En)))
}

fn map_for(lang: Lang) -> &'static HashMap<String, String> {
    match lang {
        Lang::Cs => &maps().0,
        Lang::En => &maps().1,
    }
}

/// Translated string for `key` (English fallback, then the key itself).
pub fn tr(lang: Lang, key: &str) -> String {
    map_for(lang)
        .get(key)
        .or_else(|| map_for(Lang::En).get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Translated template with `{name}` placeholders filled.
pub fn trf(lang: Lang, key: &str, args: &[(&str, &str)]) -> String {
    let mut s = tr(lang, key);
    for (name, value) in args {
        s = s.replace(&format!("{{{name}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_cover_ui_keys() {
        // Every key the native UI uses must exist in both languages.
        let keys = [
            "settings_title",
            "library_title",
            "apply",
            "stop",
            "video_pause",
            "video_resume",
            "monitor_label",
            "monitor_all",
            "mute",
            "volume_label",
            "pause_fullscreen",
            "pause_battery",
            "rotation_enable",
            "rotation_interval",
            "rotation_shuffle",
            "rotation_repeat",
            "rotation_add",
            "rotation_play",
            "rotation_skip",
            "rotation_clear",
            "rotation_count",
            "library_search",
            "library_fav_only",
            "library_count",
            "library_set",
            "library_add_rotation",
            "library_delete",
            "open_folder",
            "yt_downloading",
            "yt_queue_progress",
            "yt_done",
            "yt_error",
            "yt_invalid_url",
            "yt_need_ffmpeg",
            "yt_need_signin",
            "yt_unavailable",
            "yt_timeout",
            "yt_playlist_empty",
            "yt_playlist_loading",
            "theme_light",
            "theme_dark",
            "theme_auto",
            "warn_nofile_m",
            "img_set",
            "vid_running",
            "status_pick",
            "status_stopped",
            "status_playing",
            "video_file",
            "file_hint",
            "rotation_title",
            "img_failed",
            "mon_name",
            "fav_on",
            "refresh",
            "browse",
        ];
        for key in keys {
            assert!(!tr(Lang::Cs, key).is_empty(), "cs:{key}");
            assert!(!tr(Lang::En, key).is_empty(), "en:{key}");
            assert_ne!(tr(Lang::Cs, "__missing__"), "");
        }
    }

    #[test]
    fn placeholders_fill() {
        let s = trf(Lang::En, "img_set", &[("w", "1920"), ("h", "1080")]);
        assert!(s.contains("1920") && s.contains("1080"));
        let s = trf(Lang::Cs, "yt_error", &[("e", "x")]);
        assert!(s.contains('x'));
    }

    #[test]
    fn lang_codes_roundtrip() {
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("cs"), Lang::Cs);
        assert_eq!(Lang::from_code("??"), Lang::Cs);
        assert_eq!(Lang::Cs.toggle(), Lang::En);
    }
}
