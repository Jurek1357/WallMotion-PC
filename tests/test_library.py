"""Tests for wallmotion.library - skipped where QtWidgets cannot load."""

import pytest

try:
    from PySide6 import QtGui, QtWidgets

    from wallmotion.library import (
        LibraryDialog,
        placeholder_pixmap,
        preview_pixmap,
        toggle_favorite,
    )
    _HAVE_QT = True
except Exception:
    QtGui = None
    QtWidgets = None
    _HAVE_QT = False

pytestmark = pytest.mark.skipif(not _HAVE_QT, reason="Qt unavailable")


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
            assert dlg.panel.grid.count() == 2
            assert dlg.panel.info_label.text() == "In library: 2"
            assert dlg.windowTitle() == "Library"
        finally:
            dlg.deleteLater()

    def test_empty(self, tmp_path):
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            assert dlg.panel.grid.count() == 0
            assert dlg.panel.info_label.text() == "Empty"
        finally:
            dlg.deleteLater()

    def test_apply_emits_and_accepts(self, tmp_path):
        _make_jpg(tmp_path / "b.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            received = []
            dlg.file_chosen.connect(received.append)
            dlg.panel.grid.setCurrentRow(0)
            dlg.panel._apply_current()
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
            dlg.panel.grid.setCurrentRow(-1)
            dlg.panel.grid.clearSelection()
            dlg.panel._apply_current()
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
            assert dlg.panel.grid.count() == 3
            dlg.panel.search_input.setText("alpha")
            visible = [dlg.panel.grid.item(i).text() for i in range(3)
                       if not dlg.panel.grid.item(i).isHidden()]
            assert visible == ["alpha.jpg"]
            dlg.panel.search_input.clear()
            visible = [dlg.panel.grid.item(i).text() for i in range(3)
                       if not dlg.panel.grid.item(i).isHidden()]
            assert len(visible) == 3
        finally:
            dlg.deleteLater()

    def test_preview_follows_selection(self, tmp_path):
        dlg = self._dlg(tmp_path)
        try:
            dlg.panel.grid.setCurrentRow(0)
            assert dlg.panel.name_label.text() == dlg.panel.grid.item(0).text()
            assert not dlg.panel.preview_label.pixmap().isNull()
            assert "·" in dlg.panel.meta_label.text()
        finally:
            dlg.deleteLater()

    def test_dialog_forwards_apply(self, tmp_path):
        _make_jpg(tmp_path / "b.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            received = []
            dlg.file_chosen.connect(received.append)
            dlg.panel.grid.setCurrentRow(0)
            dlg.panel._apply_current()
            assert len(received) == 1 and received[0].endswith("b.jpg")
            assert dlg.result() == LibraryDialog.DialogCode.Accepted
        finally:
            dlg.deleteLater()

    def test_panel_retranslate(self, tmp_path):
        _make_jpg(tmp_path / "b.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            dlg.panel.retranslate({"library_search": "Hledat…",
                                    "library_apply": "Nastavit",
                                    "library_count": "V knihovně: {n}",
                                    "library_empty": "Prazdne"})
            assert dlg.panel.search_input.placeholderText() == "Hledat…"
            assert dlg.panel.apply_btn.text() == "Nastavit"
            assert dlg.panel.info_label.text() == "V knihovně: 1"
        finally:
            dlg.deleteLater()


class TestFavorites:
    def test_toggle(self):
        assert toggle_favorite(set(), "/a.mp4") == {"/a.mp4"}
        assert toggle_favorite({"/a.mp4"}, "/a.mp4") == set()
        assert toggle_favorite(None, "") == set()

    def test_star_and_filter(self, tmp_path):
        _make_jpg(tmp_path / "a.jpg")
        _make_jpg(tmp_path / "b.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            panel = dlg.panel
            assert panel.fav_button.text() == "☆"
            panel.grid.setCurrentRow(0)
            panel._toggle_favorite_current()
            assert panel.fav_button.text() == "★"
            assert len(panel.favorites) == 1
            panel.fav_only_checkbox.setChecked(True)
            visible = [panel.grid.item(i).text()
                       for i in range(panel.grid.count())
                       if not panel.grid.item(i).isHidden()]
            assert len(visible) == 1
            panel._toggle_favorite_current()
            assert panel.fav_button.text() == "☆"
        finally:
            dlg.deleteLater()

    def test_toggle_reports_add_remove(self, tmp_path):
        _make_jpg(tmp_path / "a.jpg")
        dlg = LibraryDialog(_strings(), str(tmp_path), str(tmp_path))
        try:
            panel = dlg.panel
            calls = []
            panel.on_favorites_changed = lambda p, a: calls.append((p, a))
            panel.grid.setCurrentRow(0)
            panel._toggle_favorite_current()
            panel._toggle_favorite_current()
            assert calls[0][1] is True and calls[0][0].endswith("a.jpg")
            assert calls[1] == (calls[0][0], False)
        finally:
            dlg.deleteLater()


class TestSideButtons:
    def _panel(self, tmp_path):
        _make_jpg(tmp_path / "a.jpg")
        (tmp_path / "v.mp4").write_bytes(b"x" * 50)
        from wallmotion.library import LibraryPanel
        panel = LibraryPanel(_strings(), str(tmp_path), str(tmp_path))
        panel.grid.setCurrentRow(0)
        return panel

    def test_add_rotation_signal(self, tmp_path):
        panel = self._panel(tmp_path)
        try:
            received = []
            panel.add_rotation_requested.connect(received.append)
            panel._add_current_to_rotation()
            assert len(received) == 1 and received[0].endswith(".jpg")
        finally:
            panel.deleteLater()

    def test_delete_signal(self, tmp_path):
        panel = self._panel(tmp_path)
        try:
            received = []
            panel.delete_requested.connect(received.append)
            panel._delete_current()
            assert len(received) == 1
        finally:
            panel.deleteLater()

    def test_no_selection_no_signals(self, tmp_path):
        _make_jpg(tmp_path / "a.jpg")
        from wallmotion.library import LibraryPanel
        panel = LibraryPanel(_strings(), str(tmp_path), str(tmp_path))
        try:
            received = []
            panel.add_rotation_requested.connect(received.append)
            panel.delete_requested.connect(received.append)
            panel.grid.setCurrentRow(-1)
            panel.grid.clearSelection()
            panel._add_current_to_rotation()
            panel._delete_current()
            assert received == []
        finally:
            panel.deleteLater()


class TestDeleteMediaFile:
    def test_deletes_file_and_thumb(self, tmp_path):
        from wallmotion.library import delete_media_file
        media = tmp_path / "media"
        thumbs = tmp_path / "thumbs"
        media.mkdir()
        thumbs.mkdir()
        (media / "v.mp4").write_bytes(b"x" * 10)
        (thumbs / "v.jpg").write_bytes(b"y")
        assert delete_media_file(str(media), "v.mp4", str(thumbs)) is True
        assert not (media / "v.mp4").exists()
        assert not (thumbs / "v.jpg").exists()

    def test_missing_returns_false(self, tmp_path):
        from wallmotion.library import delete_media_file
        assert delete_media_file(str(tmp_path), "nope.mp4", None) is False

    def test_traversal_rejected(self, tmp_path):
        from wallmotion.library import delete_media_file
        assert delete_media_file(str(tmp_path), "../evil.mp4", None) is False


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
