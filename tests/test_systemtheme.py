"""Tests for wallmotion.systemtheme - Qt-free, headless-safe."""

from wallmotion.systemtheme import (
    parse_gnome_value,
    parse_windows_value,
    read_system_theme,
)


class TestParsers:
    def test_windows(self):
        assert parse_windows_value(1) == "light"
        assert parse_windows_value(0) == "dark"
        assert parse_windows_value("junk") is None
        assert parse_windows_value(None) is None

    def test_gnome(self):
        assert parse_gnome_value("'prefer-dark'") == "dark"
        assert parse_gnome_value("'default'") == "light"
        assert parse_gnome_value("") is None


class TestReader:
    def test_never_raises(self):
        assert read_system_theme() in ("dark", "light", None)
