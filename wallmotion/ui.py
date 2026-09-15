"""Uzivatelske rozhrani: DropZone, hlavni okno, spousteci main()."""

from __future__ import annotations

import json
import os
import sys

from PySide6.QtCore import QLoggingCategory, Qt, QUrl, Signal
from PySide6.QtGui import (
    QAction,
    QDragEnterEvent,
    QDropEvent,
    QIcon,
    QPixmap,
)
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMenu,
    QMessageBox,
    QPushButton,
    QSlider,
    QStyle,
    QSystemTrayIcon,
    QVBoxLayout,
    QWidget,
)

from wallmotion import screens
from wallmotion.config import CONFIG_PATH
from wallmotion.i18n import (
    ACCENT,
    CURRENT_THEME,
    LANGS,
    STRINGS,
    THEMES,
    T,
    build_stylesheet,
)
from wallmotion.utils import DEBUG_LOG, _asset_path, _quiet_ffmpeg, debug_log
from wallmotion.video import VideoWallpaperWindow
from wallmotion.wallpaper import (
    IMAGE_EXTS,
    VIDEO_EXTS,
    fit_image_to_screen,
    get_current_wallpaper,
    set_static_wallpaper,
)
from wallmotion.youtube import (
    YT_DIR,
    DownloadWorker,
    is_valid_youtube_url,
)


