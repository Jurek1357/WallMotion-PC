"""Video wallpaper: playback via Qt + frame painting via GDI onto the desktop."""

from __future__ import annotations

import ctypes
import time

from PySide6.QtCore import Qt, QTimer, QUrl, Signal
from PySide6.QtGui import QImage
from PySide6.QtMultimedia import QAudioOutput, QMediaPlayer, QVideoFrame, QVideoSink
from PySide6.QtWidgets import QWidget

from wallmotion import screens
from wallmotion.autopause import (
    POLL_INTERVAL_MS,
    RESUME_AFTER_CLEAN_POLLS,
    is_fullscreen_app_active,
    is_on_battery,
    should_pause,
)
from wallmotion.frames import ensure_packed_rgb32
from wallmotion.utils import _quiet_ffmpeg, debug_log
from wallmotion.win32 import (
    _BLACKNESS,
    _CANVAS_CLASS,
    _COLORONCOLOR,
    _DIB_RGB_COLORS,
    _GDI32,
    _GWL_EXSTYLE,
    _HALFTONE,
    _LWA_ALPHA,
    _SRCCOPY,
    _USER32,
    _WS_CHILD,
    _WS_CLIPCHILDREN,
    _WS_EX_LAYERED,
    _WS_EX_NOACTIVATE,
    _WS_EX_NOREDIRECTIONBITMAP,
    _WS_EX_TOOLWINDOW,
    _WS_EX_TRANSPARENT,
    _WS_VISIBLE,
    _BitmapInfo,
    _ensure_canvas_class,
    get_workerw_handle,
    win32con,
    win32gui,
)


