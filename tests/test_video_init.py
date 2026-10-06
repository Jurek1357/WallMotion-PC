"""Tests for VideoWallpaperWindow init.

TestInitAttributes is pure AST (runs everywhere, incl. headless Linux)
and guards the real bug class: state assigned outside __init__ so the
object starts half-built. TestConstruct needs Qt and skips without it.
"""

import ast
from pathlib import Path

import pytest

try:
    from PySide6 import QtWidgets
    _HAVE_QT = True
except Exception:
    QtWidgets = None
    _HAVE_QT = False

REQUIRED_ATTRS = [
    "_hdc", "_canvas", "_dw", "_dh", "_frame_size", "_frames",
    "_blits_ok", "_blit_fail", "_blit_mode", "_bmi",
    "_proc_min_interval", "_last_proc_t", "_skipped",
    "_max_src_pixels", "_downscaled", "_user_paused", "_autopaused",
    "_monitor", "_loop", "_volume", "_muted",
]


def _assigned_in(func: ast.FunctionDef) -> set:
    found = set()
    for node in ast.walk(func):
        if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
            if node.value.id == "self":
                found.add(node.attr)
        if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Attribute):
            target = node.target
            if isinstance(target.value, ast.Name) and target.value.id == "self":
                found.add(target.attr)
    return found


def _init_and_helpers() -> set:
    path = Path(__file__).parent.parent / "wallmotion" / "video.py"
    tree = ast.parse(path.read_text(encoding="utf-8"))
    cls = next(n for n in tree.body
               if isinstance(n, ast.ClassDef)
               and n.name == "VideoWallpaperWindow")
    funcs = {f.name: f for f in cls.body if isinstance(f, ast.FunctionDef)}
    init = funcs["__init__"]
    covered = _assigned_in(init)
    # One level of helpers called as self._helper() from __init__.
    for node in ast.walk(init):
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
            if (isinstance(node.func.value, ast.Name)
                    and node.func.value.id == "self"
                    and node.func.attr in funcs):
                covered |= _assigned_in(funcs[node.func.attr])
    return covered


class TestInitAttributes:
    def test_all_state_set_from_init(self):
        covered = _init_and_helpers()
        missing = [a for a in REQUIRED_ATTRS if a not in covered]
        assert not missing, f"attributes not set from __init__: {missing}"


@pytest.mark.skipif(not _HAVE_QT, reason="Qt unavailable")
class TestConstruct:
    def test_window_constructs_with_state(self):
        QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
        from wallmotion.video import VideoWallpaperWindow
        win = VideoWallpaperWindow("dummy.mp4")
        try:
            for attr in REQUIRED_ATTRS:
                assert hasattr(win, attr), attr
            assert win._blit_mode == "coloroncolor"
            assert win._frames == 0
        finally:
            try:
                win.close()
            except Exception:
                pass
            win.deleteLater()
