"""Jazyky, motivy a stylesheety (zadna logika, jen data + T())."""

from __future__ import annotations

ACCENT = "#7c5cff"

THEMES = {
    "dark": {
        "bg": "#1b1c22",
        "card": "#25262e",
        "card_hover": "#2d2f3a",
        "text": "#eceef2",
        "dim": "#9195a3",
        "border": "#3a3c47",
        "input": "#202128",
    },
    "light": {
        "bg": "#eef0f5",
        "card": "#ffffff",
        "card_hover": "#e2e6ee",
        "text": "#1c1e24",
        "dim": "#5d6472",
        "border": "#cfd5e1",
        "input": "#ffffff",
    },
}


def build_stylesheet(theme: str) -> str:
    t = THEMES.get(theme, THEMES["dark"])
    return f"""
QMainWindow {{
    background-color: {t['bg']};
}}
QWidget {{
    color: {t['text']};
    font-family: "Segoe UI", sans-serif;
}}
QLabel#title {{
    font-size: 17px;
    font-weight: 600;
}}
QLabel#subtitle {{
    color: {t['dim']};
    font-size: 12px;
}}
QLabel#status {{
    color: {t['dim']};
    font-size: 12px;
}}
QPushButton {{
    background-color: {ACCENT};
    color: white;
    border: none;
    border-radius: 8px;
    padding: 10px 16px;
    font-size: 13px;
    font-weight: 600;
}}
QPushButton:hover {{
    background-color: #8f74ff;
}}
QPushButton:pressed {{
    background-color: #6a4de0;
}}
QPushButton#secondary {{
    background-color: {t['card']};
    color: {t['text']};
}}
QPushButton#secondary:hover {{
    background-color: {t['card_hover']};
}}
QPushButton:disabled {{
    background-color: {t['card_hover']};
    color: {t['dim']};
}}
QCheckBox {{
    color: {t['dim']};
    font-size: 12px;
}}
QLineEdit {{
    background-color: {t['input']};
    color: {t['text']};
    border: 1px solid {t['border']};
    border-radius: 8px;
    padding: 9px 12px;
    font-size: 12px;
}}
QComboBox {{
    background-color: {t['card']};
    color: {t['text']};
    border: 1px solid {t['border']};
    border-radius: 8px;
    padding: 6px 10px;
    font-size: 12px;
}}
QComboBox QAbstractItemView {{
    background-color: {t['card']};
    color: {t['text']};
    selection-background-color: {ACCENT};
}}
"""


