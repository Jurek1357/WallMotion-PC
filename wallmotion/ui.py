"""User interface: DropZone, main window, entry-point main()."""

from __future__ import annotations

import json
import math
import os
import sys

from PySide6.QtCore import (
    QLoggingCategory,
    QObject,
    Qt,
    QTimer,
    QUrl,
    Signal,
)
from PySide6.QtGui import (
    QAction,
    QColor,
    QDragEnterEvent,
    QDropEvent,
    QIcon,
    QPainter,
    QPen,
    QPixmap,
)
from PySide6.QtWidgets import (
    QApplication,
    QCheckBox,
    QComboBox,
    QDialog,
    QDialogButtonBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QListWidget,
    QListWidgetItem,
    QMainWindow,
    QMenu,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QScrollArea,
    QStyle,
    QSystemTrayIcon,
    QTabWidget,
    QVBoxLayout,
    QWidget,
)

from wallmotion import cli, instance, screens, volumememory
from wallmotion.autostart import is_enabled as autostart_is_enabled
from wallmotion.autostart import set_enabled as autostart_set_enabled
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
from wallmotion.linux_video import LinuxVideoWallpaper
from wallmotion.platform import get_backend
from wallmotion.rotation import INTERVALS, RotationQueue, format_interval
from wallmotion.slider import PinSlider
from wallmotion.systemtheme import read_system_theme
from wallmotion.updatecheck import (
    UpdateCheckWorker,
    is_newer,
    should_auto_check,
)
from wallmotion.utils import (
    DEBUG_LOG,
    _asset_path,
    _quiet_ffmpeg,
    app_version,
    debug_log,
)
from wallmotion.video import VideoWallpaperWindow
from wallmotion.wallpaper import (
    IMAGE_EXTS,
    VIDEO_EXTS,
    fit_image_to_screen,
    get_current_wallpaper,
    set_static_wallpaper,
)
from wallmotion.webui import WebLibraryServer
from wallmotion.youtube import (
    YT_DIR,
    DownloadWorker,
    PlaylistFetchWorker,
    is_playlist_url,
    is_valid_youtube_url,
    parse_progress_percent,
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
        """Native Windows-style system icon instead of emoji (QLabel with pixmap)."""
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


class _WebBridge(QObject):
    """Relays web-library actions to the GUI thread via signals."""

    apply_requested = Signal(str)
    stop_requested = Signal()


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Live Wallpaper")
        self.setMinimumSize(560, 620)
        self.resize(700, 800)

        self.video_window = None
        self.mirror_windows = []  # duplicate playback on other monitors
        self.user_paused = False  # manual Pause button state
        self.selected_path = None
        try:
            self.original_wallpaper = get_current_wallpaper()
        except Exception:
            self.original_wallpaper = ""  # non-Windows: no wallpaper to restore
        self.screen_info = screens.measure_screens()
        self.monitors = []
        self.monitor_choice = "all"  # "all" or physical monitor index
        self._detect_monitors()
        # Platform backend (Linux: session renderer; None = unsupported).
        self._is_windows = sys.platform == "win32"
        self._linux_backend = None
        if not self._is_windows:
            try:
                self._linux_backend = get_backend()
            except Exception:
                self._linux_backend = None
            try:
                from wallmotion.platform.linux import describe_session
                debug_log(f"PLATFORM: {describe_session()} "
                          f"backend={getattr(self._linux_backend, 'name', None)}")
            except Exception:
                pass
        self.lang = "cs"
        self.theme = "dark"
        self.yt_worker = None
        self.pl_fetch_worker = None
        self._pl_queue = []
        self._pl_active = False
        self._pl_current = None
        self.update_worker = None
        self._update_last_check = 0.0
        self._update_last_seen = ""
        self._volumes = {}
        self.favorites = set()
        self.theme_follow_system = False
        self.theme_poll_timer = QTimer(self)
        self.theme_poll_timer.timeout.connect(self._poll_system_theme)
        self.web_server = None
        self._web_bridge = None
        self.rotation = RotationQueue()
        self.rotation_enabled = False
        self.rotation_interval = INTERVALS[1]
        self.rotation_timer = QTimer(self)
        self.rotation_timer.timeout.connect(self._on_rotation_timeout)

        content = QWidget()
        content.setObjectName("content")
        layout = QVBoxLayout(content)
        layout.setContentsMargins(24, 20, 24, 20)
        layout.setSpacing(10)

        # -- title row with language + theme toggle buttons --------------
        title_row = QHBoxLayout()
        title_row.setSpacing(8)
        self.title_label = QLabel("Live Wallpaper")
        self.title_label.setObjectName("title")
        title_row.addWidget(self.title_label)
        title_row.addStretch(1)
        self.lang_button = QPushButton()
        self.lang_button.setObjectName("secondary")
        self.lang_button.setFixedWidth(52)
        self.lang_button.clicked.connect(self._toggle_lang)
        title_row.addWidget(self.lang_button)
        self.theme_button = QPushButton()
        self.theme_button.setObjectName("secondary")
        self.theme_button.setFixedSize(42, 30)
        self.theme_button.clicked.connect(self._toggle_theme)
        title_row.addWidget(self.theme_button)
        layout.addLayout(title_row)

        self.subtitle_label = QLabel()
        self.subtitle_label.setObjectName("subtitle")
        layout.addWidget(self.subtitle_label)

        self.screen_label = QLabel()
        self.screen_label.setObjectName("status")
        layout.addWidget(self.screen_label)

        # -- monitor selection ------------------------------------------------
        monitor_row = QHBoxLayout()
        monitor_row.setSpacing(8)
        self.monitor_label = QLabel()
        monitor_row.addWidget(self.monitor_label)
        self.monitor_combo = QComboBox()
        self.monitor_combo.currentIndexChanged.connect(self._on_monitor_changed)
        monitor_row.addWidget(self.monitor_combo, 1)
        layout.addLayout(monitor_row)

        self.drop_zone = DropZone()
        self.drop_zone.file_dropped.connect(self._on_file_chosen)
        self.drop_zone.clicked.connect(self.browse_file)
        layout.addWidget(self.drop_zone)

        # -- YouTube link -------------------------------------------------
        yt_row = QHBoxLayout()
        yt_row.setSpacing(8)
        self.yt_input = QLineEdit()
        yt_row.addWidget(self.yt_input, 1)
        self.yt_button = QPushButton()
        self.yt_button.setObjectName("secondary")
        self.yt_button.clicked.connect(self.download_youtube)
        yt_row.addWidget(self.yt_button)
        layout.addLayout(yt_row)

        self.yt_progress = QProgressBar()
        self.yt_progress.setRange(0, 100)
        self.yt_progress.setValue(0)
        self.yt_progress.setTextVisible(False)
        self.yt_progress.setFixedHeight(6)
        self.yt_progress_row = QWidget()
        progress_layout = QHBoxLayout(self.yt_progress_row)
        progress_layout.setContentsMargins(0, 2, 0, 2)
        progress_layout.setSpacing(8)
        progress_layout.addWidget(self.yt_progress, 1)
        self.yt_percent = QLabel("0%")
        self.yt_percent.setObjectName("status")
        self.yt_percent.setFixedWidth(48)
        self.yt_percent.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        progress_layout.addWidget(self.yt_percent)
        self.yt_progress_row.setVisible(False)
        layout.addWidget(self.yt_progress_row)

        # -- downloaded videos folder -----------------------------------
        self.folder_button = QPushButton()
        self.folder_button.setObjectName("secondary")
        self.folder_button.clicked.connect(self.open_downloads_folder)
        layout.addWidget(self.folder_button)

        self.library_button = QPushButton()
        self.library_button.setObjectName("secondary")
        self.library_button.clicked.connect(self.open_library)
        layout.addWidget(self.library_button)

        self.mute_checkbox = QCheckBox()
        self.mute_checkbox.setChecked(True)
        self.mute_checkbox.toggled.connect(self._on_mute_toggled)
        layout.addWidget(self.mute_checkbox)

        self.pause_fs_checkbox = QCheckBox()
        self.pause_fs_checkbox.setChecked(True)
        self.pause_fs_checkbox.toggled.connect(self._on_autopause_toggled)
        layout.addWidget(self.pause_fs_checkbox)

        self.pause_batt_checkbox = QCheckBox()
        self.pause_batt_checkbox.setChecked(False)
        self.pause_batt_checkbox.toggled.connect(self._on_autopause_toggled)
        layout.addWidget(self.pause_batt_checkbox)

        # -- video volume ---------------------------------------------
        vol_row = QHBoxLayout()
        vol_row.setSpacing(8)
        self.volume_label = QLabel()
        vol_row.addWidget(self.volume_label)
        self.volume_slider = PinSlider(Qt.Orientation.Horizontal)
        self.volume_slider.setRange(0, 100)
        self.volume_slider.setValue(30)
        self.volume_slider.valueChanged.connect(self._on_volume_changed)
        vol_row.addWidget(self.volume_slider, 1)
        self.volume_value = QLabel("30%")
        self.volume_value.setFixedWidth(42)
        self.volume_value.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        vol_row.addWidget(self.volume_value)
        layout.addLayout(vol_row)

        # -- autostart + system theme --------------------------------------
        sys_row = QHBoxLayout()
        sys_row.setSpacing(8)
        self.autostart_checkbox = QCheckBox()
        self.autostart_checkbox.setChecked(False)
        self.autostart_checkbox.toggled.connect(self._on_autostart_toggled)
        sys_row.addWidget(self.autostart_checkbox)
        self.theme_follow_checkbox = QCheckBox()
        self.theme_follow_checkbox.setChecked(False)
        self.theme_follow_checkbox.toggled.connect(
            self._on_theme_follow_toggled)
        sys_row.addWidget(self.theme_follow_checkbox)
        sys_row.addStretch(1)
        layout.addLayout(sys_row)

        # -- rotace tapet ------------------------------------------------
        rot_row = QHBoxLayout()
        rot_row.setSpacing(8)
        self.rotation_checkbox = QCheckBox()
        self.rotation_checkbox.setChecked(False)
        self.rotation_checkbox.toggled.connect(self._on_rotation_toggled)
        rot_row.addWidget(self.rotation_checkbox)
        self.rotation_interval_label = QLabel()
        rot_row.addWidget(self.rotation_interval_label)
        self.rotation_interval_combo = QComboBox()
        for seconds in INTERVALS:
            self.rotation_interval_combo.addItem(
                format_interval(seconds), seconds)
        self.rotation_interval_combo.setCurrentIndex(1)
        self.rotation_interval_combo.currentIndexChanged.connect(
            self._on_rotation_settings_changed)
        rot_row.addWidget(self.rotation_interval_combo, 1)
        self.rotation_shuffle_checkbox = QCheckBox()
        self.rotation_shuffle_checkbox.setChecked(False)
        self.rotation_shuffle_checkbox.toggled.connect(
            self._on_rotation_settings_changed)
        rot_row.addWidget(self.rotation_shuffle_checkbox)
        self.rotation_repeat_checkbox = QCheckBox()
        self.rotation_repeat_checkbox.setChecked(True)
        self.rotation_repeat_checkbox.toggled.connect(
            self._on_rotation_settings_changed)
        rot_row.addWidget(self.rotation_repeat_checkbox)
        layout.addLayout(rot_row)

        rot_btn_row = QHBoxLayout()
        rot_btn_row.setSpacing(8)
        self.rotation_add_btn = QPushButton()
        self.rotation_add_btn.setObjectName("secondary")
        self.rotation_add_btn.clicked.connect(self._on_rotation_add)
        rot_btn_row.addWidget(self.rotation_add_btn, 1)
        self.rotation_play_btn = QPushButton()
        self.rotation_play_btn.clicked.connect(self._on_rotation_play)
        rot_btn_row.addWidget(self.rotation_play_btn, 1)
        self.rotation_skip_btn = QPushButton()
        self.rotation_skip_btn.setObjectName("secondary")
        self.rotation_skip_btn.clicked.connect(self._on_rotation_skip)
        rot_btn_row.addWidget(self.rotation_skip_btn, 1)
        self.rotation_clear_btn = QPushButton()
        self.rotation_clear_btn.setObjectName("secondary")
        self.rotation_clear_btn.clicked.connect(self._on_rotation_clear)
        rot_btn_row.addWidget(self.rotation_clear_btn, 1)
        self.rotation_count_label = QLabel()
        self.rotation_count_label.setObjectName("status")
        rot_btn_row.addWidget(self.rotation_count_label)
        layout.addLayout(rot_btn_row)

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

        self.pause_btn = QPushButton()
        self.pause_btn.setObjectName("secondary")
        self.pause_btn.clicked.connect(self._toggle_user_pause)
        layout.addWidget(self.pause_btn)

        layout.addStretch()

        self.status_label = QLabel()
        self.status_label.setObjectName("status")
        self.status_label.setWordWrap(True)
        layout.addWidget(self.status_label)

        # Tabs: settings + wallpaper library (Wallpaper-Engine style).
        from wallmotion.library import LibraryPanel
        from wallmotion.webui import media_dir, thumbs_dir
        self.library_panel = LibraryPanel(self.S(), media_dir(), thumbs_dir())
        self.library_panel.file_chosen.connect(self._on_library_apply)
        self.library_panel.add_rotation_requested.connect(
            self._on_library_add_rotation)
        self.library_panel.delete_requested.connect(self._on_library_delete)
        self.library_panel.favorites = set(self.favorites)
        self.library_panel.on_favorites_changed = self._on_favorites_changed
        self.tabs = QTabWidget()
        self.tabs.addTab(content, "")
        self.tabs.addTab(self.library_panel, "")

        # Scrollable content: the window is resizable and nothing ever
        # overlaps or gets cut off, whatever the font scaling is.
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        scroll.setWidget(self.tabs)
        self.setCentralWidget(scroll)

        self._init_tray()
        self._load_config()
        self.apply_theme(self.theme, save=False)
        self.retranslate()

    # -- tray ---------------------------------------------------------
    def _make_app_icon(self) -> QIcon:
        # Prefer icon from assets/icon.png file (monitor with play).
        try:
            p = _asset_path("icon.png")
            if os.path.exists(p):
                icon = QIcon(p)
                if not icon.isNull():
                    return icon
        except Exception:
            pass
        # Fallback: simple purple icon drawn programmatically so the tray
        # is never without an icon (fromTheme always returns null on Windows).
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
        self.tray_update_action = QAction("Zkontrolovat aktualizace", self)
        self.tray_update_action.triggered.connect(
            lambda: self._check_updates(force=True))
        self.tray_library_action = QAction("Otevřít knihovnu", self)
        self.tray_library_action.triggered.connect(self.open_library)
        self.tray_pause_action = QAction("Pozastavit", self)
        self.tray_pause_action.triggered.connect(self._toggle_user_pause)
        self.tray_quit_action = QAction("Ukončit", self)
        self.tray_quit_action.triggered.connect(self.quit_app)
        menu.addAction(self.tray_show_action)
        menu.addAction(self.tray_library_action)
        menu.addAction(self.tray_pause_action)
        menu.addAction(self.tray_update_action)
        menu.addAction(self.tray_quit_action)
        self.tray.setContextMenu(menu)
        self.tray.activated.connect(
            lambda reason: self.showNormal() if reason == QSystemTrayIcon.ActivationReason.DoubleClick else None
        )
        self.tray.show()

    def _theme_icon(self, dark: bool) -> QIcon:
        """Painted toggle icon: sun when dark mode is on, moon when light."""
        try:
            size = 22
            pix = QPixmap(size, size)
            pix.fill(Qt.transparent)
            p = QPainter(pix)
            p.setRenderHint(QPainter.Antialiasing)
            color = QColor(T("text"))
            if dark:
                p.setBrush(color)
                p.setPen(Qt.NoPen)
                p.drawEllipse(7, 7, 8, 8)
                pen = QPen(color)
                pen.setWidth(2)
                pen.setCapStyle(Qt.RoundCap)
                p.setPen(pen)
                for deg in range(0, 360, 45):
                    rad = math.radians(deg)
                    x1 = 11 + 6 * math.cos(rad)
                    y1 = 11 + 6 * math.sin(rad)
                    x2 = 11 + 9 * math.cos(rad)
                    y2 = 11 + 9 * math.sin(rad)
                    p.drawLine(int(x1), int(y1), int(x2), int(y2))
            else:
                p.setBrush(color)
                p.setPen(Qt.NoPen)
                p.drawEllipse(4, 3, 14, 14)
                p.setCompositionMode(QPainter.CompositionMode_Clear)
                p.drawEllipse(9, 0, 13, 13)
            p.end()
            return QIcon(pix)
        except Exception:
            return QIcon()

    def _toggle_theme(self):
        self.apply_theme("light" if self.theme == "dark" else "dark")
        self.retranslate()

    def _toggle_lang(self):
        self.lang = "en" if self.lang == "cs" else "cs"
        self._save_config()
        self.retranslate()

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
                    self.pause_fs_checkbox.setChecked(
                        bool(cfg.get("pause_fullscreen", True))
                    )
                    self.pause_batt_checkbox.setChecked(
                        bool(cfg.get("pause_battery", False))
                    )
                except Exception:
                    pass
                try:
                    mon = cfg.get("monitor", "all")
                    if mon != "all":
                        mon = int(mon)
                    self.monitor_choice = mon
                except Exception:
                    self.monitor_choice = "all"
                try:
                    self._update_last_check = float(cfg.get("update_last_check", 0.0))
                    self._update_last_seen = str(cfg.get("update_last_seen", ""))
                except Exception:
                    pass
                try:
                    volumes = cfg.get("volumes", {})
                    self._volumes = dict(volumes) if isinstance(volumes, dict) else {}
                except Exception:
                    self._volumes = {}
                try:
                    rot = cfg.get("rotation", {})
                    self.rotation = RotationQueue.from_config(rot)
                    self.rotation.prune_missing()
                    self.rotation_enabled = bool(rot.get("enabled", False))
                    interval = int(rot.get("interval", INTERVALS[1]))
                    if interval in INTERVALS:
                        self.rotation_interval = interval
                    self.rotation_checkbox.blockSignals(True)
                    self.rotation_checkbox.setChecked(self.rotation_enabled)
                    self.rotation_checkbox.blockSignals(False)
                    self.rotation_shuffle_checkbox.blockSignals(True)
                    self.rotation_shuffle_checkbox.setChecked(self.rotation.shuffle)
                    self.rotation_shuffle_checkbox.blockSignals(False)
                    self.rotation_repeat_checkbox.blockSignals(True)
                    self.rotation_repeat_checkbox.setChecked(self.rotation.repeat)
                    self.rotation_repeat_checkbox.blockSignals(False)
                    idx = list(INTERVALS).index(self.rotation_interval)
                    self.rotation_interval_combo.blockSignals(True)
                    self.rotation_interval_combo.setCurrentIndex(idx)
                    self.rotation_interval_combo.blockSignals(False)
                except Exception:
                    pass
                try:
                    self.volume_slider.blockSignals(True)
                    self.volume_slider.setValue(int(cfg.get("volume", 30)))
                    self.volume_slider.blockSignals(False)
                    self.volume_value.setText(f"{self.volume_slider.value()}%")
                except Exception:
                    pass
                path = cfg.get("last_path")
                if path and os.path.exists(path):
                    self.selected_path = path
                    self.drop_zone.set_file(path)
                if self.selected_path:
                    self._apply_volume_memory(self.selected_path)
                try:
                    favs = cfg.get("favorites", [])
                    self.favorites = {str(p) for p in favs if p}
                except Exception:
                    self.favorites = set()
                try:
                    self.theme_follow_system = bool(
                        cfg.get("theme_follow_system", False))
                except Exception:
                    self.theme_follow_system = False
            except Exception:
                pass
        try:
            self.theme_follow_checkbox.blockSignals(True)
            self.theme_follow_checkbox.setChecked(self.theme_follow_system)
            self.theme_follow_checkbox.blockSignals(False)
            self.theme_button.setEnabled(not self.theme_follow_system)
        except Exception:
            pass
        try:
            supported = sys.platform == "win32" or sys.platform.startswith("linux")
            self.autostart_checkbox.setVisible(supported)
            if supported:
                self.autostart_checkbox.blockSignals(True)
                self.autostart_checkbox.setChecked(autostart_is_enabled())
                self.autostart_checkbox.blockSignals(False)
        except Exception:
            pass
        try:
            self.library_panel.favorites = set(self.favorites)
            self.library_panel.refresh()
        except Exception:
            pass
        if self.theme_follow_system:
            try:
                self.theme_poll_timer.start(15000)
            except Exception:
                pass
        self._restart_rotation_timer()
        self._refresh_rotation_label()

    def _save_config(self):
        try:
            try:
                if self.selected_path:
                    self._volumes = volumememory.remember(
                        self._volumes, self.selected_path,
                        self.volume_slider.value(),
                        self.mute_checkbox.isChecked(),
                    )
            except Exception:
                pass
            os.makedirs(os.path.dirname(CONFIG_PATH), exist_ok=True)
            with open(CONFIG_PATH, "w", encoding="utf-8") as f:
                json.dump({
                    "last_path": self.selected_path,
                    "lang": self.lang,
                    "theme": self.theme,
                    "muted": self.mute_checkbox.isChecked(),
                    "volume": self.volume_slider.value(),
                    "pause_fullscreen": self.pause_fs_checkbox.isChecked(),
                    "pause_battery": self.pause_batt_checkbox.isChecked(),
                    "monitor": self.monitor_choice,
                    "update_last_check": self._update_last_check,
                    "update_last_seen": self._update_last_seen,
                    "volumes": self._volumes,
                    "favorites": sorted(self.favorites),
                    "theme_follow_system": self.theme_follow_system,
                    "rotation": {
                        **self.rotation.to_config(),
                        "enabled": self.rotation_enabled,
                        "interval": self.rotation_interval,
                    },
                }, f)
        except Exception:
            pass

    def _apply_volume_memory(self, path: str) -> None:
        """Restore remembered volume for a file, if any.

        The slider is recalled, but mute is never turned OFF by memory:
        a checked mute checkbox is the master switch and only the user
        may uncheck it. (Memory may still turn mute ON.)
        """
        try:
            entry = volumememory.lookup(self._volumes, path)
            if not entry:
                return
            self.volume_slider.blockSignals(True)
            self.volume_slider.setValue(entry["volume"])
            self.volume_slider.blockSignals(False)
            self.volume_value.setText(f"{self.volume_slider.value()}%")
            if entry["muted"]:
                self.mute_checkbox.blockSignals(True)
                self.mute_checkbox.setChecked(True)
                self.mute_checkbox.blockSignals(False)
        except Exception:
            pass

    def _stop_all_video_windows(self):
        """Stop the primary window and every mirror, silently."""
        try:
            if self.video_window is not None:
                self.video_window.stop()
        except Exception:
            pass
        self.video_window = None
        try:
            for w in self.mirror_windows:
                try:
                    w.stop()
                except Exception:
                    continue
        except Exception:
            pass
        self.mirror_windows = []

    def _each_video_window(self):
        """Primary window first, then mirrors (for mute/volume/rules)."""
        try:
            if self.video_window is not None:
                yield self.video_window
            yield from list(self.mirror_windows)
        except Exception:
            return

    def _on_mute_toggled(self, checked: bool):
        """F4: save the option and immediately toggle sound of the running wallpaper."""
        self._save_config()
        try:
            for w in self._each_video_window():
                # Mirrors stay muted so two monitors never echo.
                if w is self.video_window:
                    w.set_muted(bool(checked))
                else:
                    w.set_muted(True)
        except Exception:
            pass

    def _on_autopause_toggled(self, _checked: bool):
        """Save rules and apply them to the running wallpaper immediately."""
        self._save_config()
        try:
            for w in self._each_video_window():
                w.set_auto_pause(
                    self.pause_fs_checkbox.isChecked(),
                    self.pause_batt_checkbox.isChecked(),
                )
        except Exception:
            pass

    def _on_autostart_toggled(self, checked: bool):
        """Create/remove the OS autostart entry. Reverts on failure."""
        try:
            ok = autostart_set_enabled(bool(checked))
        except Exception:
            ok = False
        if not ok:
            try:
                self.autostart_checkbox.blockSignals(True)
                self.autostart_checkbox.setChecked(not checked)
                self.autostart_checkbox.blockSignals(False)
            except Exception:
                pass
            debug_log("AUTOSTART: change failed, reverted")

    def _on_theme_follow_toggled(self, checked: bool):
        """Follow the OS theme (disables the manual toggle button)."""
        self.theme_follow_system = bool(checked)
        self._save_config()
        try:
            self.theme_button.setEnabled(not self.theme_follow_system)
        except Exception:
            pass
        if self.theme_follow_system:
            try:
                system_theme = read_system_theme()
                if system_theme in ("dark", "light"):
                    self.apply_theme(system_theme)
                    self.retranslate()
                self.theme_poll_timer.start(15000)
            except Exception:
                pass
        else:
            try:
                self.theme_poll_timer.stop()
            except Exception:
                pass

    def _poll_system_theme(self):
        """Apply the OS theme when it changed (only in follow mode)."""
        try:
            if not self.theme_follow_system:
                return
            system_theme = read_system_theme()
            if system_theme in ("dark", "light") and system_theme != self.theme:
                self.apply_theme(system_theme)
                self.retranslate()
        except Exception as e:
            debug_log(f"THEME poll exception: {e!r}")

    def _on_volume_changed(self, value: int):
        """Save the volume and immediately apply it to the running wallpaper."""
        self.volume_value.setText(f"{int(value)}%")
        self._save_config()
        try:
            if self.video_window is not None:
                self.video_window.set_volume(float(value) / 100.0)
        except Exception:
            pass
    # -- theme + language ---------------------------------------------------------
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

    def retranslate(self):
        s = self.S()
        self.subtitle_label.setText(s["subtitle"])
        self.tabs.setTabText(0, s["settings_title"])
        self.tabs.setTabText(1, s["library_title"])
        try:
            self.library_panel.retranslate(s)
        except Exception:
            pass
        # Toggle buttons show the *target*: EN while Czech is on, sun
        # while dark mode is on (click switches to the other one).
        try:
            target_lang = "en" if self.lang == "cs" else "cs"
            self.lang_button.setText("EN" if target_lang == "en" else "CZ")
            self.lang_button.setToolTip(LANGS.get(target_lang, target_lang))
            self.theme_button.setIcon(self._theme_icon(self.theme == "dark"))
            self.theme_button.setToolTip(
                s["theme_light"] if self.theme == "dark" else s["theme_dark"])
        except Exception:
            pass
        self.drop_zone.set_hint(s["drop_hint"])
        self.mute_checkbox.setText(s["mute"])
        self.pause_fs_checkbox.setText(s["pause_fullscreen"])
        self.pause_batt_checkbox.setText(s["pause_battery"])
        self.monitor_label.setText(s["monitor_label"])
        self._refresh_monitor_combo()
        self.autostart_checkbox.setText(s["autostart"])
        self.theme_follow_checkbox.setText(s["theme_auto"])
        self.rotation_checkbox.setText(s["rotation_enable"])
        self.rotation_interval_label.setText(s["rotation_interval"])
        self.rotation_shuffle_checkbox.setText(s["rotation_shuffle"])
        self.rotation_repeat_checkbox.setText(s["rotation_repeat"])
        self.rotation_add_btn.setText(s["rotation_add"])
        self.rotation_play_btn.setText(s["rotation_play"])
        self.rotation_skip_btn.setText(s["rotation_skip"])
        self.rotation_clear_btn.setText(s["rotation_clear"])
        self._refresh_rotation_label()
        self.volume_label.setText(s["volume_label"])
        self.apply_btn.setText(s["apply"])
        self.measure_btn.setText(s["measure"])
        self.stop_btn.setText(s["stop"])
        self.yt_input.setPlaceholderText(s["yt_placeholder"])
        self.yt_button.setText(s["yt_button"])
        self.folder_button.setText(s["open_folder"])
        self.library_button.setText(s["tray_library"])
        if not self.status_label.text():
            self.status_label.setText(s["ready"])
        try:
            self.tray_show_action.setText(s["open_tray"])
            self.tray_library_action.setText(s["tray_library"])
            self.tray_update_action.setText(s["tray_check_update"])
            self.tray_quit_action.setText(s["quit_tray"])
        except Exception:
            pass
        self._refresh_pause_ui()
        self._refresh_screen_label()

    # -- YouTube ---------------------------------------------------------
    def download_youtube(self):
        s = self.S()
        url = self.yt_input.text().strip()
        if not url:
            self.status_label.setText(s["warn_nofile_m"])
            return
        # F1: validate the URL before passing it to yt-dlp.
        if not is_valid_youtube_url(url):
            self.status_label.setText(
                s["yt_error"].format(e=s["yt_invalid_url"])
            )
            return
        if is_playlist_url(url):
            self._fetch_playlist(url)
            return
        self._pl_active = False
        self._pl_queue = []
        self._pl_current = None
        self._start_download_worker(url)

    def _start_download_worker(self, url: str):
        """Start DownloadWorker for one video (single URL or queue item)."""
        s = self.S()
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

    def _fetch_playlist(self, url: str):
        """Load playlist entries on a background thread, then show picker."""
        if self.yt_worker is not None and self.yt_worker.isRunning():
            return
        if self.pl_fetch_worker is not None and self.pl_fetch_worker.isRunning():
            return
        self.yt_button.setEnabled(False)
        self.status_label.setText(self.S()["yt_playlist_loading"])
        debug_log(f"YT playlist: nacitam {url}")
        self.pl_fetch_worker = PlaylistFetchWorker(url, self)
        self.pl_fetch_worker.loaded.connect(self._on_playlist_loaded)
        self.pl_fetch_worker.error.connect(self._on_playlist_error)
        self.pl_fetch_worker.loaded.connect(
            lambda _e: self.yt_button.setEnabled(True)
        )
        self.pl_fetch_worker.error.connect(
            lambda _e: self.yt_button.setEnabled(True)
        )
        self.pl_fetch_worker.start()

    def _on_playlist_loaded(self, entries: list):
        s = self.S()
        debug_log(f"YT playlist: nacteno {len(entries)} polozek")
        dialog = QDialog(self)
        dialog.setWindowTitle(s["yt_playlist_title"])
        dialog.setMinimumWidth(360)
        layout = QVBoxLayout(dialog)
        list_widget = QListWidget(dialog)
        list_widget.setSelectionMode(QListWidget.MultiSelection)
        for e in entries:
            try:
                item = QListWidgetItem(str(e.get("title") or e.get("id")))
                item.setData(Qt.UserRole, e.get("url"))
                list_widget.addItem(item)
            except Exception:
                continue
        layout.addWidget(list_widget)
        buttons = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel, dialog
        )
        buttons.accepted.connect(dialog.accept)
        buttons.rejected.connect(dialog.reject)
        layout.addWidget(buttons)
        if dialog.exec() != QDialog.Accepted:
            self.status_label.setText(s["ready"])
            return
        urls = []
        for item in list_widget.selectedItems():
            try:
                u = item.data(Qt.UserRole)
                if u:
                    urls.append(u)
            except Exception:
                continue
        if not urls:
            self.status_label.setText(s["ready"])
            return
        # Sequential queue: each item keeps the 500 MB + H.264/1080p guards
        # of DownloadWorker; last downloaded video ends up as wallpaper.
        total = len(urls)
        self._pl_queue = [(i + 1, total, u) for i, u in enumerate(urls[1:])]
        self._pl_active = True
        self._pl_current = (1, total)
        self.status_label.setText(s["yt_queue_progress"].format(i=1, n=total))
        self._start_download_worker(urls[0])

    def _on_playlist_error(self, err: str):
        debug_log(f"YT playlist CHYBA: {err}")
        if err == "EMPTY_PLAYLIST":
            err = self.S()["yt_playlist_empty"]
        self.status_label.setText(self.S()["yt_error"].format(e=err))

    def _on_queue_item_done(self):
        """Start next queued playlist item, if any."""
        if not self._pl_active:
            return
        if self._pl_queue:
            i, n, url = self._pl_queue.pop(0)
            self._pl_current = (i, n)
            self.status_label.setText(
                self.S()["yt_queue_progress"].format(i=i, n=n)
            )
            self._start_download_worker(url)
        else:
            self._pl_active = False
            self._pl_current = None

    def _on_yt_progress(self, pct: str):
        if self._pl_active and self._pl_current:
            i, n = self._pl_current
            self.status_label.setText(
                self.S()["yt_queue_progress"].format(i=i, n=n)
                + " " + self.S()["yt_downloading"].format(p=pct)
            )
        else:
            self.status_label.setText(self.S()["yt_downloading"].format(p=pct))
        try:
            value = parse_progress_percent(pct)
            if value is not None:
                self.yt_progress.setValue(int(round(value)))
                self.yt_percent.setText(f"{value:.0f}%")
                if not self.yt_progress_row.isVisible():
                    self.yt_progress_row.setVisible(True)
        except Exception:
            pass

    def _hide_yt_progress(self):
        try:
            self.yt_progress.setValue(0)
            self.yt_percent.setText("0%")
            self.yt_progress_row.setVisible(False)
        except Exception:
            pass

    def _on_yt_finished(self, path: str):
        s = self.S()
        debug_log(f"YT: stazeno {path}")
        self._hide_yt_progress()
        self.status_label.setText(s["yt_done"])
        self._on_file_chosen(path)
        # set as wallpaper right after download
        self.apply_wallpaper()
        # playlist queue: continue with the next selected item
        self._on_queue_item_done()

    def _on_yt_error(self, err: str):
        debug_log(f"YT CHYBA: {err}")
        self._hide_yt_progress()
        if err == "NEED_FFMPEG":
            err = self.S()["yt_need_ffmpeg"]
        elif err == "NEED_SIGNIN":
            err = self.S()["yt_need_signin"]
        elif err == "UNAVAILABLE":
            err = self.S()["yt_unavailable"]
        elif err == "TIMEOUT":
            err = self.S()["yt_timeout"]
        self.status_label.setText(self.S()["yt_error"].format(e=err))
        # playlist queue: skip the broken video, keep going
        self._on_queue_item_done()

    def _refresh_linux_backend(self):
        """(Re-)detect the Linux session backend. None when unsupported."""
        if self._is_windows:
            return None
        if self._linux_backend is None:
            try:
                self._linux_backend = get_backend()
            except Exception:
                self._linux_backend = None
        return self._linux_backend

    def _start_linux_video(self, s, pw, ph, monitor=None):
        """Video wallpaper on Linux via the session backend."""
        from wallmotion.platform.linux import describe_session
        backend = self._refresh_linux_backend()
        if backend is None:
            self.status_label.setText(
                s["linux_no_backend"].format(s=describe_session()))
            return
        if getattr(backend, "name", "") == "gnome":
            if not self._ensure_hanabi(s):
                return
        self.video_window = LinuxVideoWallpaper(
            backend, self.selected_path, muted=self.mute_checkbox.isChecked(),
            volume=self.volume_slider.value() / 100.0,
            auto_pause_fullscreen=self.pause_fs_checkbox.isChecked(),
            auto_pause_battery=self.pause_batt_checkbox.isChecked(),
            monitor=monitor,
        )
        self.video_window.failed.connect(self._on_video_failed)
        if self.video_window.start():
            self.user_paused = False
            self.status_label.setText(s["vid_running"].format(w=pw, h=ph))
            self.tray.showMessage(
                s["app_name"], s["vid_started_msg"],
                QSystemTrayIcon.MessageIcon.Information, 3000
            )
            self._refresh_pause_ui()
        else:
            self.video_window.stop()
            self.video_window = None
            self.user_paused = False
            self._refresh_pause_ui()
            try:
                tools = ", ".join(backend.missing_tools()) or "?"
            except Exception:
                tools = "?"
            self.status_label.setText(s["linux_missing"].format(tools=tools))
            QMessageBox.warning(
                self, s["vid_fail_t"],
                s["linux_missing"].format(tools=tools),
            )

    def _ensure_hanabi(self, s) -> bool:
        """GNOME video needs the Hanabi extension. Install/enable it via
        GNOME's own mechanisms; returns True when usable right now."""
        from wallmotion.platform.linux import hanabi_enable, hanabi_install, hanabi_state
        state = hanabi_state()
        if state == "missing":
            answer = QMessageBox.question(
                self, s["linux_hanabi_install_t"], s["linux_gnome_video_m"])
            if answer != QMessageBox.StandardButton.Yes:
                self.status_label.setText(s["linux_gnome_video_t"])
                return False
            ok, res = hanabi_install()
            state = hanabi_state()
            if state == "missing":
                self.status_label.setText(
                    s["linux_hanabi_failed_m"].format(err=res.strip() or "?"))
                return False
        if state == "queued":
            # installed but never loaded by the shell - enable queues
            # it in enabled-extensions; it activates on next login.
            answer = QMessageBox.question(
                self, s["linux_hanabi_enable_t"], s["linux_hanabi_enable_m"])
            if answer != QMessageBox.StandardButton.Yes:
                return False
            hanabi_enable()
            QMessageBox.information(
                self, s["linux_gnome_video_t"], s["linux_hanabi_relogin_m"])
            self.status_label.setText(s["linux_hanabi_relogin_m"])
            return False
        if state == "installed":
            # the shell knows the extension - enabling works live.
            # set_video writes video-path BEFORE enabling so Hanabi's
            # renderer launches into playback (an empty path makes it
            # pop its preferences window).
            answer = QMessageBox.question(
                self, s["linux_hanabi_enable_t"], s["linux_hanabi_enable_m"])
            if answer != QMessageBox.StandardButton.Yes:
                return False
            return True
        if state != "enabled":
            QMessageBox.information(
                self, s["linux_gnome_video_t"], s["linux_hanabi_relogin_m"])
            self.status_label.setText(s["linux_hanabi_relogin_m"])
            return False
        return True

    def _rotation_interval_seconds(self) -> int:
        try:
            return int(self.rotation_interval_combo.currentData() or INTERVALS[1])
        except Exception:
            return INTERVALS[1]

    def _refresh_rotation_label(self):
        try:
            self.rotation_count_label.setText(
                self.S()["rotation_count"].format(n=len(self.rotation.files)))
        except Exception:
            pass

    def _restart_rotation_timer(self):
        try:
            if self.rotation_enabled and len(self.rotation.files) > 0:
                self.rotation_timer.start(self.rotation_interval * 1000)
            else:
                self.rotation_timer.stop()
        except Exception:
            pass

    def _on_rotation_toggled(self, checked: bool):
        self.rotation_enabled = bool(checked)
        self._save_config()
        self._restart_rotation_timer()

    def _on_rotation_settings_changed(self, _index=None):
        try:
            self.rotation_interval = self._rotation_interval_seconds()
            self.rotation.set_shuffle(
                self.rotation_shuffle_checkbox.isChecked())
            self.rotation.repeat = self.rotation_repeat_checkbox.isChecked()
        except Exception:
            pass
        self._save_config()
        self._restart_rotation_timer()

    def _on_rotation_add(self):
        if self.selected_path and os.path.exists(self.selected_path):
            if self.rotation.add(self.selected_path):
                self._save_config()
        self._refresh_rotation_label()

    def _on_rotation_play(self):
        """Start the list right now (from the top), then keep rotating."""
        try:
            if not self.rotation.files:
                return
            self.rotation_enabled = True
            try:
                self.rotation_checkbox.blockSignals(True)
                self.rotation_checkbox.setChecked(True)
                self.rotation_checkbox.blockSignals(False)
            except Exception:
                pass
            path = self.rotation.restart()
            if path:
                self._on_file_chosen(path)
                self.apply_wallpaper()
            self._save_config()
            self._restart_rotation_timer()
            self._refresh_rotation_label()
        except Exception as e:
            debug_log(f"ROTATION play exception: {e!r}")

    def _on_rotation_skip(self):
        """Jump to the next wallpaper in the rotation list right now."""
        try:
            if not self.rotation.files:
                return
            path = self.rotation.next_file()
            if path:
                self._on_file_chosen(path)
                self.apply_wallpaper()
                self._save_config()
        except Exception as e:
            debug_log(f"ROTATION skip exception: {e!r}")

    def _on_rotation_clear(self):
        self.rotation.clear()
        self.rotation_enabled = False
        try:
            self.rotation_checkbox.blockSignals(True)
            self.rotation_checkbox.setChecked(False)
            self.rotation_checkbox.blockSignals(False)
        except Exception:
            pass
        self._save_config()
        self._restart_rotation_timer()
        self._refresh_rotation_label()

    def _on_rotation_timeout(self):
        try:
            path = self.rotation.next_file()
            if not path:
                return
            if (not self.rotation.repeat and path == self.selected_path
                    and self.rotation.files):
                # End of a non-repeating list: keep the last wallpaper
                # and switch rotation off.
                self.rotation_enabled = False
                try:
                    self.rotation_checkbox.blockSignals(True)
                    self.rotation_checkbox.setChecked(False)
                    self.rotation_checkbox.blockSignals(False)
                except Exception:
                    pass
                self._save_config()
                self._restart_rotation_timer()
                self._refresh_rotation_label()
                return
            self._on_file_chosen(path)
            self.apply_wallpaper()
            self._save_config()
        except Exception as e:
            debug_log(f"ROTATION exception: {e!r}")

    def open_downloads_folder(self):
        """Open the downloaded videos folder in Explorer."""
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

    # -- screen ---------------------------------------------------------
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

    def _detect_monitors(self):
        """Physical monitor list: WinAPI on Windows, Qt data on Linux."""
        try:
            mons = screens.get_physical_monitors()
        except Exception:
            mons = []
        if not mons:
            try:
                mons = screens.qt_monitors_to_physical(self.screen_info)
            except Exception:
                mons = []
        self.monitors = mons

    def remeasure_screen(self):
        self.screen_info = screens.measure_screens()
        self._detect_monitors()
        self._refresh_screen_label()
        self._refresh_monitor_combo()
        pw, ph = self.screen_info.get("primary", (0, 0))
        self.status_label.setText(self.S()["measured"].format(w=pw, h=ph))

    def _refresh_monitor_combo(self):
        """Rebuild monitor selector (all + physical monitors)."""
        s = self.S()
        try:
            self.monitor_combo.blockSignals(True)
            self.monitor_combo.clear()
            self.monitor_combo.addItem(s["monitor_all"], "all")
            for m in self.monitors:
                try:
                    label = f"Monitor {m['index'] + 1} ({m['w']}x{m['h']})"
                    if m.get("primary"):
                        label += f" - {s['monitor_primary']}"
                    self.monitor_combo.addItem(label, m["index"])
                except Exception:
                    continue
            idx = 0
            if self.monitor_choice != "all":
                for i in range(self.monitor_combo.count()):
                    if self.monitor_combo.itemData(i) == self.monitor_choice:
                        idx = i
                        break
                else:
                    self.monitor_choice = "all"
            self.monitor_combo.setCurrentIndex(idx)
        except Exception:
            pass
        finally:
            try:
                self.monitor_combo.blockSignals(False)
            except Exception:
                pass

    def _on_monitor_changed(self, index: int):
        try:
            choice = self.monitor_combo.itemData(index)
            self.monitor_choice = choice if choice is not None else "all"
        except Exception:
            self.monitor_choice = "all"
        self._save_config()

    def _selected_monitor(self) -> dict | None:
        """Chosen physical monitor {x, y, w, h}, or None for all monitors."""
        if self.monitor_choice == "all":
            return None
        try:
            for m in self.monitors:
                if m.get("index") == self.monitor_choice:
                    return dict(m)
        except Exception:
            pass
        return None

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
        self._apply_volume_memory(path)
        self.status_label.setText(s["file_selected"])

    def apply_wallpaper(self):
        s = self.S()
        if not self.selected_path:
            QMessageBox.warning(self, s["warn_nofile_t"], s["warn_nofile_m"])
            return

        self._stop_all_video_windows()

        ext = os.path.splitext(self.selected_path)[1].lower()

        # Always fit the background to the measured screen size.
        self.screen_info = screens.measure_screens()
        self._detect_monitors()
        self._refresh_screen_label()
        self._refresh_monitor_combo()
        mon = self._selected_monitor()
        if mon:
            pw, ph = mon["w"], mon["h"]
        else:
            pw, ph = self.screen_info.get("primary", (0, 0))
        debug_log(f"APPLY: path={self.selected_path} ext={ext} screen={pw}x{ph}")
        try:
            debug_log("APPLY: monitors=" + ", ".join(
                f"{m.get('index')}:{m.get('x')},{m.get('y')} "
                f"{m.get('w')}x{m.get('h')}" for m in self.monitors))
        except Exception:
            pass

        if ext in IMAGE_EXTS:
            # Windows needs a pre-fitted bitmap; every Linux renderer
            # scales itself (feh --bg-fill, swww --resize crop, GNOME
            # 'zoom'), so on Linux pass the original file - fitting would
            # only duplicate work and leave a volatile BMP in /tmp.
            if self._is_windows:
                fitted = fit_image_to_screen(self.selected_path, pw, ph)
                set_static_wallpaper(fitted)
            else:
                fitted = self.selected_path
                from wallmotion.platform.linux import describe_session
                backend = self._refresh_linux_backend()
                if backend is None:
                    self.status_label.setText(
                        s["linux_no_backend"].format(s=describe_session()))
                    return
                if not backend.set_image(fitted):
                    try:
                        tools = ", ".join(backend.missing_tools()) or "?"
                    except Exception:
                        tools = "?"
                    self.status_label.setText(
                        s["linux_missing"].format(tools=tools))
                    return
            self.status_label.setText(s["img_set"].format(w=pw, h=ph))
        elif ext in VIDEO_EXTS:
            if not self._is_windows:
                self._start_linux_video(s, pw, ph, monitor=mon)
                self._save_config()
                return
            # "All monitors" on 2+ screens: same video on each screen
            # (first window carries audio, mirrors stay muted).
            targets = screens.duplicate_targets(
                self.monitor_choice, self.monitors)
            started = 0
            prev_hwnd = None
            for i, target in enumerate(targets):
                window = VideoWallpaperWindow(
                    self.selected_path,
                    muted=self.mute_checkbox.isChecked() if i == 0 else True,
                    volume=self.volume_slider.value() / 100.0,
                    auto_pause_fullscreen=self.pause_fs_checkbox.isChecked(),
                    auto_pause_battery=self.pause_batt_checkbox.isChecked(),
                    monitor=target,
                )
                if i == 0:
                    window.failed.connect(self._on_video_failed)
                    self.video_window = window
                else:
                    window.failed.connect(self._on_mirror_failed)
                if window.start(after_hwnd=prev_hwnd):
                    started += 1
                    try:
                        prev_hwnd = int(window._canvas) or None
                    except Exception:
                        prev_hwnd = None
                    if i > 0:
                        self.mirror_windows.append(window)
                    continue
                try:
                    window.stop()
                except Exception:
                    pass
                if i == 0:
                    # Primary failed: same fatal path as single-monitor.
                    self.video_window = None
                    self.user_paused = False
                    self._refresh_pause_ui()
                    self.status_label.setText(
                        s["vid_fail_m"].format(log=DEBUG_LOG))
                    QMessageBox.warning(
                        self, s["vid_fail_t"],
                        s["vid_fail_m"].format(log=DEBUG_LOG),
                    )
                    return
                debug_log(f"APPLY: mirror {i} failed to start, primary runs")
                self.status_label.setText(s["mirror_failed"].format(i=i + 1))
            if self.video_window is not None:
                self.user_paused = False
                self.status_label.setText(s["vid_running"].format(w=pw, h=ph))
                self.tray.showMessage(
                    s["app_name"], s["vid_started_msg"],
                    QSystemTrayIcon.MessageIcon.Information, 3000
                )
                self._refresh_pause_ui()
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
        try:
            for w in self.mirror_windows:
                try:
                    w.stop()
                except Exception:
                    continue
        except Exception:
            pass
        self.mirror_windows = []
        if reason == "decode":
            self.status_label.setText(s["vid_decode_m"])
            QMessageBox.warning(self, s["vid_decode_t"], s["vid_decode_m"])
        else:
            self.status_label.setText(s["vid_fail_m"].format(log=DEBUG_LOG))
        self.user_paused = False
        self._refresh_pause_ui()

    def _on_mirror_failed(self, reason: str):
        """A mirror window died: drop just it, the primary keeps playing."""
        debug_log(f"MIRROR FAILED: {reason}")
        try:
            sender = self.sender()
            if sender is not None:
                try:
                    sender.stop()
                except Exception:
                    pass
                self.mirror_windows = [
                    w for w in self.mirror_windows if w is not sender]
        except Exception:
            pass

    def _toggle_user_pause(self):
        """Manual Pause/Resume: freeze the frame, keep canvas and config."""
        try:
            if self.video_window is None:
                return
            target = not self.user_paused
            self.user_paused = target
            try:
                self.video_window.set_user_paused(target)
                for w in list(self.mirror_windows):
                    try:
                        w.set_user_paused(target)
                    except Exception:
                        continue
            except Exception:
                pass
            self._refresh_pause_ui()
        except Exception as e:
            debug_log(f"PAUSE exception: {e!r}")

    def _refresh_pause_ui(self):
        """Pause button + tray texts follow state; disabled without video."""
        try:
            s = self.S()
            running = self.video_window is not None
            text = s["video_resume"] if self.user_paused else s["video_pause"]
            self.pause_btn.setText(text)
            self.pause_btn.setEnabled(running)
            self.tray_pause_action.setText(text)
            self.tray_pause_action.setEnabled(running)
        except Exception:
            pass

    def stop_wallpaper(self):
        self.user_paused = False
        self._stop_all_video_windows()
        if self._is_windows:
            if self.original_wallpaper:
                set_static_wallpaper(self.original_wallpaper)
        else:
            try:
                if self._linux_backend is not None:
                    self._linux_backend.stop()
                    self._linux_backend.restore()
            except Exception as e:
                debug_log(f"RESTORE: backend restore error: {e!r}")
        self.status_label.setText(self.S()["restored"])
        self._refresh_pause_ui()

    def closeEvent(self, event):
        s = self.S()
        event.ignore()
        self.hide()
        self.tray.showMessage(
            s["app_name"], s["hidden_tray_msg"],
            QSystemTrayIcon.MessageIcon.Information, 2000
        )

    def _on_favorites_changed(self, path=None, added=None):
        """Panel favorites changed: merge with disk, then save.

        Merge (not overwrite) so two running instances do not wipe
        each other's stars: the toggle carries what changed.
        """
        try:
            if path:
                disk = set()
                try:
                    if os.path.exists(CONFIG_PATH):
                        with open(CONFIG_PATH, encoding="utf-8") as f:
                            disk = {str(p) for p in
                                    json.load(f).get("favorites", []) if p}
                except Exception:
                    pass
                current = disk | set(self.library_panel.favorites or set())
                if added:
                    current.add(str(path))
                else:
                    current.discard(str(path))
                self.favorites = current
                try:
                    self.library_panel.favorites = set(current)
                except Exception:
                    pass
            else:
                self.favorites = set(self.library_panel.favorites or set())
        except Exception:
            pass
        self._save_config()

    def restore_last_wallpaper(self):
        """Re-apply the last wallpaper (autostart / relaunch)."""
        try:
            if self.selected_path and os.path.exists(self.selected_path):
                debug_log(f"RESTORE: re-applying {self.selected_path}")
                self.apply_wallpaper()
        except Exception as e:
            debug_log(f"RESTORE exception: {e!r}")

    def handle_remote_command(self, cmd: dict):
        """Apply a CLI / single-instance command to the running app."""
        try:
            if not isinstance(cmd, dict) or not cmd:
                return
            debug_log(f"CMD: {sorted(cmd)}")
            if "set" in cmd:
                path = cmd["set"]
                if path and os.path.exists(path):
                    self._on_file_chosen(path)
                    self.apply_wallpaper()
            if cmd.get("stop"):
                self.stop_wallpaper()
            if "muted" in cmd:
                try:
                    self.mute_checkbox.setChecked(bool(cmd["muted"]))
                except Exception:
                    pass
            if "volume" in cmd:
                try:
                    self.volume_slider.setValue(int(cmd["volume"]))
                except Exception:
                    pass
            self.showNormal()
        except Exception as e:
            debug_log(f"CMD exception: {e!r}")

    def _schedule_update_check(self):
        """Automatic weekly update check, shortly after startup."""
        try:
            if should_auto_check(self._update_last_check):
                QTimer.singleShot(5000, lambda: self._check_updates(force=False))
        except Exception:
            pass

    def _check_updates(self, force: bool = False):
        try:
            if self.update_worker is not None and self.update_worker.isRunning():
                return
            if not force and not should_auto_check(self._update_last_check):
                return
            self.update_worker = UpdateCheckWorker(self)
            self.update_worker.result.connect(
                lambda info: self._on_update_result(info, force))
            self.update_worker.start()
        except Exception as e:
            debug_log(f"UPDATE exception: {e!r}")

    def _on_update_result(self, info: dict, manual: bool):
        try:
            import time as _time
            previous = self._update_last_seen
            tag = (info or {}).get("tag", "")
            self._update_last_check = _time.time()
            if tag:
                self._update_last_seen = tag
            self._save_config()
            if not tag:
                if manual:
                    self.status_label.setText(self.S()["update_failed"])
                return
            if is_newer(tag, previous):
                msg = self.S()["update_available"].format(tag=tag)
                self.status_label.setText(msg)
                try:
                    self.tray.showMessage(
                        self.S()["app_name"], msg,
                        QSystemTrayIcon.MessageIcon.Information, 5000)
                except Exception:
                    pass
            elif manual:
                self.status_label.setText(self.S()["update_uptodate"])
        except Exception as e:
            debug_log(f"UPDATE result exception: {e!r}")

    def quit_app(self):
        try:
            if self.web_server is not None:
                self.web_server.stop()
        except Exception:
            pass
        self._stop_all_video_windows()
        QApplication.quit()

    def start_web_library(self) -> str | None:
        """Start the localhost library server. Returns its URL or None."""
        try:
            if self.web_server is not None:
                return self.web_server.url
            bridge = _WebBridge(self)
            bridge.apply_requested.connect(self._on_web_apply)
            bridge.stop_requested.connect(self.stop_wallpaper)
            self._web_bridge = bridge
            server = WebLibraryServer(
                apply_fn=lambda p: bridge.apply_requested.emit(p) or True,
                stop_fn=lambda: bridge.stop_requested.emit(),
            )
            if server.start():
                self.web_server = server
                debug_log(f"WEB: library at {server.url}")
                return server.url
        except Exception as e:
            debug_log(f"WEB start failed: {e!r}")
        self.web_server = None
        return None

    def _on_web_apply(self, path: str):
        """Apply a wallpaper chosen in the web library (GUI thread)."""
        try:
            if path and os.path.exists(path):
                self._on_file_chosen(path)
                self.apply_wallpaper()
        except Exception as e:
            debug_log(f"WEB apply exception: {e!r}")

    def open_library(self):
        """Show the library tab (refresh first so it is always current)."""
        try:
            self.showNormal()
            self.raise_()
            try:
                self.library_panel.refresh()
            except Exception:
                pass
            self.tabs.setCurrentIndex(1)
        except Exception as e:
            debug_log(f"LIB open exception: {e!r}")

    def _on_library_apply(self, path: str):
        """Apply a wallpaper chosen in the library dialog."""
        try:
            if path and os.path.exists(path):
                self._on_file_chosen(path)
                self.apply_wallpaper()
        except Exception as e:
            debug_log(f"LIB apply exception: {e!r}")

    def _on_library_add_rotation(self, path: str):
        """Add the library file to the rotation list."""
        try:
            if path and os.path.exists(path):
                if self.rotation.add(path):
                    self._save_config()
                self._refresh_rotation_label()
        except Exception as e:
            debug_log(f"LIB rotation exception: {e!r}")

    def _on_library_delete(self, path: str):
        """Delete a library file (with confirmation)."""
        try:
            from wallmotion.library import delete_media_file
            from wallmotion.webui import media_dir, thumbs_dir
            if not path or not os.path.exists(path):
                return
            name = os.path.basename(path)
            answer = QMessageBox.question(
                self, self.S()["library_delete_title"],
                self.S()["library_delete_text"].format(name=name))
            if answer != QMessageBox.StandardButton.Yes:
                return
            if delete_media_file(media_dir(), name, thumbs_dir()):
                try:
                    self.rotation.remove(path)
                except Exception:
                    pass
                try:
                    self.favorites.discard(path)
                except Exception:
                    pass
                try:
                    if self.selected_path == path:
                        self.selected_path = None
                except Exception:
                    pass
                self._save_config()
                try:
                    self.library_panel.refresh()
                except Exception:
                    pass
        except Exception as e:
            debug_log(f"LIB delete exception: {e!r}")


