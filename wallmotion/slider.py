"""Pin-style volume slider: portrait rectangle with a triangle tip.

Qt stylesheets cannot draw the pin shape, so it is painted with
QPainter: a thin groove (accent fill up to the value) and a handle
made of a rounded rectangle with a downward triangle whose tip
touches the groove - like a map pin.
"""

from __future__ import annotations

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor, QPainter, QPainterPath
from PySide6.QtWidgets import QSlider, QStyle

from wallmotion.i18n import ACCENT, T

HANDLE_W = 16
HANDLE_RECT_H = 18
HANDLE_TIP_H = 8
GROOVE_H = 6


class PinSlider(QSlider):
    """Horizontal slider with a pin-shaped handle. Drop-in QSlider."""

    def __init__(self, orientation=Qt.Orientation.Horizontal, parent=None):
        super().__init__(orientation, parent)
        self.setMinimumHeight(HANDLE_RECT_H + HANDLE_TIP_H + 8)

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
            groove_y = rect.height() - 4 - GROOVE_H // 2
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

            # Handle: rounded rectangle with a narrow triangle tip below.
            top = groove_y - HANDLE_TIP_H - HANDLE_RECT_H
            left = cx - HANDLE_W // 2
            painter.setBrush(QColor(ACCENT))
            painter.drawRoundedRect(left, top, HANDLE_W, HANDLE_RECT_H, 5, 5)
            tip = QPainterPath()
            tip.moveTo(left + 3, top + HANDLE_RECT_H - 2)
            tip.lineTo(left + HANDLE_W - 3, top + HANDLE_RECT_H - 2)
            tip.lineTo(cx, top + HANDLE_RECT_H + HANDLE_TIP_H)
            tip.closeSubpath()
            painter.fillPath(tip, QColor(ACCENT))
            # Small light dot centered in the rectangle.
            dot_top = top + (HANDLE_RECT_H - 4) // 2
            painter.setBrush(QColor("#ffffff"))
            painter.drawEllipse(cx - 2, dot_top, 4, 4)
            painter.end()
        except Exception:
            try:
                painter.end()
            except Exception:
                pass
            super().paintEvent(_event)
