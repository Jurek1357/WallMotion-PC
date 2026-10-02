"""Tests for wallmotion.library - skipped where QtWidgets cannot load."""

import pytest

try:
    from PySide6 import QtGui, QtWidgets
    _HAVE_QT = True
except Exception:
    QtGui = None
    QtWidgets = None
    _HAVE_QT = False

pytestmark = pytest.mark.skipif(not _HAVE_QT, reason="Qt unavailable")

from wallmotion.library import (  # noqa: E402 (needs QtWidgets first)
    LibraryDialog,
    format_size,
    placeholder_pixmap,
    preview_pixmap,
)


@pytest.fixture(scope="module", autouse=True)
def _qt_app():
    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    yield app


def _strings():
    return {"library_title": "Library", "library_apply": "Set",
            "library_close": "Close", "library_count": "In library: {n}",
            "library_empty": "Empty"}


def _make_jpg(path, color="#112233"):
    pix = QtGui.QPixmap(64, 40)
    pix.fill(QtGui.QColor(color))
    assert pix.save(str(path), "JPG")


class TestLibraryDialog:
    def test_lists_files(self, tmp_path):
        (tmp_path / "v.mp4").write_bytes(b"x" * 100)
        _make_jpg(tmp_path / "b.jpg")
        (tmp_path / "c.txt").write_bytes(b"z")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            assert dlg.grid.count() == 2
            assert dlg.info_label.text() == "In library: 2"
            assert dlg.windowTitle() == "Library"
        finally:
            dlg.deleteLater()

    def test_empty(self, tmp_path):
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            assert dlg.grid.count() == 0
            assert dlg.info_label.text() == "Empty"
        finally:
            dlg.deleteLater()

    def test_apply_emits_and_accepts(self, tmp_path):
        _make_jpg(tmp_path / "b.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            received = []
            dlg.file_chosen.connect(received.append)
            dlg.grid.setCurrentRow(0)
            dlg._apply_current()
            assert len(received) == 1 and received[0].endswith("b.jpg")
            assert dlg.result() == LibraryDialog.DialogCode.Accepted
        finally:
            dlg.deleteLater()

    def test_no_selection_no_emit(self, tmp_path):
        _make_jpg(tmp_path / "b.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            received = []
            dlg.file_chosen.connect(received.append)
            dlg.grid.setCurrentRow(-1)
            dlg.grid.clearSelection()
            dlg._apply_current()
            assert received == []
        finally:
            dlg.deleteLater()


class TestSearchAndPreview:
    def _dlg(self, tmp_path):
        _make_jpg(tmp_path / "alpha.jpg")
        _make_jpg(tmp_path / "beta.jpg")
        (tmp_path / "movie.mp4").write_bytes(b"x" * 100)
        return LibraryDialog(_strings(), str(tmp_path), str(tmp_path))

    def test_filter(self, tmp_path):
        dlg = self._dlg(tmp_path)
        try:
            assert dlg.grid.count() == 3
            dlg.search_input.setText("alpha")
            visible = [dlg.grid.item(i).text() for i in range(3)
                       if not dlg.grid.item(i).isHidden()]
            assert visible == ["alpha.jpg"]
            dlg.search_input.clear()
            visible = [dlg.grid.item(i).text() for i in range(3)
                       if not dlg.grid.item(i).isHidden()]
            assert len(visible) == 3
        finally:
            dlg.deleteLater()

    def test_preview_follows_selection(self, tmp_path):
        dlg = self._dlg(tmp_path)
        try:
            dlg.grid.setCurrentRow(0)
            assert dlg.name_label.text() == dlg.grid.item(0).text()
            assert not dlg.preview_label.pixmap().isNull()
            assert "·" in dlg.meta_label.text()
        finally:
            dlg.deleteLater()


class TestFormatSize:
    def test_mb(self):
        assert format_size(2097152) == "2.0 MB"

    def test_kb(self):
        assert format_size(2048) == "2 KB"

    def test_garbage(self):
        assert format_size("x") == ""


class TestPreviews:
    def test_placeholder(self):
        pix = placeholder_pixmap()
        assert not pix.isNull()
        assert (pix.width(), pix.height()) == (200, 130)

    def test_image_preview(self, tmp_path):
        _make_jpg(tmp_path / "b.jpg")
        pix = preview_pixmap("image", str(tmp_path / "b.jpg"), str(tmp_path))
        assert not pix.isNull()

    def test_missing_falls_back(self, tmp_path):
        pix = preview_pixmap("image", str(tmp_path / "nope.jpg"),
                             str(tmp_path))
        assert not pix.isNull()
