# WallMotion PC – Live Wallpaper for Windows / Živá tapeta pro Windows

**English** | [Čeština](#čeština)

## English

A simple Wallpaper Engine alternative for **Windows 10/11**. It can:

- set an **image** as the classic desktop wallpaper (auto-fitted to your screen resolution),
- set a **video (mp4 and more)** as a live wallpaper that loops behind the desktop icons.

### How it works

- **Image:** resized/cropped to match the measured screen resolution, then set via `SystemParametersInfo`.
- **Video:** decoded with Qt Multimedia (`QMediaPlayer` + `QVideoSink`), frames are painted with GDI (`StretchDIBits`) onto a native window embedded in the desktop:
  - on Windows 11 ("raised desktop") as a `WS_EX_LAYERED` child of `Progman`, z-ordered between `WorkerW` and `SHELLDLL_DefView` (same technique as Lively / Wallpaper Engine),
  - on older Windows as a child of the `WorkerW` window created via message `0x052C`.
- Clicks pass through to the desktop/icons (`WS_EX_TRANSPARENT` + `WS_EX_NOACTIVATE`).

### 1. Install

You need Python 3.9+ on Windows.

```bash
pip install -r requirements.txt
```

### 2. Run

```bash
python main.py
```

A small window opens – drop an image/video (or click to browse) and hit **"Nastavit jako tapetu" (Set as wallpaper)**. For video the app must keep running (closing the window only hides it to the system tray, so the player keeps animating the wallpaper). Quit via the tray icon → "Ukončit" (Quit).

The **"Zastavit / obnovit původní" (Stop / restore original)** button stops the video wallpaper and restores the wallpaper that was set before the app started. **"Změřit obrazovku znovu" (Re-measure screen)** re-detects the screen resolution (useful after plugging in a monitor).

### 3. Optional: build a .exe

So the app runs without an installed Python, package it with PyInstaller:

```bash
pip install pyinstaller
pyinstaller --onefile --windowed --name LiveWallpaper main.py
```

The resulting `.exe` will be in `dist/`.

### 4. Optional: start automatically with Windows

Place a shortcut to `LiveWallpaper.exe` (or `main.py`) in:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

### Known limitations

- Single-monitor focused (primary screen); multi-monitor spanning can be added.
- Supported video formats depend on codecs available through Qt Multimedia (mp4/H.264 works out of the box).
- The video wallpaper disappears when the app/PC restarts until you launch the app again (see autostart above).
- GDI software rendering is used on purpose (Qt never paints a reparented window); 1080p@30fps costs roughly 6 ms/frame.

---

## Čeština

Jednoduchá náhrada Wallpaper Engine pro **Windows 10/11**. Umí:

- nastavit **obrázek** jako klasickou tapetu plochy (automaticky upravený na rozlišení obrazovky),
- nastavit **video (mp4 a další)** jako živou tapetu, která se smyčkově přehrává za ikonami plochy.

### Jak to funguje

- **Obrázek:** ořízne/přizpůsobí se změřenému rozlišení obrazovky a nastaví přes `SystemParametersInfo`.
- **Video:** dekóduje Qt Multimedia (`QMediaPlayer` + `QVideoSink`), snímky se malují přes GDI (`StretchDIBits`) do nativního okna vnořeného do plochy:
  - na Windows 11 („raised desktop“) jako `WS_EX_LAYERED` potomek `Progmanu`, vrstveně mezi `WorkerW` a `SHELLDLL_DefView` (stejný postup jako Lively / Wallpaper Engine),
  - na starších Windows jako potomek okna `WorkerW` vytvořeného zprávou `0x052C`.
- Kliky propadají na plochu/ikony (`WS_EX_TRANSPARENT` + `WS_EX_NOACTIVATE`).

### 1. Instalace

Potřebuješ Python 3.9+ na Windows.

```bash
pip install -r requirements.txt
```

### 2. Spuštění

```bash
python main.py
```

Otevře se malé okno – přetáhni obrázek/video (nebo klikni pro výběr) a stiskni **„Nastavit jako tapetu“**. U videa musí aplikace běžet dál (zavření okna křížkem ji jen schová do systémové lišty, aby přehrávač dál animoval tapetu). Ukončíš ji přes ikonu v liště → „Ukončit“.

Tlačítko **„Zastavit / obnovit původní“** video tapetu vypne a vrátí tapetu, která byla nastavená před spuštěním appky. Tlačítko **„Změřit obrazovku znovu“** znovu změří rozlišení (hodí se po připojení monitoru).

### 3. Volitelné: sbalení do .exe

Aby appka nepotřebovala nainstalovaný Python, zabal ji přes PyInstaller:

```bash
pip install pyinstaller
pyinstaller --onefile --windowed --name LiveWallpaper main.py
```

Výsledné `.exe` najdeš ve složce `dist/`.

### 4. Volitelné: spouštět automaticky při startu Windows

Zkratku na `LiveWallpaper.exe` (nebo na `main.py`) umísti do:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

### Známá omezení

- Primárně jeden monitor (primární obrazovka); roztažení přes víc monitorů jde doplnit.
- Podporované formáty videa závisí na kodecích v Qt Multimedia (mp4/H.264 funguje bez problémů).
- Video tapeta zmizí po restartu appky/PC, dokud appku znovu nespustíš (viz autostart výše).
- Záměrně se používá softwarové GDI vykreslování (Qt vnořené okno nikdy nevykreslí); 1080p@30fps stojí cca 6 ms/snímek.
