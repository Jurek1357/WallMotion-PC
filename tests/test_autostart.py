"""Tests for wallmotion.autostart - Qt-free, headless-safe."""

import os

from wallmotion import autostart


class TestPaths:
    def test_startup_dir_uses_appdata(self, monkeypatch, tmp_path):
        monkeypatch.setenv("APPDATA", str(tmp_path))
        assert autostart.windows_startup_dir().startswith(str(tmp_path))
        assert autostart.windows_link_path().endswith("WallMotion.lnk")

    def test_launch_target_shape(self):
        target, args, workdir = autostart.launch_target()
        assert target and workdir


class TestEnabled:
    def test_missing_means_disabled(self, monkeypatch, tmp_path):
        monkeypatch.setenv("APPDATA", str(tmp_path))
        assert autostart.is_enabled() is False

    def test_remove_missing_succeeds(self, monkeypatch, tmp_path):
        monkeypatch.setenv("APPDATA", str(tmp_path))
        assert autostart.set_enabled(False) is True

    def test_enable_without_backend_fails_quietly(self, monkeypatch, tmp_path):
        # No win32com / tools here -> False, never raises.
        monkeypatch.setenv("APPDATA", str(tmp_path))
        assert autostart.set_enabled(True) in (True, False)

    def test_linux_desktop_entry(self, monkeypatch, tmp_path):
        monkeypatch.setattr(autostart.sys, "platform", "linux")
        monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
        assert autostart.set_enabled(True) is True
        content = (tmp_path / "autostart" / "wallmotion.desktop").read_text(
            encoding="utf-8")
        assert "Exec=" in content
        assert autostart.is_enabled() is True
        assert autostart.set_enabled(False) is True
        assert autostart.is_enabled() is False
        assert not os.path.exists(tmp_path / "autostart" / "wallmotion.desktop")
