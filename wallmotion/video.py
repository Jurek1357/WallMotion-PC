"""Video tapeta: prehravani pres Qt + malovani snimku pres GDI na plochu."""

from __future__ import annotations

import ctypes
import time

from PySide6.QtCore import Qt, QTimer, QUrl, Signal
from PySide6.QtGui import QImage
from PySide6.QtMultimedia import QAudioOutput, QMediaPlayer, QVideoFrame, QVideoSink
from PySide6.QtWidgets import QWidget

from wallmotion import screens
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
    """Video tapeta vnorena do WorkerW za ikonami plochy.

    Zamerne NEPOUZIVA QVideoWidget: Qt totiz vnoreneho potomka ciziho okna
    povazuje za „neexponovaneho“ a nikdy mu nedoruci paint eventy (ani
    QVideoWidget pak nic nevykresli, zustane cerny). Misto toho bereme
    snimky pres QVideoSink (ty chodi spolehlive) a malujeme je primo pres
    GDI (StretchDIBits) na HDC naseho okna - to funguje bez ohledu na Qt.
    """

    failed = Signal(str)

    def __init__(self, video_path: str, muted: bool = True, volume: float = 0.3):
        super().__init__()
        # Bez ramecku, bez focusu, bez aktivace - nesmi krast kliky/focus.
        # Zadny layout ani potomek: cele okno je platno, maluje se pres GDI.
        self.setWindowFlags(
            Qt.FramelessWindowHint
            | Qt.WindowDoesNotAcceptFocus
            | Qt.BypassWindowManagerHint
        )
        self.setAttribute(Qt.WA_ShowWithoutActivating, True)
        # Qt uroveň: ignoruj mys, at kliky padaji na plochu
        self.setAttribute(Qt.WA_TransparentForMouseEvents, True)

        self.video_path = video_path
        self._muted = bool(muted)
        self._volume = max(0.0, min(1.0, float(volume)))

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
        # Nativni nekonecna smycka staci sama. Rucni restart pres
        # mediaStatusChanged by se s ni hadal a zpusoboval vicenasobne
        # otevirani souboru, tak ho tu schvalne nepripojujeme.
        try:
            self.player.setLoops(QMediaPlayer.Loops.Infinite)
        except Exception:
            self.player.mediaStatusChanged.connect(self._loop_video)

        self._hdc = 0
        self._canvas = 0  # nativni HWND platna (potomek WorkerW, zadne Qt okno)
        self._dw = 0  # sirka platna ve fyzickych pixelech (dle WorkerW)
        self._dh = 0
        self._frame_size = (0, 0)
        self._frames = 0
        self._blits_ok = 0
        self._blit_fail = 0
        # mod vykreslovani: "coloroncolor" (rychly) | "halftone" (hezci, 4x pomalejsi)
        self._blit_mode = "coloroncolor"
        self._bmi = None  # kesovane BITMAPINFO pro aktualni src rozmer
        # Ochrana proti zahlceni: zpracuj nejvys ~40 snimku/s, zbytek zahod.
        # Jinak se pri 60fps / pomalem blitu fronta snimku nafukuje do pameti
        # a system muze zamrznout. Bezne 24-30fps video prochazi bez skipu.
        self._proc_min_interval = 0.025
        self._last_proc_t = 0.0
        self._skipped = 0
        # Nejvetsi snimek, jaky jeste malujeme 1:1. Vetsi (4K/8K z YouTube)
        # nejdriv rychle zmensime, at GDI nezahltime desitkami MB na snimek.
        self._max_src_pixels = 2560 * 1440
        self._downscaled = False

    def _loop_video(self, status):
        if status == QMediaPlayer.MediaStatus.EndOfMedia:
            self.player.setPosition(0)
            self.player.play()

    def set_muted(self, muted: bool) -> None:
        """F4: okamzite prepne zvuk, i kdyz video prave hraje."""
        self._muted = bool(muted)
        try:
            self.audio_output.setVolume(0.0 if self._muted else self._volume)
        except Exception:
            pass

    def set_volume(self, volume: float) -> None:
        """Okamzite nastavi hlasitost (0.0-1.0), i kdyz video prave hraje."""
        self._volume = max(0.0, min(1.0, float(volume)))
        try:
            if not self._muted:
                self.audio_output.setVolume(self._volume)
        except Exception:
            pass

    def _on_player_error(self, error, error_string):
        debug_log(f"PLAYER ERROR: {error} | {error_string}")

    def start(self) -> bool:
        """Vytvori nativni platno pro video a spusti prehravani. Vrati True.

        Windows 11 „raised desktop“ (Progman ma WS_EX_NOREDIRECTIONBITMAP,
        DefView je WS_EX_LAYERED): platno musi byt PRIMO potomek Progmanu
        s WS_EX_LAYERED + SetLayeredWindowAttributes(alpha=255), z-orderovane
        POD DefView (ikony) a NAD WorkerW (staticka tapeta). Bez vrstveneho
        okna DWM video neskomponuje a zustane cerno/nic (doporuceni primo
        od Microsoftu, stejne to dela Lively/Wallpaper Engine).

        Starsi Windows (bez raised desktop): platno jako potomek WorkerW.
        Snimky se maluji pres GDI v obou pripadech stejne.
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
            # plne nepruhledne vrstvene okno, jinak DWM neskomponuje
            try:
                _USER32.SetLayeredWindowAttributes(
                    self._canvas, 0, 255, _LWA_ALPHA
                )
            except Exception as e:
                debug_log(f"START: SetLayeredWindowAttributes: {e!r}")
            # Z-order choreografie: platno POD ikony, WorkerW POD platno
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
        _quiet_ffmpeg()  # pro jistotu znovu pred otevrenim souboru
        self.player.setSource(QUrl.fromLocalFile(self.video_path))
        self.player.play()
        debug_log("START: play() zavolano -> OK")
        # Watchdog: kdyz do 8 s neprijde ani snimek (nedejboze nepodporovany
        # kodek typu AV1 - prehravac se zasekne v bufferingu bez chyby),
        # platno zase zrusime, at nezustane svitit naprazdno.
        try:
            QTimer.singleShot(8000, self._check_progress)
        except Exception:
            pass
        return True

    def _check_progress(self):
        try:
            if self._canvas == 0:
                return  # uz zastaveno, vse OK
            if self._frames > 0:
                return  # hraje, vse OK
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
        """Cover pres GDI StretchDIBits (rychly COLORONCOLOR rezim).
        Zero-copy: predava se primo pointer na buffer QImage pres writable
        memoryview (bits -> from_buffer -> cast), bez kopie celeho snimku
        (1080p RGB32 = ~8 MB na snimek). img i view ziji po celou dobu
        synchronniho volani, takze je to bezpecne. Pri neuspechu zalozni
        kopie pres bytes(). BITMAPINFO je kesovane podle rozmeru videa."""
        scale = max(self._dw / sw, self._dh / sh)
        dw, dh = int(sw * scale), int(sh * scale)
        dx, dy = int((self._dw - dw) / 2), int((self._dh - dh) / 2)
        buf = None
        _keepalive = None
        try:
            n = img.sizeInBytes()
            if n > 0:
                mv = img.bits()  # writable memoryview, zadna kopie
                _keepalive = (ctypes.c_char * n).from_buffer(mv)
                buf = ctypes.cast(_keepalive, ctypes.c_void_p)
        except Exception:
            buf = None
        if buf is None:
            try:
                raw = bytes(img.constBits())  # zaloha s kopii
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
                # nestihame: snimek zahodit (fronta se nesmi nafukovat)
                self._skipped += 1
                return
            img = frame.toImage()
            if img.isNull():
                debug_log("FRAME: toImage null")
                return
            if img.format() != QImage.Format.Format_RGB32:
                img = img.convertToFormat(QImage.Format.Format_RGB32)
            sw, sh = img.width(), img.height()
            if sw <= 0 or sh <= 0 or self._dw <= 0 or self._dh <= 0:
                debug_log(f"FRAME: spatny rozmer src={sw}x{sh} dst={self._dw}x{self._dh}")
                return
            if sw * sh > self._max_src_pixels:
                # obri snimek (4K/8K): rychle zmensit, jinak OOM/zaseknuti
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
                debug_log(
                    f"FRAME: prvni snimek src={sw}x{sh} "
                    f"dst={self._dw}x{self._dh} mode={self._blit_mode}"
                )
            if (sw, sh) != self._frame_size:
                # zmena rozmeru videa -> premazat platno, at nezustanou okraje
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
        """NEPOUZIVA SE: stary zpusob vnoreni Qt okna (SetParent + konverze
        stylu). Qt takhle vnorene okno nikdy nevykreslovalo, proto start()
        ted vytvari nativni WS_CHILD platno rovnou v WorkerW. Ponechano jen
        pro historii, casem smazat."""
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
            # Kdyz WorkerW neexistuje, aspon posli okno dospodu a zpruhledni pro mys
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
            # 1) predelat z WS_POPUP na WS_CHILD, at je to opravdu potomek plochy
            style = win32gui.GetWindowLong(hwnd, GWL_STYLE)
            style = style & ~WS_POPUP & ~WS_CAPTION & ~WS_THICKFRAME
            style = style & ~WS_MINIMIZEBOX & ~WS_MAXIMIZEBOX & ~WS_SYSMENU
            style = style | WS_CHILD | WS_VISIBLE
            win32gui.SetWindowLong(hwnd, GWL_STYLE, style)

            # 2) ex-style: nikdy nebrat focus, nikdy nebrat kliky
            ex = win32gui.GetWindowLong(hwnd, GWL_EXSTYLE)
            ex = ex & ~WS_EX_TOPMOST & ~WS_EX_APPWINDOW
            ex = ex | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW
            win32gui.SetWindowLong(hwnd, GWL_EXSTYLE, ex)

            # 3) vnoreni do WorkerW
            win32gui.SetParent(hwnd, workerw)

            # 4) roztahnout pres cele WorkerW ( souradnice potomka jsou relativni
            #    k WorkerW, takze vzdy 0,0 + sirka/vyska WorkerW ).
            #    Zadna virtual-screen matematika - WorkerW uz ma spravnou velikost.
            try:
                wx1, wy1, wx2, wy2 = win32gui.GetWindowRect(workerw)
                w, h = max(1, wx2 - wx1), max(1, wy2 - wy1)
            except Exception:
                _, _, w, h = screens.get_virtual_screen_rect()

            win32gui.SetWindowPos(
                hwnd, win32con.HWND_BOTTOM, 0, 0, w, h,
                win32con.SWP_NOACTIVATE | win32con.SWP_SHOWWINDOW | win32con.SWP_ASYNCWINDOWPOS,
            )
            # POZOR: zadny self.resize(w, h)! w/h jsou fyzicke pixely z WinAPI,
            # zatimco Qt pracuje v logickych (pri 150% skalovani je 1920x1080
            # fyzicky = 1280x720 logicky). Nativni velikost uz sedi s Qt
            # geometrii z __init__, dalsi resize by video rozhodil.
            # Rozmer si jen ulozime pro GDI vykreslovani snimku.
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
        try:
            self.player.stop()
        except Exception:
            pass
        self._destroy_canvas()
        try:
            self.close()
        except Exception:
            pass