class DropZone(QFrame):
    file_dropped = Signal(str)
    clicked = Signal()

    def __init__(self):
        super().__init__()
        self.setAcceptDrops(True)
        self.setFixedHeight(150)
        self.setCursor(Qt.PointingHandCursor)
        self._has_file = False
        self._hint = ""
        self._set_style(T("card"), T("border"))

        layout = QVBoxLayout(self)
        layout.setAlignment(Qt.AlignCenter)

        self.icon_label = QLabel()
        self.icon_label.setAlignment(Qt.AlignCenter)
        self.icon_label.setStyleSheet("background: transparent;")
        layout.addWidget(self.icon_label)
        self._show_system_icon(QStyle.StandardPixmap.SP_DialogOpenButton)

        self.text_label = QLabel()
        self.text_label.setAlignment(Qt.AlignCenter)
        self.text_label.setStyleSheet(f"color: {T('dim')}; font-size: 12px; background: transparent;")
        layout.addWidget(self.text_label)
        self.set_hint(STRINGS["cs"]["drop_hint"])

    def _set_style(self, color, border):
        self.setStyleSheet(f"""
            QFrame {{
                background-color: {color};
                border: 2px dashed {border};
                border-radius: 12px;
            }}
        """)

    def _show_system_icon(self, which) -> None:
        """Systemova ikona stylu Windows misto emoji (QLabel s pixmapou)."""
        try:
            pm = self.style().standardIcon(which).pixmap(52, 52)
            if not pm.isNull():
                self.icon_label.setPixmap(pm)
                return
        except Exception:
            pass
        self.icon_label.clear()

    def set_hint(self, text: str):
        self._hint = text
        if not self._has_file:
            self.text_label.setText(text)

    def refresh_style(self):
        self.text_label.setStyleSheet(
            f"color: {T('dim')}; font-size: 12px; background: transparent;"
        )
        self.icon_label.setStyleSheet("background: transparent;")
        self._set_style(T("card"), ACCENT if self._has_file else T("border"))

    def set_file(self, path: str):
        self._has_file = True
        name = os.path.basename(path)
        ext = os.path.splitext(path)[1].lower()
        if ext in IMAGE_EXTS:
            pix = QPixmap(path)
            if not pix.isNull():
                self.icon_label.setPixmap(
                    pix.scaledToHeight(70, Qt.SmoothTransformation)
                )
            else:
                self._show_system_icon(QStyle.StandardPixmap.SP_FileIcon)
        else:
            self._show_system_icon(QStyle.StandardPixmap.SP_MediaPlay)
            self.icon_label.setStyleSheet("background: transparent;")
        self.text_label.setText(name)
        self._set_style(T("card"), ACCENT)

    def dragEnterEvent(self, event: QDragEnterEvent):
        if event.mimeData().hasUrls():
            event.acceptProposedAction()
            self._set_style(T("card_hover"), ACCENT)

    def dragLeaveEvent(self, event):
        self._set_style(T("card"), T("border"))

    def dropEvent(self, event: QDropEvent):
        urls = event.mimeData().urls()
        if urls:
            path = urls[0].toLocalFile()
            self.file_dropped.emit(path)

    def mousePressEvent(self, event):
        self.clicked.emit()


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Live Wallpaper")
        self.setFixedSize(430, 640)

        self.video_window = None
        self.selected_path = None
        self.original_wallpaper = get_current_wallpaper()
        self.screen_info = screens.measure_screens()
        self.lang = "cs"
        self.theme = "dark"
        self.yt_worker = None

        central = QWidget()
        self.setCentralWidget(central)
        layout = QVBoxLayout(central)
        layout.setContentsMargins(24, 20, 24, 20)
        layout.setSpacing(10)

        self.title_label = QLabel("Live Wallpaper")
        self.title_label.setObjectName("title")
        layout.addWidget(self.title_label)

        self.subtitle_label = QLabel()
        self.subtitle_label.setObjectName("subtitle")
        layout.addWidget(self.subtitle_label)

        self.screen_label = QLabel()
        self.screen_label.setObjectName("status")
        layout.addWidget(self.screen_label)

        # -- nastaveni: jazyk + motiv -------------------------------------
        settings_row = QHBoxLayout()
        settings_row.setSpacing(8)
        self.lang_label = QLabel()
        settings_row.addWidget(self.lang_label)
        self.lang_combo = QComboBox()
        for code, name in LANGS.items():
            self.lang_combo.addItem(name, code)
        self.lang_combo.currentIndexChanged.connect(self._on_lang_changed)
        settings_row.addWidget(self.lang_combo, 1)
        self.theme_label = QLabel()
        settings_row.addWidget(self.theme_label)
        self.theme_combo = QComboBox()
        self.theme_combo.addItem("Tmavý", "dark")
        self.theme_combo.addItem("Světlý", "light")
        self.theme_combo.currentIndexChanged.connect(self._on_theme_changed)
        settings_row.addWidget(self.theme_combo, 1)
        layout.addLayout(settings_row)

        self.drop_zone = DropZone()
        self.drop_zone.file_dropped.connect(self._on_file_chosen)
        self.drop_zone.clicked.connect(self.browse_file)
        layout.addWidget(self.drop_zone)

        # -- YouTube odkaz -------------------------------------------------
        yt_row = QHBoxLayout()
        yt_row.setSpacing(8)
        self.yt_input = QLineEdit()
        yt_row.addWidget(self.yt_input, 1)
        self.yt_button = QPushButton()
        self.yt_button.setObjectName("secondary")
        self.yt_button.clicked.connect(self.download_youtube)
        yt_row.addWidget(self.yt_button)
        layout.addLayout(yt_row)

        # -- slozka se stazenymi videi -----------------------------------
        self.folder_button = QPushButton()
        self.folder_button.setObjectName("secondary")
        self.folder_button.clicked.connect(self.open_downloads_folder)
        layout.addWidget(self.folder_button)

        self.mute_checkbox = QCheckBox()
        self.mute_checkbox.setChecked(True)
        self.mute_checkbox.toggled.connect(self._on_mute_toggled)
        layout.addWidget(self.mute_checkbox)

        # -- hlasitost videa ---------------------------------------------
        vol_row = QHBoxLayout()
        vol_row.setSpacing(8)
        self.volume_label = QLabel()
        vol_row.addWidget(self.volume_label)
        self.volume_slider = QSlider(Qt.Horizontal)
        self.volume_slider.setRange(0, 100)
        self.volume_slider.setValue(30)
        self.volume_slider.valueChanged.connect(self._on_volume_changed)
        vol_row.addWidget(self.volume_slider, 1)
        layout.addLayout(vol_row)

        self.apply_btn = QPushButton()
        self.apply_btn.clicked.connect(self.apply_wallpaper)
        layout.addWidget(self.apply_btn)

        self.measure_btn = QPushButton()
        self.measure_btn.setObjectName("secondary")
        self.measure_btn.clicked.connect(self.remeasure_screen)
        layout.addWidget(self.measure_btn)

        self.stop_btn = QPushButton()
        self.stop_btn.setObjectName("secondary")
        self.stop_btn.clicked.connect(self.stop_wallpaper)
        layout.addWidget(self.stop_btn)

        layout.addStretch()

        self.status_label = QLabel()
        self.status_label.setObjectName("status")
        self.status_label.setWordWrap(True)
        layout.addWidget(self.status_label)

        self._init_tray()
        self._load_config()
        self.apply_theme(self.theme, save=False)
        self.retranslate()

    # -- tray ---------------------------------------------------------
    def _make_app_icon(self) -> QIcon:
        # Primarne ikona ze souboru assets/icon.png (monitor s play).
        try:
            p = _asset_path("icon.png")
            if os.path.exists(p):
                icon = QIcon(p)
                if not icon.isNull():
                    return icon
        except Exception:
            pass
        # Fallback: jednoducha fialova ikona programove, at tray nikdy
        # neni bez ikony (na Windows fromTheme vzdy vrati null).
        try:
            pix = QPixmap(64, 64)
            pix.fill(Qt.transparent)
            from PySide6.QtGui import QColor, QFont, QPainter

            p = QPainter(pix)
            p.setRenderHint(QPainter.Antialiasing)
            p.setBrush(QColor(ACCENT))
            p.setPen(Qt.NoPen)
            p.drawRoundedRect(4, 4, 56, 56, 14, 14)
            p.setPen(QColor("#ffffff"))
            p.setFont(QFont("Segoe UI", 30, QFont.Bold))
            p.drawText(pix.rect(), Qt.AlignCenter, "W")
            p.end()
            return QIcon(pix)
        except Exception:
            try:
                from PySide6.QtWidgets import QStyle

                return self.style().standardIcon(QStyle.StandardPixmap.SP_ComputerIcon)
            except Exception:
                return QIcon()

    def _init_tray(self):
        app_icon = self._make_app_icon()
        try:
            self.setWindowIcon(app_icon)
        except Exception:
            pass
        self.tray = QSystemTrayIcon(self)
        self.tray.setIcon(app_icon)
        menu = QMenu()
        self.tray_show_action = QAction("Otevřít", self)
        self.tray_show_action.triggered.connect(self.showNormal)
        self.tray_quit_action = QAction("Ukončit", self)
        self.tray_quit_action.triggered.connect(self.quit_app)
        menu.addAction(self.tray_show_action)
        menu.addAction(self.tray_quit_action)
        self.tray.setContextMenu(menu)
        self.tray.activated.connect(
            lambda reason: self.showNormal() if reason == QSystemTrayIcon.ActivationReason.DoubleClick else None
        )
        self.tray.show()

    def S(self) -> dict:
        return STRINGS.get(self.lang, STRINGS["cs"])

    # -- config ---------------------------------------------------------
    def _load_config(self):
        if os.path.exists(CONFIG_PATH):
            try:
                with open(CONFIG_PATH, encoding="utf-8") as f:
                    cfg = json.load(f)
                lang = cfg.get("lang", "cs")
                if lang in STRINGS:
                    self.lang = lang
                theme = cfg.get("theme", "dark")
                if theme in THEMES:
                    self.theme = theme
                try:
                    self.mute_checkbox.setChecked(bool(cfg.get("muted", True)))
                except Exception:
                    pass
                try:
                    self.volume_slider.blockSignals(True)
                    self.volume_slider.setValue(int(cfg.get("volume", 30)))
                    self.volume_slider.blockSignals(False)
                except Exception:
                    pass
                path = cfg.get("last_path")
                if path and os.path.exists(path):
                    self.selected_path = path
                    self.drop_zone.set_file(path)
            except Exception:
                pass
        # combo boxy nastavit bez vyvolani signalu
        try:
            self.lang_combo.blockSignals(True)
            self.lang_combo.setCurrentIndex(list(LANGS).index(self.lang))
            self.lang_combo.blockSignals(False)
            self.theme_combo.blockSignals(True)
            self.theme_combo.setCurrentIndex(0 if self.theme == "dark" else 1)
            self.theme_combo.blockSignals(False)
        except Exception:
            pass

    def _save_config(self):
        try:
            with open(CONFIG_PATH, "w", encoding="utf-8") as f:
                json.dump({
                    "last_path": self.selected_path,
                    "lang": self.lang,
                    "theme": self.theme,
                    "muted": self.mute_checkbox.isChecked(),
                    "volume": self.volume_slider.value(),
                }, f)
        except Exception:
            pass

    def _on_mute_toggled(self, checked: bool):
        """F4: ulozit volbu a okamzite prepnpout zvuk bezici tapety."""
        self._save_config()
        try:
            if self.video_window is not None:
                self.video_window.set_muted(bool(checked))
        except Exception:
            pass

    def _on_volume_changed(self, value: int):
        """Ulozit hlasitost a okamzite ji nastavit bezici tapete."""
        self._save_config()
        try:
            if self.video_window is not None:
                self.video_window.set_volume(float(value) / 100.0)
        except Exception:
            pass

    # -- motiv + jazyk ---------------------------------------------------------
    def apply_theme(self, theme: str, save: bool = True):
        if theme not in THEMES:
            theme = "dark"
        self.theme = theme
        CURRENT_THEME.clear()
        CURRENT_THEME.update(THEMES[theme])
        try:
            QApplication.instance().setStyleSheet(build_stylesheet(theme))
        except Exception:
            pass
        try:
            self.drop_zone.refresh_style()
        except Exception:
            pass
        if save:
            self._save_config()

    def _on_theme_changed(self, _index: int):
        theme = self.theme_combo.currentData() or "dark"
        self.apply_theme(theme)
        self.retranslate()

    def _on_lang_changed(self, _index: int):
        lang = self.lang_combo.currentData() or "cs"
        if lang not in STRINGS:
            lang = "cs"
        self.lang = lang
        self._save_config()
        self.retranslate()

    def retranslate(self):
        s = self.S()
        self.subtitle_label.setText(s["subtitle"])
        self.lang_label.setText(s["lang_label"])
        self.theme_label.setText(s["theme_label"])
        # texty v comboboxech motivu
        try:
            self.theme_combo.blockSignals(True)
            self.theme_combo.setItemText(0, s["theme_dark"])
            self.theme_combo.setItemText(1, s["theme_light"])
            self.theme_combo.blockSignals(False)
        except Exception:
            pass
        self.drop_zone.set_hint(s["drop_hint"])
        self.mute_checkbox.setText(s["mute"])
        self.volume_label.setText(s["volume_label"])
        self.apply_btn.setText(s["apply"])
        self.measure_btn.setText(s["measure"])
        self.stop_btn.setText(s["stop"])
        self.yt_input.setPlaceholderText(s["yt_placeholder"])
        self.yt_button.setText(s["yt_button"])
        self.folder_button.setText(s["open_folder"])
        if not self.status_label.text():
            self.status_label.setText(s["ready"])
        try:
            self.tray_show_action.setText(s["open_tray"])
            self.tray_quit_action.setText(s["quit_tray"])
        except Exception:
            pass
        self._refresh_screen_label()

    # -- YouTube ---------------------------------------------------------
    def download_youtube(self):
        s = self.S()
        url = self.yt_input.text().strip()
        if not url:
            self.status_label.setText(s["warn_nofile_m"])
            return
        # F1: URL overit driv, nez se preda yt-dlp.
        if not is_valid_youtube_url(url):
            self.status_label.setText(
                s["yt_error"].format(e=s["yt_invalid_url"])
            )
            return
        if self.yt_worker is not None and self.yt_worker.isRunning():
            return
        self.yt_button.setEnabled(False)
        self.status_label.setText(s["yt_downloading"].format(p="0%"))
        debug_log(f"YT: stahuji {url}")
        self.yt_worker = DownloadWorker(url, self)
        self.yt_worker.progress.connect(self._on_yt_progress)
        self.yt_worker.finished.connect(self._on_yt_finished)
        self.yt_worker.error.connect(self._on_yt_error)
        self.yt_worker.finished.connect(lambda _p: self.yt_button.setEnabled(True))
        self.yt_worker.error.connect(lambda _e: self.yt_button.setEnabled(True))
        self.yt_worker.start()

    def _on_yt_progress(self, pct: str):
        self.status_label.setText(self.S()["yt_downloading"].format(p=pct))

    def _on_yt_finished(self, path: str):
        s = self.S()
        debug_log(f"YT: stazeno {path}")
        self.status_label.setText(s["yt_done"])
        self._on_file_chosen(path)
        # po stazeni rovnou nastavit jako tapetu
        self.apply_wallpaper()

    def _on_yt_error(self, err: str):
        debug_log(f"YT CHYBA: {err}")
        if err == "NEED_FFMPEG":
            err = self.S()["yt_need_ffmpeg"]
        self.status_label.setText(self.S()["yt_error"].format(e=err))

    def open_downloads_folder(self):
        """Otevre slozku se stazenymi videi v Pruzkumniku."""
        try:
            os.makedirs(YT_DIR, exist_ok=True)
        except Exception:
            pass
        try:
            os.startfile(YT_DIR)  # Windows
        except Exception:
            try:
                from PySide6.QtCore import QDesktopServices
                QDesktopServices.openUrl(QUrl.fromLocalFile(YT_DIR))
            except Exception:
                pass

    # -- obrazovka ---------------------------------------------------------
    def _refresh_screen_label(self):
        s = self.S()
        screens_info = self.screen_info.get("screens", [])
        pw, ph = self.screen_info.get("primary", (0, 0))
        if not screens_info:
            self.screen_label.setText(s["screen_unknown"])
            return
        if len(screens_info) == 1:
            self.screen_label.setText(s["screen_one"].format(w=pw, h=ph))
        else:
            parts = " + ".join(
                f"{x['physical_width']}×{x['physical_height']}" for x in screens_info
            )
            self.screen_label.setText(
                s["screen_multi"].format(n=len(screens_info), parts=parts, w=pw, h=ph)
            )

    def remeasure_screen(self):
        self.screen_info = screens.measure_screens()
        self._refresh_screen_label()
        pw, ph = self.screen_info.get("primary", (0, 0))
        self.status_label.setText(self.S()["measured"].format(w=pw, h=ph))

    # -- UI actions ---------------------------------------------------------
    def browse_file(self):
        s = self.S()
        filters = (
            "Obrázky a videa / Images & videos "
            "(*.jpg *.jpeg *.png *.bmp *.gif *.mp4 *.avi *.mkv *.mov *.wmv *.webm)"
        )
        path, _ = QFileDialog.getOpenFileName(self, s["apply"], "", filters)
        if path:
            self._on_file_chosen(path)

    def _on_file_chosen(self, path: str):
        s = self.S()
        ext = os.path.splitext(path)[1].lower()
        if ext not in IMAGE_EXTS and ext not in VIDEO_EXTS:
            QMessageBox.warning(self, s["warn_unsupported_t"], s["warn_unsupported_m"])
            return
        self.selected_path = path
        self.drop_zone.set_file(path)
        self.status_label.setText(s["file_selected"])

    def apply_wallpaper(self):
        s = self.S()
        if not self.selected_path:
            QMessageBox.warning(self, s["warn_nofile_t"], s["warn_nofile_m"])
            return

        if self.video_window is not None:
            self.video_window.stop()
            self.video_window = None

        ext = os.path.splitext(self.selected_path)[1].lower()

        # Pozadi se vzdy prizpusobi zmerenemu rozmeru obrazovky.
        self.screen_info = screens.measure_screens()
        self._refresh_screen_label()
        pw, ph = self.screen_info.get("primary", (0, 0))
        debug_log(f"APPLY: path={self.selected_path} ext={ext} screen={pw}x{ph}")

        if ext in IMAGE_EXTS:
            fitted = fit_image_to_screen(self.selected_path, pw, ph)
            set_static_wallpaper(fitted)
            self.status_label.setText(s["img_set"].format(w=pw, h=ph))
        elif ext in VIDEO_EXTS:
            self.video_window = VideoWallpaperWindow(
                self.selected_path, muted=self.mute_checkbox.isChecked(),
                volume=self.volume_slider.value() / 100.0,
            )
            self.video_window.failed.connect(self._on_video_failed)
            if self.video_window.start():
                self.status_label.setText(s["vid_running"].format(w=pw, h=ph))
                self.tray.showMessage(
                    s["app_name"], s["vid_started_msg"],
                    QSystemTrayIcon.MessageIcon.Information, 3000
                )
            else:
                self.video_window.stop()
                self.video_window = None
                self.status_label.setText(s["vid_fail_m"].format(log=DEBUG_LOG))
                QMessageBox.warning(
                    self, s["vid_fail_t"],
                    s["vid_fail_m"].format(log=DEBUG_LOG),
                )
                return
        else:
            return

        self._save_config()

    def _on_video_failed(self, reason: str):
        s = self.S()
        debug_log(f"VIDEO FAILED: {reason}")
        if self.video_window is not None:
            try:
                self.video_window.failed.disconnect(self._on_video_failed)
            except Exception:
                pass
            self.video_window = None
        if reason == "decode":
            self.status_label.setText(s["vid_decode_m"])
            QMessageBox.warning(self, s["vid_decode_t"], s["vid_decode_m"])
        else:
            self.status_label.setText(s["vid_fail_m"].format(log=DEBUG_LOG))

    def stop_wallpaper(self):
        if self.video_window is not None:
            self.video_window.stop()
            self.video_window = None
        if self.original_wallpaper:
            set_static_wallpaper(self.original_wallpaper)
        self.status_label.setText(self.S()["restored"])

    def closeEvent(self, event):
        s = self.S()
        event.ignore()
        self.hide()
        self.tray.showMessage(
            s["app_name"], s["hidden_tray_msg"],
            QSystemTrayIcon.MessageIcon.Information, 2000
        )

    def quit_app(self):
        if self.video_window is not None:
            self.video_window.stop()
        QApplication.quit()


def main():
    # Qt 6 si DPI awareness (Per-Monitor V2) nastavuje samo.
    # Rucni SetProcessDpiAwareness by hazelo chybu "Pristup byl odepren",
    # tak ho tu schvalne NEvolame.
    # Ztiseni ukecanych FFmpeg logu (Input #0, MFT, ...). Nejsou to chyby,
    # jen info o dekodovani, tak je skryjeme, at nezasvinuji konzoli.
    # Qt kategorie (pres QT_LOGGING_RULES) + nativni av_log level (primo ve FFmpegu).
    os.environ.setdefault("QT_LOGGING_RULES", "qt.multimedia.ffmpeg=false")
    try:
        QLoggingCategory.setFilterRules("qt.multimedia.ffmpeg=false")
    except Exception:
        pass
    _quiet_ffmpeg()
    try:
        with open(DEBUG_LOG, "w", encoding="utf-8") as f:
            f.write("=== Live Wallpaper start ===\n")
    except Exception:
        pass
    debug_log("APP start")
    app = QApplication(sys.argv)
    app.setQuitOnLastWindowClosed(False)
    app.setStyleSheet(build_stylesheet("dark"))
    win = MainWindow()
    win.show()
    sys.exit(app.exec())