def main():
    # Qt 6 sets DPI awareness (Per-Monitor V2) by itself.
    # Manual SetProcessDpiAwareness would throw "Access denied",
    # so we deliberately do NOT call it here.
    # Silence chatty FFmpeg logs (Input #0, MFT, ...). They are not errors,
    # just decoding info, so hide them to keep the console clean.
    # Qt categories (via QT_LOGGING_RULES) + native av_log level (directly in FFmpeg).
    os.environ.setdefault("QT_LOGGING_RULES", "qt.multimedia.ffmpeg=false")
    try:
        QLoggingCategory.setFilterRules("qt.multimedia.ffmpeg=false")
    except Exception:
        pass
    _quiet_ffmpeg()
    try:
        from wallmotion.paths import ensure_dirs
        ensure_dirs()
    except Exception:
        pass
    # CLI: parse before QApplication (it would eat its own flags).
    cli_args, startup_cmd = None, {}
    try:
        cli_args = cli.parse_args(sys.argv[1:])
    except SystemExit:
        return  # --help or usage error: argparse already printed
    except Exception:
        cli_args = None
    if cli_args is not None and getattr(cli_args, "version", False):
        print(f"WallMotion {app_version()}")
        return
    if cli_args is not None:
        try:
            startup_cmd = cli.args_to_command(cli_args)
        except Exception:
            startup_cmd = {}
    # Single instance gate: only one app may run (two writers corrupt
    # the config and two players fight over the desktop). Dead owners
    # are detected by PID, so a crash never wedges the lock.
    app_lock = None
    try:
        app_lock = instance.acquire_single_instance_lock()
    except Exception:
        app_lock = None
    if app_lock is None:
        if startup_cmd:
            # Another instance runs: forward the command and exit.
            try:
                instance.send_command(startup_cmd)
            except Exception:
                pass
        return
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
    win._instance_lock = app_lock
    try:
        server = instance.InstanceServer(win)
        server.command_received.connect(win.handle_remote_command)
        if server.start():
            win._instance_server = server
    except Exception:
        pass
    win.show()
    if startup_cmd:
        # CLI launch: apply, then stay in the tray.
        try:
            win.handle_remote_command(startup_cmd)
            win.hide()
        except Exception:
            pass
    else:
        # Normal launch (incl. autostart): restore the last wallpaper
        # shortly after start (cold onefile start already takes seconds,
        # so the desktop is settled - no need to wait longer).
        try:
            QTimer.singleShot(1000, win.restore_last_wallpaper)
        except Exception:
            pass
    try:
        win.start_web_library()
    except Exception:
        pass
    try:
        win._schedule_update_check()
    except Exception:
        pass
    sys.exit(app.exec())
