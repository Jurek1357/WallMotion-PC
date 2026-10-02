"""Tests for wallmotion.slider - skipped where QtWidgets cannot load."""

import pytest

try:
    from PySide6 import QtWidgets

    from wallmotion.slider import HANDLE_W, PinSlider
    _HAVE_QT = True
except Exception:
    QtWidgets = None
    _HAVE_QT = False

pytestmark = pytest.mark.skipif(not _HAVE_QT, reason="Qt unavailable")


@pytest.fixture(scope="module", autouse=True)
def _qt_app():
    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    yield app


class TestPinSlider:
    def test_value_roundtrip(self):
        s = PinSlider()
        s.setRange(0, 100)
        s.setValue(37)
        assert s.value() == 37
        s.deleteLater()

    def test_handle_moves_with_value(self):
        s = PinSlider()
        s.resize(200, 34)
        s.setRange(0, 100)
        positions = []
        for v in (0, 50, 100):
            s.setValue(v)
            positions.append(s._handle_center_x())
        assert positions[0] < positions[1] < positions[2]
        assert positions[0] >= HANDLE_W // 2
        assert positions[2] <= 200 - HANDLE_W // 2
        s.deleteLater()

    def test_paints(self):
        from PySide6.QtGui import QPixmap
        s = PinSlider()
        s.resize(200, 34)
        s.setRange(0, 100)
        s.setValue(60)
        s.show()
        pix = s.grab()
        assert isinstance(pix, QPixmap) and not pix.isNull()
        s.deleteLater()
