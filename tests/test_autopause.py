"""Tests for wallmotion.autopause - pure decision logic, no Win32 needed."""

from wallmotion.autopause import (
    is_fullscreen_rect,
    parse_ac_line_status,
    parse_xprop_active_window,
    parse_xrandr_monitors,
    parse_xwininfo_geometry,
    should_pause,
)


class TestShouldPause:
    def test_nothing_fires(self):
        assert not should_pause(
            False, False, pause_on_fullscreen=True, pause_on_battery=True
        )

    def test_fullscreen_rule(self):
        assert should_pause(
            True, False, pause_on_fullscreen=True, pause_on_battery=False
        )
        assert not should_pause(
            True, False, pause_on_fullscreen=False, pause_on_battery=False
        )

    def test_battery_rule(self):
        assert should_pause(
            False, True, pause_on_fullscreen=False, pause_on_battery=True
        )
        assert not should_pause(
            False, True, pause_on_fullscreen=False, pause_on_battery=False
        )

    def test_either_rule_fires(self):
        assert should_pause(
            True, True, pause_on_fullscreen=True, pause_on_battery=True
        )
        # Only fullscreen enabled, only battery active -> no pause.
        assert not should_pause(
            False, True, pause_on_fullscreen=True, pause_on_battery=False
        )


class TestFullscreenRect:
    def test_exact_match(self):
        assert is_fullscreen_rect((0, 0, 1920, 1080), (0, 0, 1920, 1080))

    def test_windowed(self):
        assert not is_fullscreen_rect((100, 100, 1800, 900), (0, 0, 1920, 1080))

    def test_borderless_offset_by_one(self):
        assert not is_fullscreen_rect((0, 0, 1920, 1079), (0, 0, 1920, 1080))

    def test_secondary_monitor(self):
        assert is_fullscreen_rect(
            (1920, 0, 3840, 1080), (1920, 0, 3840, 1080)
        )


class TestX11FullscreenParsers:
    XPROP = "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x3e00007\n"
    XWININFO = """xwininfo: Window id: 0x3e00007 "mpv"

  Absolute upper-left X:  1920
  Absolute upper-left Y:  0
  Relative upper-left X:  0
  Relative upper-left Y:  0
  Width: 1920
  Height: 1080
  Depth: 24
"""
    XRANDR = """Monitors: 2
 0: +*eDP-1 1920/344x1200/215+0+0  eDP-1
 1: +HDMI-1 1920/510x1080/290+1920+0  HDMI-1
"""

    def test_active_window_id(self):
        assert parse_xprop_active_window(self.XPROP) == 0x3E00007

    def test_active_window_none(self):
        assert parse_xprop_active_window(
            "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n") == 0
        assert parse_xprop_active_window("") is None

    def test_xwininfo_rect(self):
        assert parse_xwininfo_geometry(self.XWININFO) == (1920, 0, 3840, 1080)

    def test_xwininfo_garbage(self):
        assert parse_xwininfo_geometry("not xwininfo") is None

    def test_xrandr_monitors(self):
        mons = parse_xrandr_monitors(self.XRANDR)
        assert mons == [(0, 0, 1920, 1200), (1920, 0, 3840, 1080)]

    def test_fullscreen_match_via_parsers(self):
        rect = parse_xwininfo_geometry(self.XWININFO)
        assert any(rect == m for m in parse_xrandr_monitors(self.XRANDR))


class TestAcLineStatus:
    def test_battery(self):
        assert parse_ac_line_status(0) is True

    def test_ac_power(self):
        assert parse_ac_line_status(1) is False

    def test_unknown_means_no_pause(self):
        assert parse_ac_line_status(255) is False
