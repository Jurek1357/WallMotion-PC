"""In-app wallpaper library: panel with a thumbnail grid.

Wallpaper-Engine-style two panes (grid + live preview, search on top)
embedded as a tab in the main window - native Qt, no browser,
no QtWebEngine (which would break the AppImage). Double-click or
the Apply button emits file_chosen(path).
"""

from __future__ import annotations

import os

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtGui import QColor, QIcon, QPixmap
from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from wallmotion.webui import ensure_thumbnail, format_size, list_media

THUMB_W = 200
THUMB_H = 130
PREVIEW_W = 280
PREVIEW_H = 180


def toggle_favorite(favorites, path: str) -> set:
    """Add/remove path from the favorites set. Pure, unit-tested."""
    try:
        favs = set(favorites or [])
        if path in favs:
            favs.discard(path)
        elif path:
            favs.add(path)
        return favs
    except Exception:
        try:
            return set(favorites or [])
        except Exception:
            return set()


def placeholder_pixmap(w: int = THUMB_W, h: int = THUMB_H) -> QPixmap:
    """Dark tile used when a preview cannot be made."""
    try:
        pix = QPixmap(w, h)
        pix.fill(QColor("#25262e"))
        return pix
    except Exception:
        return QPixmap()


def preview_pixmap(kind: str, path: str, thumbs_dir: str | None = None,
                   w: int = THUMB_W, h: int = THUMB_H) -> QPixmap:
    """Thumbnail for a library item (video thumb or scaled image)."""
    try:
        source = None
        if kind == "video":
            source = ensure_thumbnail(path, thumbs_dir)
        else:
            source = path if os.path.exists(path) else None
        if source:
            pix = QPixmap(source)
            if not pix.isNull():
                return pix.scaled(
                    w, h,
                    Qt.AspectRatioMode.KeepAspectRatioByExpanding,
                    Qt.TransformationMode.SmoothTransformation)
    except Exception:
        pass
    return placeholder_pixmap(w, h)


