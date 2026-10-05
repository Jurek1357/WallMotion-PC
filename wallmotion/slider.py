"""Plain rectangular volume slider handle.

Qt stylesheets cannot do a custom handle shape, so it is painted with
QPainter: a thin groove (accent fill up to the value) and a plain
rounded-rectangle handle centered on the groove.
"""

from __future__ import annotations

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor, QPainter
from PySide6.QtWidgets import QSlider, QStyle

from wallmotion.i18n import ACCENT, T

HANDLE_W = 16
HANDLE_H = 22
GROOVE_H = 6


class PinSlider(QSlider):
    """Horizontal slider with a plain rectangular handle. Drop-in QSlider."""

    def __init__(self, orientation=Qt.Orientation.Horizontal, parent=None):
        super().__init__(orientation, parent)
        self.setMinimumHeight(HANDLE_H + 8)

    def _handle_center_x(self) -> int:
        try:
            span = max(1, self.width() - HANDLE_W)
            pos = QStyle.sliderPositionFromValue(
                self.minimum(), self.maximum(), self.value(), span, False)
            return int(pos + HANDLE_W / 2)
        except Exception:
            return HANDLE_W // 2

    def paintEvent(self, _event) -> None:
        try:
            painter = QPainter(self)
            painter.setRenderHint(QPainter.RenderHint.Antialiasing)
            rect = self.rect()
            groove_y = rect.height() // 2
            cx = self._handle_center_x()

            # Groove: full bar in border color, filled part in accent.
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(QColor(T("border")))
            painter.drawRoundedRect(
                HANDLE_W // 2, groove_y - GROOVE_H // 2,
                rect.width() - HANDLE_W, GROOVE_H, 3, 3)
            if cx > HANDLE_W // 2:
                painter.setBrush(QColor(ACCENT))
                painter.drawRoundedRect(
                    HANDLE_W // 2, groove_y - GROOVE_H // 2,
                    cx - HANDLE_W // 2, GROOVE_H, 3, 3)

            # Handle: plain rounded rectangle centered on the groove.
            painter.setBrush(QColor(ACCENT))
            painter.drawRoundedRect(
                cx - HANDLE_W // 2, groove_y - HANDLE_H // 2,
                HANDLE_W, HANDLE_H, 5, 5)
            painter.end()
        except Exception:
            try:
                painter.end()
            except Exception:
                pass
            super().paintEvent(_event)