class VideoWallpaperWindow(QWidget):
    """Video wallpaper embedded into WorkerW behind desktop icons.

    Deliberately does NOT use QVideoWidget: Qt treats an embedded child
    of a foreign window as "not exposed" and never delivers paint events
    to it (so QVideoWidget would stay black). Instead we take frames via
    QVideoSink (those arrive reliably) and paint them directly via GDI
    (StretchDIBits) onto our window HDC - this works regardless of Qt.
    """

    failed = Signal(str)

    def __init__(self, video_path: str, muted: bool = True, volume: float = 0.3,
                 auto_pause_fullscreen: bool = True,
                 auto_pause_battery: bool = False,
                 monitor: dict | None = None):
        super().__init__()
        # No frame, no focus, no activation - must not steal clicks/focus.
        # No layout or children: the whole window is a canvas painted via GDI.
        self.setWindowFlags(
            Qt.FramelessWindowHint
            | Qt.WindowDoesNotAcceptFocus
            | Qt.BypassWindowManagerHint
        )
        self.setAttribute(Qt.WA_ShowWithoutActivating, True)
        # Qt level: ignore mouse so clicks fall through to the desktop
        self.setAttribute(Qt.WA_TransparentForMouseEvents, True)

        self.video_path = video_path
        self._muted = bool(muted)
        self._volume = max(0.0, min(1.0, float(volume)))
        # Selected monitor {x, y, w, h} in physical pixels, or None = all.
        self._monitor = dict(monitor) if monitor else None
        # Auto-pause rules (fullscreen app / battery). Timer starts in start().
        self._pause_on_fullscreen = bool(auto_pause_fullscreen)
        self._pause_on_battery = bool(auto_pause_battery)
        self._autopause_timer = None
        self._autopaused = False
        self._clean_polls = 0
        self._user_paused = False  # manual Pause button (autopause must not override)

        self.player = QMediaPlayer(self)
        self.audio_output = QAudioOutput(self)
        self.audio_output.setVolume(0.0 if self._muted else self._volume)
        self.player.setAudioOutput(self.audio_output)
        self.sink = QVideoSink(self)
        self.player.setVideoOutput(self.sink)
        self.sink.videoFrameChanged.connect(self._on_frame)
        try:
            self.player.errorOccurred.connect(self._on_player_error)
        except Exception:
            pass
        # Native infinite loop is enough on its own. A manual restart via
        # mediaStatusChanged would fight with it and cause repeated file
        # reopening, so we deliberately do not connect it here.
        try:
            self.player.setLoops(QMediaPlayer.Loops.Infinite)
        except Exception:
            self.player.mediaStatusChanged.connect(self._loop_video)

        self._hdc = 0
        self._canvas = 0  # native canvas HWND (WorkerW child, no Qt window)
        self._dw = 0  # canvas width in physical pixels (per WorkerW)
        self._dh = 0
        self._frame_size = (0, 0)
        self._frames = 0
        self._blits_ok = 0
        self._blit_fail = 0
        # render mode: "coloroncolor" (fast) | "halftone" (nicer, 4x slower)
        self._blit_mode = "coloroncolor"
        self._bmi = None  # cached BITMAPINFO for the current src size
        # Flood guard: process at most ~40 frames/s, drop the rest.
        # Otherwise with 60fps / slow blit the frame queue balloons in memory
        # and the system may freeze. Usual 24-30fps video passes without skips.
        self._proc_min_interval = 0.025
        self._last_proc_t = 0.0
        self._skipped = 0
        # Largest frame still painted 1:1. Bigger ones (4K/8K from YouTube)
        # are downscaled first, so GDI is not flooded with tens of MB per frame.
        self._max_src_pixels = 2560 * 1440
        self._downscaled = False

    def _loop_video(self, status):
        if status == QMediaPlayer.MediaStatus.EndOfMedia:
            self.player.setPosition(0)
            self.player.play()

    def set_muted(self, muted: bool) -> None:
        """F4: toggles sound immediately, even while video is playing."""
        self._muted = bool(muted)
        try:
            self.audio_output.setVolume(0.0 if self._muted else self._volume)
        except Exception:
            pass

    def set_volume(self, volume: float) -> None:
        """Sets volume immediately (0.0-1.0), even while video is playing."""
        self._volume = max(0.0, min(1.0, float(volume)))
        try:
            if not self._muted:
                self.audio_output.setVolume(self._volume)
        except Exception:
            pass

    def set_auto_pause(self, fullscreen: bool, battery: bool) -> None:
        """Change auto-pause rules live (checkboxes in MainWindow)."""
        self._pause_on_fullscreen = bool(fullscreen)
        self._pause_on_battery = bool(battery)
        self._clean_polls = 0
        try:
            if self._pause_on_fullscreen or self._pause_on_battery:
                self._start_autopause_timer()
            else:
                self._stop_autopause_timer()
                if self._autopaused and not self._user_paused:
                    self._resume_playback("rules-off")
        except Exception as e:
            debug_log(f"AUTOPAUSE: set_auto_pause vyjimka: {e!r}")

    def _start_autopause_timer(self) -> None:
        try:
            if self._autopause_timer is None:
                self._autopause_timer = QTimer(self)
                self._autopause_timer.timeout.connect(self._autopause_tick)
            if not self._autopause_timer.isActive():
                self._autopause_timer.start(POLL_INTERVAL_MS)
        except Exception as e:
            debug_log(f"AUTOPAUSE: start timeru selhalo: {e!r}")

    def _stop_autopause_timer(self) -> None:
        try:
            if self._autopause_timer is not None:
                self._autopause_timer.stop()
        except Exception:
            pass

    def _pause_playback(self, reason: str) -> None:
        try:
            self.player.pause()
            self._autopaused = True
            self._clean_polls = 0
            debug_log(f"AUTOPAUSE: pauza ({reason})")
        except Exception as e:
            debug_log(f"AUTOPAUSE: pause selhalo: {e!r}")

    def _resume_playback(self, reason: str) -> None:
        try:
            self.player.play()
            self._autopaused = False
            self._clean_polls = 0
            debug_log(f"AUTOPAUSE: pokracuji ({reason})")
        except Exception as e:
            debug_log(f"AUTOPAUSE: play selhalo: {e!r}")

    def set_user_paused(self, paused: bool) -> None:
        """Manual Pause button: freeze the frame, keep the canvas.

        Autopause never resumes a user-paused video (see tick guard).
        """
        self._user_paused = bool(paused)
        try:
            if self._user_paused:
                self.player.pause()
            elif not self._autopaused:
                self.player.play()
            debug_log(f"user paused={self._user_paused}")
        except Exception as e:
            debug_log(f"user pause failed: {e!r}")

    def is_user_paused(self) -> bool:
        return bool(self._user_paused)

    def _autopause_tick(self) -> None:
        """Poll sensors every POLL_INTERVAL_MS. Pause at once, resume only
        after RESUME_AFTER_CLEAN_POLLS clean polls (no alt-tab flapping)."""
        try:
            if not (self._pause_on_fullscreen or self._pause_on_battery):
                return
            if self._canvas == 0:
                return  # already stopped
            if self._user_paused:
                return  # manual pause wins, do not touch playback
            try:
                fullscreen = (
                    is_fullscreen_app_active() if self._pause_on_fullscreen else False
                )
                battery = is_on_battery() if self._pause_on_battery else False
            except Exception:
                return  # sensor failure: keep current state, try next poll
            want_pause = should_pause(
                fullscreen, battery,
                pause_on_fullscreen=self._pause_on_fullscreen,
                pause_on_battery=self._pause_on_battery,
            )
            if want_pause:
                if not self._autopaused:
                    self._pause_playback(
                        f"fullscreen={fullscreen} battery={battery}"
                    )
                return
            if self._autopaused:
                self._clean_polls += 1
                if self._clean_polls >= RESUME_AFTER_CLEAN_POLLS:
                    self._resume_playback("clean")
        except Exception as e:
            debug_log(f"AUTOPAUSE: tick vyjimka: {e!r}")

    def _on_player_error(self, error, error_string):
        debug_log(f"PLAYER ERROR: {error} | {error_string}")

    def start(self) -> bool:
        """Creates a native canvas for video and starts playback. Returns True.

        Windows 11 "raised desktop" (Progman has WS_EX_NOREDIRECTIONBITMAP,
        DefView is WS_EX_LAYERED): the canvas must be a DIRECT child of Progman
        with WS_EX_LAYERED + SetLayeredWindowAttributes(alpha=255), z-ordered
        BELOW DefView (icons) and ABOVE WorkerW (static wallpaper). Without a
        layered window DWM will not composite the video and it stays black /
        empty (recommendation straight from Microsoft, same as Lively /
        Wallpaper Engine does).

        Older Windows (no raised desktop): canvas as a WorkerW child.
        Frames are painted via GDI the same way in both cases.
        """
        debug_log(f"START: path={self.video_path}")
        progman = win32gui.FindWindow("Progman", None)
        if progman == 0:
            debug_log("START: Progman nenalezeno")
            return False
        try:
            pex = _USER32.GetWindowLongW(progman, _GWL_EXSTYLE)
        except Exception:
            pex = 0
        raised = bool(pex & _WS_EX_NOREDIRECTIONBITMAP)
        defview = win32gui.FindWindowEx(progman, 0, "SHELLDLL_DefView", None)
        workerw = get_workerw_handle()
        debug_log(
            f"START: progman={progman} raised={raised} "
            f"defview={defview} workerw={workerw}"
        )
        if workerw == 0:
            debug_log("START: WorkerW nenalezeno")
            return False
        try:
            px1, py1, px2, py2 = win32gui.GetWindowRect(progman)
            wx1, wy1, wx2, wy2 = win32gui.GetWindowRect(workerw)
            debug_log(f"START: progman rect=({px1},{py1})-({px2},{py2}) "
                      f"workerw rect=({wx1},{wy1})-({wx2},{wy2})")
            if self._monitor:
                # Per-monitor video: canvas only above the selected monitor.
                base = (px1, py1, px2, py2) if raised else (wx1, wy1, wx2, wy2)
                x, y, w, h = screens.place_canvas(base, self._monitor)
                debug_log(f"START: monitor canvas x={x} y={y} {w}x{h}")
            else:
                w, h = max(1, wx2 - wx1), max(1, wy2 - wy1)
                x, y = wx1 - px1, wy1 - py1
        except Exception as e:
            debug_log(f"START: GetWindowRect selhalo: {e!r}")
            return False

        if raised:
            parent = progman
            exstyle = (
                _WS_EX_LAYERED | _WS_EX_NOACTIVATE
                | _WS_EX_TRANSPARENT | _WS_EX_TOOLWINDOW
            )
        else:
            parent = workerw
            if not self._monitor:
                x, y = 0, 0
            exstyle = _WS_EX_NOACTIVATE | _WS_EX_TRANSPARENT | _WS_EX_TOOLWINDOW
        try:
            cls_ok = _ensure_canvas_class()
            canvas_cls = _CANVAS_CLASS if cls_ok else "STATIC"
            canvas = _USER32.CreateWindowExW(
                exstyle, canvas_cls, None,
                _WS_CHILD | _WS_VISIBLE | _WS_CLIPCHILDREN,
                x, y, w, h,
                parent, None, None, None,
            )
        except Exception as e:
            debug_log(f"START: CreateWindowEx vyjimka: {e!r}")
            return False
        if not canvas:
            try:
                err = ctypes.windll.kernel32.GetLastError()
            except Exception:
                err = "?"
            debug_log(f"START: CreateWindowEx vratilo 0 (err={err})")
            return False
        self._canvas = int(canvas)
        self._dw, self._dh = int(w), int(h)

        if raised:
            # fully opaque layered window, otherwise DWM will not composite
            try:
                _USER32.SetLayeredWindowAttributes(
                    self._canvas, 0, 255, _LWA_ALPHA
                )
            except Exception as e:
                debug_log(f"START: SetLayeredWindowAttributes: {e!r}")
            # Z-order choreography: canvas BELOW icons, WorkerW BELOW canvas
            try:
                if defview:
                    win32gui.SetWindowPos(
                        self._canvas, defview, 0, 0, 0, 0,
                        win32con.SWP_NOMOVE | win32con.SWP_NOSIZE
                        | win32con.SWP_NOACTIVATE,
                    )
                win32gui.SetWindowPos(
                    workerw, self._canvas, 0, 0, 0, 0,
                    win32con.SWP_NOMOVE | win32con.SWP_NOSIZE
                    | win32con.SWP_NOACTIVATE,
                )
                debug_log("START: z-order nastaven (WorkerW < canvas < DefView)")
            except Exception as e:
                debug_log(f"START: SetWindowPos z-order: {e!r}")
        else:
            try:
                win32gui.SetWindowPos(
                    self._canvas, win32con.HWND_BOTTOM, 0, 0, 0, 0,
                    win32con.SWP_NOMOVE | win32con.SWP_NOSIZE
                    | win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW,
                )
            except Exception:
                pass

        try:
            self._hdc = win32gui.GetDC(self._canvas)
            sm = _COLORONCOLOR if self._blit_mode == "coloroncolor" else _HALFTONE
            _GDI32.SetStretchBltMode(self._hdc, sm)
            _GDI32.PatBlt(self._hdc, 0, 0, self._dw, self._dh, _BLACKNESS)
            debug_log(f"START: canvas={self._canvas} {w}x{h} hdc={self._hdc}")
        except Exception as e:
            debug_log(f"START: GetDC selhalo: {e!r}")
            self._destroy_canvas()
            return False
        _quiet_ffmpeg()  # just in case, again before opening the file
        self.player.setSource(QUrl.fromLocalFile(self.video_path))
        self.player.play()
        debug_log("START: play() zavolano -> OK")
        if self._pause_on_fullscreen or self._pause_on_battery:
            self._start_autopause_timer()
        # Watchdog: if no frame arrives within 8 s (e.g. unsupported codec
        # like AV1 - the player gets stuck buffering without an error),
        # destroy the canvas again so it does not stay up empty.
        try:
            QTimer.singleShot(8000, self._check_progress)
        except Exception:
            pass
        return True

    def _check_progress(self):
        try:
            if self._canvas == 0:
                return  # already stopped, all OK
            if self._frames > 0:
                return  # playing, all OK
            pos = 0
            try:
                pos = self.player.position()
            except Exception:
                pass
            debug_log(f"WATCHDOG: zadny snimek za 8 s (pos={pos}) -> rusim platno")
            self.failed.emit("decode")
            self.stop()
        except Exception as e:
            debug_log(f"WATCHDOG vyjimka: {e!r}")

    def _make_bmi(self, sw: int, sh: int) -> _BitmapInfo:
        bmi = _BitmapInfo()
        bmi.header.biSize = 40
        bmi.header.biWidth = sw
        bmi.header.biHeight = -sh  # top-down
        bmi.header.biPlanes = 1
        bmi.header.biBitCount = 32
        bmi.header.biCompression = 0
        return bmi

    def _blit_stretch(self, img: QImage, sw: int, sh: int) -> int:
        """Cover via GDI StretchDIBits (fast COLORONCOLOR mode).
        Zero-copy: passes the QImage buffer pointer directly via a writable
        memoryview (bits -> from_buffer -> cast), without copying the whole
        frame (1080p RGB32 = ~8 MB per frame). img and view live for the whole
        synchronous call, so it is safe. On failure, fallback copy via
        bytes(). BITMAPINFO is cached per video size."""
        scale = max(self._dw / sw, self._dh / sh)
        dw, dh = int(sw * scale), int(sh * scale)
        dx, dy = int((self._dw - dw) / 2), int((self._dh - dh) / 2)
        buf = None
        _keepalive = None
        try:
            n = img.sizeInBytes()
            if n > 0:
                mv = img.bits()  # writable memoryview, no copy
                _keepalive = (ctypes.c_char * n).from_buffer(mv)
                buf = ctypes.cast(_keepalive, ctypes.c_void_p)
        except Exception:
            buf = None
        if buf is None:
            try:
                raw = bytes(img.constBits())  # fallback with copy
                if not raw:
                    return 0
                _keepalive = raw
                buf = ctypes.c_char_p(raw)
            except Exception:
                return 0
        if self._bmi is None:
            self._bmi = self._make_bmi(sw, sh)
        return _GDI32.StretchDIBits(
            self._hdc, dx, dy, dw, dh, 0, 0, sw, sh,
            buf, ctypes.byref(self._bmi), _DIB_RGB_COLORS, _SRCCOPY,
        )

    def _on_frame(self, frame: QVideoFrame):
        if not self._hdc:
            if self._frames == 0:
                debug_log("FRAME: prvni snimek, ale _hdc==0 (GetDC selhalo?)")
            self._frames += 1
            return
        try:
            mapped = frame.map(QVideoFrame.MapMode.ReadOnly)
        except Exception as e:
            debug_log(f"FRAME: map vyjimka: {e!r}")
            return
        if not mapped:
            debug_log("FRAME: map vratil False")
            return
        try:
            now = time.perf_counter()
            if self._frames > 0 and (now - self._last_proc_t) < self._proc_min_interval:
                # lagging behind: drop the frame (queue must not grow)
                self._skipped += 1
                return
            img = frame.toImage()
            if img.isNull():
                debug_log("FRAME: toImage null")
                return
            img = ensure_packed_rgb32(img)
            sw, sh = img.width(), img.height()
            if sw <= 0 or sh <= 0 or self._dw <= 0 or self._dh <= 0:
                debug_log(f"FRAME: spatny rozmer src={sw}x{sh} dst={self._dw}x{self._dh}")
                return
            if sw * sh > self._max_src_pixels:
                # huge frame (4K/8K): downscale fast, else OOM/stall
                f = (self._max_src_pixels / (sw * sh)) ** 0.5
                nw, nh = max(2, int(sw * f)), max(2, int(sh * f))
                img = img.scaled(
                    nw, nh,
                    Qt.AspectRatioMode.IgnoreAspectRatio,
                    Qt.TransformationMode.FastTransformation,
                )
                sw, sh = img.width(), img.height()
                if not self._downscaled:
                    self._downscaled = True
                    debug_log(f"FRAME: downscale na {sw}x{sh} (guard)")
            self._last_proc_t = now
            if self._frames == 0:
                try:
                    samples = [img.pixel(10, 10),
                               img.pixel(max(0, sw - 11), 10),
                               img.pixel(sw // 2, sh // 2)]
                    total = 0
                    for px in samples:
                        total += (px & 0xFF) + ((px >> 8) & 0xFF) + ((px >> 16) & 0xFF)
                    bright = total // max(1, 3 * 3)
                except Exception:
                    bright = -1
                debug_log(
                    f"FRAME: prvni snimek src={sw}x{sh} "
                    f"dst={self._dw}x{self._dh} mode={self._blit_mode} "
                    f"bright={bright}"
                )
            if (sw, sh) != self._frame_size:
                # video size changed -> repaint canvas so no borders remain
                self._frame_size = (sw, sh)
                self._bmi = None
                try:
                    _GDI32.PatBlt(self._hdc, 0, 0, self._dw, self._dh, _BLACKNESS)
                except Exception:
                    pass
            r = self._blit_stretch(img, sw, sh)
            self._frames += 1
            if r > 0:
                self._blits_ok += 1
            else:
                self._blit_fail += 1
                if self._blit_fail <= 3:
                    debug_log(f"FRAME: blit vratil {r}")
            if self._frames == 30:
                debug_log(f"FRAME: 30 snimku OK, blits_ok={self._blits_ok}")
        except Exception:
            import traceback
            debug_log("FRAME VYJIMKA:\n" + traceback.format_exc())
            pass
        finally:
            try:
                frame.unmap()
            except Exception:
                pass

    def _embed_into_desktop_DEPRECATED(self):
        """UNUSED: old way of embedding the Qt window (SetParent + style
        conversion). Qt never painted a window embedded this way, so start()
        now creates a native WS_CHILD canvas directly in WorkerW. Kept only
        for history, delete later."""
        GWL_STYLE = -16
        GWL_EXSTYLE = -20
        WS_CHILD = 0x40000000
        WS_POPUP = 0x80000000
        WS_VISIBLE = 0x10000000
        WS_CAPTION = 0x00C00000
        WS_THICKFRAME = 0x00040000
        WS_MINIMIZEBOX = 0x00020000
        WS_MAXIMIZEBOX = 0x00010000
        WS_SYSMENU = 0x00080000
        WS_EX_TOPMOST = 0x00000008
        WS_EX_TOOLWINDOW = 0x00000080
        WS_EX_APPWINDOW = 0x00040000
        WS_EX_NOACTIVATE = 0x08000000
        WS_EX_TRANSPARENT = 0x00000020  # click-through

        hwnd = int(self.winId())
        workerw = get_workerw_handle()
        debug_log(f"EMBED: hwnd={hwnd} workerw={workerw}")

        vx, vy, vw, vh = screens.get_virtual_screen_rect()

        if workerw == 0:
            # When WorkerW does not exist, at least send the window to the bottom and make it mouse-transparent
            try:
                style = win32gui.GetWindowLong(hwnd, GWL_STYLE)
                style = style & ~WS_POPUP & ~WS_CAPTION & ~WS_THICKFRAME
                win32gui.SetWindowLong(hwnd, GWL_STYLE, style)
                ex = win32gui.GetWindowLong(hwnd, GWL_EXSTYLE)
                ex = (ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT) & ~WS_EX_APPWINDOW
                win32gui.SetWindowLong(hwnd, GWL_EXSTYLE, ex)
                win32gui.SetWindowPos(
                    hwnd, win32con.HWND_BOTTOM, vx, vy, vw, vh,
                    win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW | win32con.SWP_ASYNCWINDOWPOS,
                )
                try:
                    x1, y1, x2, y2 = win32gui.GetWindowRect(hwnd)
                    self._dw, self._dh = max(1, x2 - x1), max(1, y2 - y1)
                except Exception:
                    pass
            except Exception:
                pass
            return

        try:
            # 1) switch from WS_POPUP to WS_CHILD so it is really a desktop child
            style = win32gui.GetWindowLong(hwnd, GWL_STYLE)
            style = style & ~WS_POPUP & ~WS_CAPTION & ~WS_THICKFRAME
            style = style & ~WS_MINIMIZEBOX & ~WS_MAXIMIZEBOX & ~WS_SYSMENU
            style = style | WS_CHILD | WS_VISIBLE
            win32gui.SetWindowLong(hwnd, GWL_STYLE, style)

            # 2) ex-style: never take focus, never take clicks
            ex = win32gui.GetWindowLong(hwnd, GWL_EXSTYLE)
            ex = ex & ~WS_EX_TOPMOST & ~WS_EX_APPWINDOW
            ex = ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW
            win32gui.SetWindowLong(hwnd, GWL_EXSTYLE, ex)

            # 3) embed into WorkerW
            win32gui.SetParent(hwnd, workerw)

            # 4) stretch over the whole WorkerW (child coords are relative
            #    to WorkerW, so always 0,0 + WorkerW width/height).
            #    No virtual-screen math - WorkerW already has the right size.
            try:
                wx1, wy1, wx2, wy2 = win32gui.GetWindowRect(workerw)
                w, h = max(1, wx2 - wx1), max(1, wy2 - wy1)
            except Exception:
                _, _, w, h = screens.get_virtual_screen_rect()

            win32gui.SetWindowPos(
                hwnd, win32con.HWND_BOTTOM, 0, 0, w, h,
                win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW | win32con.SWP_ASYNCWINDOWPOS,
            )
            # NOTE: no self.resize(w, h)! w/h are physical pixels from WinAPI,
            # while Qt works in logical ones (at 150% scaling, 1920x1080
            # physical = 1280x720 logical). Native size already matches the Qt
            # geometry from __init__, another resize would break the video.
            # Just store the size for GDI frame painting.
            self._dw, self._dh = int(w), int(h)
            debug_log(f"EMBED: canvas={w}x{h}")
        except Exception as e:
            print("Embed selhal:", e)
            debug_log(f"EMBED VYJIMKA: {e!r}")

    def _destroy_canvas(self):
        try:
            if self._hdc and self._canvas:
                try:
                    win32gui.ReleaseDC(self._canvas, self._hdc)
                except Exception:
                    pass
        except Exception:
            pass
        self._hdc = 0
        try:
            if self._canvas:
                _USER32.DestroyWindow(self._canvas)
                debug_log(f"STOP: canvas {self._canvas} zniceno")
        except Exception as e:
            debug_log(f"STOP: DestroyWindow selhalo: {e!r}")
        self._canvas = 0
        self._dw, self._dh = 0, 0

    def stop(self):
        debug_log(
            f"STOP: frames={self._frames} skipped={self._skipped} "
            f"blits_ok={self._blits_ok} blit_fail={self._blit_fail}"
        )
        self._stop_autopause_timer()
        self._autopaused = False
        self._user_paused = False
        try:
            self.player.stop()
        except Exception:
            pass
        self._destroy_canvas()
        try:
            self.close()
        except Exception:
            pass