class LibraryPanel(QWidget):
    """Thumbnail grid with preview pane. Emits file_chosen(path)."""

    file_chosen = Signal(str)

    def __init__(self, strings: dict, media_dir: str,
                 thumbs_dir: str | None = None, parent=None):
        super().__init__(parent)
        self._strings = strings
        self._media_dir = media_dir
        self._thumbs_dir = thumbs_dir
        self._meta: dict = {}
        self.setObjectName("library")
        self.favorites: set = set()
        self.on_favorites_changed = None
        self._fav_only = False

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)

        self.search_input = QLineEdit()
        self.search_input.setPlaceholderText(
            strings.get("library_search", "Search…"))
        self.search_input.setClearButtonEnabled(True)
        self.search_input.textChanged.connect(self._apply_filter)

        search_row = QHBoxLayout()
        search_row.setSpacing(8)
        search_row.addWidget(self.search_input, 1)
        self.fav_only_checkbox = QCheckBox(
            strings.get("library_fav_only", "Favorites"))
        self.fav_only_checkbox.toggled.connect(self._on_fav_only_toggled)
        search_row.addWidget(self.fav_only_checkbox)
        self.fav_button = QPushButton("★")
        self.fav_button.setToolTip(strings.get("library_fav_tip", "Favorite"))
        self.fav_button.setObjectName("secondary")
        self.fav_button.setFixedWidth(44)
        self.fav_button.clicked.connect(self._toggle_favorite_current)
        search_row.addWidget(self.fav_button)
        layout.addLayout(search_row)

        panes = QHBoxLayout()
        panes.setSpacing(12)

        self.grid = QListWidget()
        self.grid.setViewMode(QListWidget.ViewMode.IconMode)
        self.grid.setIconSize(QSize(THUMB_W, THUMB_H))
        self.grid.setResizeMode(QListWidget.ResizeMode.Adjust)
        self.grid.setMovement(QListWidget.Movement.Static)
        self.grid.setSpacing(8)
        self.grid.itemDoubleClicked.connect(self._apply_current)
        self.grid.currentItemChanged.connect(self._update_preview)
        panes.addWidget(self.grid, 1)

        side = QWidget()
        side.setFixedWidth(280)
        side_layout = QVBoxLayout(side)
        side_layout.setContentsMargins(0, 0, 0, 0)
        self.preview_label = QLabel()
        self.preview_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.preview_label.setMinimumSize(260, PREVIEW_H)
        self.preview_label.setStyleSheet(
            "background: #25262e; border-radius: 12px;")
        side_layout.addWidget(self.preview_label)
        self.name_label = QLabel()
        self.name_label.setWordWrap(True)
        self.name_label.setStyleSheet("font-weight: 600; font-size: 14px;")
        side_layout.addWidget(self.name_label)
        self.meta_label = QLabel()
        self.meta_label.setStyleSheet("color: #9195a3; font-size: 12px;")
        side_layout.addWidget(self.meta_label)
        side_layout.addStretch(1)
        panes.addWidget(side)

        layout.addLayout(panes, 1)

        row = QHBoxLayout()
        self.info_label = QLabel()
        row.addWidget(self.info_label, 1)
        self.apply_btn = QPushButton(strings.get("library_apply", "Set"))
        self.apply_btn.clicked.connect(self._apply_current)
        row.addWidget(self.apply_btn)
        layout.addLayout(row)

        self.refresh()

    def refresh(self) -> None:
        items = list_media(self._media_dir)
        self._meta = {e["name"]: e for e in items}
        self.grid.clear()
        for entry in items:
            try:
                name, kind = entry["name"], entry["kind"]
                full = os.path.join(self._media_dir, name)
                item = QListWidgetItem(
                    QIcon(preview_pixmap(kind, full, self._thumbs_dir)), name)
                item.setData(Qt.ItemDataRole.UserRole, full)
                self.grid.addItem(item)
            except Exception:
                continue
        if self.grid.count() > 0:
            self.grid.setCurrentRow(0)
        self._apply_filter(self.search_input.text())
        self._refresh_star()
        self._update_info()

    def _update_info(self) -> None:
        try:
            total = len(self._meta)
            if total:
                self.info_label.setText(
                    self._strings.get("library_count", "{n}").format(n=total))
            else:
                self.info_label.setText(
                    self._strings.get("library_empty", ""))
        except Exception:
            pass

    def _apply_filter(self, text: str) -> None:
        needle = (text or "").strip().lower()
        try:
            for i in range(self.grid.count()):
                item = self.grid.item(i)
                full = item.data(Qt.ItemDataRole.UserRole) or ""
                hidden = bool(needle) and needle not in item.text().lower()
                if not hidden and self._fav_only and full not in self.favorites:
                    hidden = True
                item.setHidden(hidden)
        except Exception:
            pass

    def _on_fav_only_toggled(self, checked: bool):
        self._fav_only = bool(checked)
        self._apply_filter(self.search_input.text())

    def _toggle_favorite_current(self):
        try:
            current = self.grid.currentItem()
            if current is None:
                return
            full = str(current.data(Qt.ItemDataRole.UserRole) or "")
            if not full:
                return
            self.favorites = toggle_favorite(self.favorites, full)
            try:
                if callable(self.on_favorites_changed):
                    self.on_favorites_changed()
            except Exception:
                pass
            self._refresh_star()
            self._apply_filter(self.search_input.text())
        except Exception:
            pass

    def _refresh_star(self) -> None:
        try:
            current = self.grid.currentItem()
            full = str(current.data(Qt.ItemDataRole.UserRole) or "") \
                if current is not None else ""
            starred = bool(full) and full in (self.favorites or set())
            self.fav_button.setText("★" if starred else "☆")
            self.fav_button.setToolTip(
                self._strings.get("library_fav_tip", "Favorite"))
        except Exception:
            pass

    def _update_preview(self, *_args) -> None:
        try:
            self._refresh_star()
            current = self.grid.currentItem()
            if current is None:
                self.preview_label.setPixmap(placeholder_pixmap(
                    PREVIEW_W, PREVIEW_H))
                self.name_label.clear()
                self.meta_label.clear()
                return
            name = current.text()
            full = current.data(Qt.ItemDataRole.UserRole) or ""
            kind = self._meta.get(name, {}).get("kind", "")
            size = self._meta.get(name, {}).get("size", 0)
            self.preview_label.setPixmap(
                preview_pixmap(kind, full, self._thumbs_dir,
                               PREVIEW_W, PREVIEW_H))
            self.name_label.setText(name)
            self.meta_label.setText(f"{kind} · {format_size(size)}")
        except Exception:
            pass

    def _apply_current(self, *_args):
        try:
            current = self.grid.currentItem()
            if current is None:
                return
            path = current.data(Qt.ItemDataRole.UserRole)
            if path:
                self.file_chosen.emit(str(path))
        except Exception:
            pass

    def retranslate(self, strings: dict) -> None:
        """Refresh texts after a language switch."""
        try:
            self._strings = strings
            self.search_input.setPlaceholderText(
                strings.get("library_search", "Search…"))
            self.apply_btn.setText(strings.get("library_apply", "Set"))
            self.fav_only_checkbox.setText(
                strings.get("library_fav_only", "Favorites"))
            self._refresh_star()
            self._update_info()
        except Exception:
            pass


class LibraryDialog(QDialog):
    """Standalone dialog wrapping LibraryPanel (kept for tests/tools)."""

    file_chosen = Signal(str)

    def __init__(self, strings: dict, media_dir: str,
                 thumbs_dir: str | None = None, parent=None):
        super().__init__(parent)
        self.setWindowTitle(strings.get("library_title", "Library"))
        self.setMinimumSize(720, 480)
        self.resize(820, 540)
        layout = QVBoxLayout(self)
        self.panel = LibraryPanel(strings, media_dir, thumbs_dir, self)
        self.panel.file_chosen.connect(self._forward)
        layout.addWidget(self.panel)
        row = QHBoxLayout()
        row.addStretch(1)
        close_btn = QPushButton(strings.get("library_close", "Close"))
        close_btn.setObjectName("secondary")
        close_btn.clicked.connect(self.reject)
        row.addWidget(close_btn)
        layout.addLayout(row)

    def _forward(self, path: str):
        try:
            self.file_chosen.emit(path)
            self.accept()
        except Exception:
            pass