STRINGS = {
    "cs": {
        "subtitle": "Nastav si obrázek nebo video jako pozadí plochy",
        "screen_unknown": "Obrazovka: nezjištěna",
        "screen_one": "Obrazovka: {w} × {h}",
        "screen_multi": "Obrazovky ({n}): {parts} | primární {w} × {h}",
        "drop_hint": "Přetáhni sem obrázek nebo video\nnebo klikni pro výběr",
        "mute": "Ztlumit zvuk videa",
        "volume_label": "Hlasitost:",
        "apply": "Nastavit jako tapetu",
        "measure": "Změřit obrazovku znovu",
        "stop": "Zastavit / obnovit původní",
        "ready": "Připraveno.",
        "file_selected": "Soubor vybrán. Klikni na „Nastavit jako tapetu“.",
        "yt_placeholder": "Vlož YouTube odkaz…",
        "yt_button": "Stáhnout a nastavit",
        "yt_downloading": "Stahuji z YouTube… {p}",
        "yt_done": "Video staženo, nastavuji jako tapetu…",
        "yt_error": "Stažení selhalo: {e}",
        "yt_invalid_url": "Neplatný nebo nepodporovaný YouTube odkaz.",
        "open_folder": "Otevřít složku videí",
        "yt_need_ffmpeg": (
            "Toto video má obraz a zvuk odděleně – nainstaluj ffmpeg "
            "(do terminálu napiš: winget install ffmpeg), restartuj aplikaci "
            "a stáhni ho znovu."
        ),
        "lang_label": "Jazyk:",
        "theme_label": "Motiv:",
        "theme_dark": "Tmavý",
        "theme_light": "Světlý",
        "warn_unsupported_t": "Nepodporovaný formát",
        "warn_unsupported_m": "Tento typ souboru není podporovaný.",
        "warn_nofile_t": "Chybí soubor",
        "warn_nofile_m": "Nejdřív vyber obrázek nebo video.",
        "img_set": "Obrázek upraven na {w} × {h} a nastaven.",
        "vid_running": "Video tapeta běží na pozadí ({w} × {h}).",
        "vid_started_msg": "Video tapeta byla spuštěna.",
        "restored": "Původní tapeta byla obnovena.",
        "hidden_tray_msg": "Aplikace běží na pozadí v systémové liště.",
        "measured": "Obrazovka změřena: {w} × {h}.",
        "open_tray": "Otevřít",
        "quit_tray": "Ukončit",
        "vid_fail_t": "Video tapeta",
        "vid_fail_m": "Nepodařilo se vložit video na plochu (WorkerW nenalezeno).\nDetail v souboru {log}",
        "vid_decode_t": "Video se nepodařilo přehrát",
        "vid_decode_m": (
            "Z videa nepřišel ani jeden snímek (pravděpodobně nepodporovaný "
            "kodek, např. AV1/VP9 bez zvuku ve vysokém rozlišení). "
            "Zkus jiné video, ideálně H264 mp4 do 1080p."
        ),
        "app_name": "Live Wallpaper",
    },
    "en": {
        "subtitle": "Set an image or video as your desktop background",
        "screen_unknown": "Display: not detected",
        "screen_one": "Display: {w} × {h}",
        "screen_multi": "Displays ({n}): {parts} | primary {w} × {h}",
        "drop_hint": "Drag & drop an image or video here\nor click to browse",
        "mute": "Mute video sound",
        "volume_label": "Volume:",
        "apply": "Set as wallpaper",
        "measure": "Re-measure display",
        "stop": "Stop / restore original",
        "ready": "Ready.",
        "file_selected": "File selected. Click “Set as wallpaper”.",
        "yt_placeholder": "Paste a YouTube link…",
        "yt_button": "Download & set",
        "yt_downloading": "Downloading from YouTube… {p}",
        "yt_done": "Video downloaded, setting as wallpaper…",
        "yt_error": "Download failed: {e}",
        "yt_invalid_url": "Invalid or unsupported YouTube link.",
        "open_folder": "Open videos folder",
        "yt_need_ffmpeg": (
            "This video has separate video and audio tracks – install ffmpeg "
            "(run: winget install ffmpeg), restart the app and download it again."
        ),
        "lang_label": "Language:",
        "theme_label": "Theme:",
        "theme_dark": "Dark",
        "theme_light": "Light",
        "warn_unsupported_t": "Unsupported format",
        "warn_unsupported_m": "This file type is not supported.",
        "warn_nofile_t": "No file",
        "warn_nofile_m": "Pick an image or video first.",
        "img_set": "Image fitted to {w} × {h} and set.",
        "vid_running": "Video wallpaper running ({w} × {h}).",
        "vid_started_msg": "Video wallpaper started.",
        "restored": "Original wallpaper restored.",
        "hidden_tray_msg": "The app keeps running in the system tray.",
        "measured": "Display measured: {w} × {h}.",
        "open_tray": "Open",
        "quit_tray": "Quit",
        "vid_fail_t": "Video wallpaper",
        "vid_fail_m": "Could not embed the video into the desktop (WorkerW not found).\nSee {log}",
        "vid_decode_t": "Could not play the video",
        "vid_decode_m": (
            "No frames arrived from the video (likely an unsupported codec, "
            "e.g. AV1/VP9-only high-resolution file). Try a different video, "
            "ideally H264 mp4 up to 1080p."
        ),
        "app_name": "Live Wallpaper",
    },
}

LANGS = {"cs": "Čeština", "en": "English"}

CURRENT_THEME = dict(THEMES["dark"])


def T(key: str) -> str:
    """Aktualni barva motivu (po apply_theme se prepne dark/light)."""
    return CURRENT_THEME.get(key, THEMES["dark"].get(key, "#000000"))
