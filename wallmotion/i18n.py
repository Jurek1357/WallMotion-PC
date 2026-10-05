"""Languages, themes and stylesheets (data + T())."""

from __future__ import annotations

import json
import os
import sys

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
QScrollArea {{
    background: transparent;
    border: none;
}}
QWidget#content {{
    background-color: {t['bg']};
}}
QWidget#library {{
    background-color: {t['bg']};
}}
QTabWidget::pane {{
    border: none;
    background-color: {t['bg']};
}}
QTabBar {{
    background: transparent;
    border: none;
}}
QTabBar::tab {{
    background-color: {t['card']};
    color: {t['dim']};
    padding: 8px 20px;
    margin-right: 4px;
    border-top-left-radius: 8px;
    border-top-right-radius: 8px;
    font-size: 12px;
    font-weight: 600;
}}
QTabBar::tab:selected {{
    background-color: {ACCENT};
    color: white;
}}
QTabBar::tab:hover:!selected {{
    background-color: {t['card_hover']};
}}
QListWidget#library_grid {{
    background-color: {t['bg']};
    border: none;
    outline: none;
}}
QListWidget#library_grid::item {{
    color: {t['text']};
    border-radius: 8px;
    padding: 4px;
}}
QListWidget#library_grid::item:selected {{
    background-color: {ACCENT};
    color: white;
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
    border-radius: 7px;
    padding: 7px 12px;
    font-size: 12px;
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
    padding-right: 36px;
    font-size: 12px;
}}
QComboBox::drop-down {{
    subcontrol-origin: padding;
    subcontrol-position: top right;
    width: 30px;
    border: none;
    border-top-right-radius: 8px;
    border-bottom-right-radius: 8px;
    background: transparent;
}}
QComboBox::down-arrow {{
    width: 12px;
    height: 12px;
}}
QComboBox QAbstractItemView {{
    background-color: {t['card']};
    color: {t['text']};
    selection-background-color: {ACCENT};
    selection-color: white;
    border: 1px solid {t['border']};
    border-radius: 8px;
    padding: 4px;
    outline: none;
}}
QComboBox QAbstractItemView::item {{
    padding: 8px 12px;
    border-radius: 6px;
    min-height: 20px;
}}
"""


def _locales_dir() -> str:
    """Directory with cs.json / en.json (works in frozen exe via _MEIPASS)."""
    try:
        meipass = getattr(sys, "_MEIPASS", None)
        if meipass:
            return os.path.join(meipass, "locales")
        return os.path.join(
            os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
            "locales",
        )
    except Exception:
        return "locales"


def _load_strings() -> dict:
    """Load UI strings from locales/*.json (fails fast when missing)."""
    strings = {}
    for lang in ("cs", "en"):
        path = os.path.join(_locales_dir(), f"{lang}.json")
        with open(path, encoding="utf-8") as f:
            strings[lang] = json.load(f)
    return strings


STRINGS = _load_strings()

LANGS = {"cs": "Čeština", "en": "English"}

CURRENT_THEME = dict(THEMES["dark"])


def T(key: str) -> str:
    """Current theme color (switches dark/light after apply_theme)."""
    return CURRENT_THEME.get(key, THEMES["dark"].get(key, "#000000"))
