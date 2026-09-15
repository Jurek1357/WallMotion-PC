# WallMotion PC – Live Wallpaper for Windows / Živá tapeta pro Windows

[![CI](https://github.com/Jurek1357/WallMotion-PC/actions/workflows/ci.yml/badge.svg)](https://github.com/Jurek1357/WallMotion-PC/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Jurek1357/WallMotion-PC)](https://github.com/Jurek1357/WallMotion-PC/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**English** | [Čeština](#čeština)

## English

A lightweight Wallpaper Engine alternative for **Windows 10/11**:

- **Image wallpaper** – auto-fitted to your exact screen resolution
- **Video wallpaper** – loops behind the desktop icons, clicks pass through
- **YouTube support** – paste a link, the video downloads in the background and sets itself as wallpaper (H.264 only, up to 1080p, files up to 500 MB, always with an audio track)
- **Volume control** – mute checkbox and volume slider apply immediately, even while the video is playing
- **Dark / light themes** and **Czech / English UI**, remembered between launches
- **Screen measurement** – detects resolution of all monitors (physical pixels, HiDPI aware)

### How it works

- **Image:** resized/cropped to the measured resolution and set via `SystemParametersInfo`.
- **Video:** decoded with Qt Multimedia (`QMediaPlayer` + `QVideoSink`); frames are painted with GDI (`StretchDIBits`, fast `COLORONCOLOR` stretch, ~6 ms/frame at 1080p) onto a native window:
  - on Windows 11 ("raised desktop") as an opaque `WS_EX_LAYERED` child of `Progman`, z-ordered `WorkerW` → **video** → `SHELLDLL_DefView` (same technique as Lively / Wallpaper Engine, per Microsoft guidance),
  - on older Windows as a child of the `WorkerW` window spawned with message `0x052C`.
- Safety guards: frame throttling (~40 fps max, no memory backlog), auto-downscale of 4K/8K frames, and a watchdog that removes the canvas with a clear message if a video produces no frames (e.g. AV1 files, which Qt can't decode here – the downloader avoids them automatically).
- Clicks and focus never get stolen (`WS_EX_TRANSPARENT` + `WS_EX_NOACTIVATE`).

### Install & run (from source)

Requires Python 3.9+ on Windows.

```bash
pip install -r requirements.txt
python main.py
```

Drop an image/video onto the window (or click to browse) and hit **Set as wallpaper**. For video the app must keep running – closing the window only hides it to the system tray. Quit via tray icon → Quit.

- **Stop / restore original** – stops the video and restores the previous wallpaper.
- **Re-measure display** – re-detects resolution (e.g. after plugging in a monitor).
- **YouTube field** – paste a link, hit the button, download runs on a background thread with progress in the status line. Only genuine YouTube links are accepted. Downloads are stored in the `downloads/` folder next to the app (or .exe).
- **Sound** – uncheck *Mute video sound* and set the volume with the slider; both act instantly. Merging of picture and sound is handled by the built-in ffmpeg, nothing to install.
- Language/theme dropdowns are at the top; everything (incl. last file and mute) is saved to `%USERPROFILE%\.live_wallpaper_config.json`.
- Debug log (if anything misbehaves): `%TEMP%\live_wallpaper_debug.log`.

### Standalone .exe

```
dist\WallMotion.exe
```

Built with PyInstaller (`--onefile --windowed`), ~65 MB, needs no Python installed. To rebuild:

```bash
pip install pyinstaller
python -m PyInstaller --onefile --windowed --name WallMotion --icon assets\icon.ico --add-data "assets;assets" --noconfirm main.py
```

### Autostart with Windows

Place a shortcut to `WallMotion.exe` (or `main.py`) in:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

### Known limitations

- Primary-monitor focused; multi-monitor spanning is a possible extension.
- Video formats depend on Qt Multimedia codecs (mp4/H.264 works out of the box; AV1/VP9 files are refused with a message, the downloader only ever fetches H.264).
- The video wallpaper needs the app running – after a reboot, launch it again (or use autostart).
- YouTube downloads need `yt-dlp` (`pip install -r requirements.txt` includes it, ffmpeg rides along via `imageio-ffmpeg`).
- No Python needed if you use the ready-made `WallMotion.exe` from [Releases](https://github.com/Jurek1357/WallMotion-PC/releases).

### Contributing & license

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Report bugs via
[Issues](https://github.com/Jurek1357/WallMotion-PC/issues) and attach
`%TEMP%\live_wallpaper_debug.log`. Released under the [MIT license](LICENSE);
the bundled ffmpeg binary keeps its own (L)GPL license.

---

## Čeština

Lehká náhrada Wallpaper Engine pro **Windows 10/11**:

- **Tapeta z obrázku** – automaticky upravená přesně na rozlišení obrazovky
- **Tapeta z videa** – smyčkově hraje za ikonami plochy, kliky propadají skrz
- **YouTube podpora** – vlož odkaz, video se stáhne na pozadí a samo nastaví jako tapeta (jen H.264, do 1080p, soubory do 500 MB, vždy se zvukovou stopou)
- **Ovládání hlasitosti** – ztlumení i slider hlasitosti fungují okamžitě, i za běhu videa
- **Tmavý / světlý motiv** a **čeština / angličtina**, pamatuje se pro příště
- **Měření obrazovky** – zjistí rozlišení všech monitorů (fyzické pixely, HiDPI aware)

### Jak to funguje

- **Obrázek:** ořízne se na změřené rozlišení a nastaví přes `SystemParametersInfo`.
- **Video:** dekóduje Qt Multimedia (`QMediaPlayer` + `QVideoSink`), snímky se malují přes GDI (`StretchDIBits`, rychlý `COLORONCOLOR` stretch, cca 6 ms/snímek při 1080p) do nativního okna:
  - na Windows 11 („raised desktop“) jako neprůhledný `WS_EX_LAYERED` potomek `Progmanu`, ve vrstvách `WorkerW` → **video** → `SHELLDLL_DefView` (stejný postup jako Lively / Wallpaper Engine, dle Microsoftu),
  - na starších Windows jako potomek okna `WorkerW` vytvořeného zprávou `0x052C`.
- Pojistky: throttle snímků (max ~40/s, fronta se nenafukuje), auto-zmenšení 4K/8K snímků a watchdog, který při nulovém počtu snímků plátno zruší s hláškou (např. AV1 soubory, které Qt tu nedekóduje – stahovač se jim automaticky vyhýbá).
- Kliky ani focus se nekradou (`WS_EX_TRANSPARENT` + `WS_EX_NOACTIVATE`).

### Instalace a spuštění (ze zdrojáků)

Potřebuješ Python 3.9+ na Windows.

```bash
pip install -r requirements.txt
python main.py
```

Přetáhni obrázek/video do okna (nebo klikni pro výběr) a stiskni **Nastavit jako tapetu**. U videa musí aplikace běžet dál – zavření okna křížkem ji jen schová do systémové lišty. Ukončíš ji přes ikonu v liště → Ukončit.

- **Zastavit / obnovit původní** – vypne video a vrátí předchozí tapetu.
- **Změřit obrazovku znovu** – znovu změří rozlišení (třeba po připojení monitoru).
- **YouTube políčko** – vlož odkaz, stiskni tlačítko, stahování běží ve vlákně na pozadí s průběhem ve stavovém řádku. Berou se jen pravé YouTube odkazy. Stažená videa najdeš ve složce `downloads/` vedle aplikace (nebo .exe).
- **Zvuk** – odškrtni *Ztlumit zvuk videa* a nastav hlasitost sliderem; obojí funguje hned. Sloučení obrazu a zvuku řeší přibalený ffmpeg, nic se neinstaluje.
- Jazyk/motiv se přepíná roletkami nahoře; vše (včetně posledního souboru a ztlumení) se ukládá do `%USERPROFILE%\.live_wallpaper_config.json`.
- Debug log (kdyby něco zlobilo): `%TEMP%\live_wallpaper_debug.log`.

### Samostatné .exe

```
dist\WallMotion.exe
```

Sbalené přes PyInstaller (`--onefile --windowed`), cca 65 MB, nepotřebuje Python. Rebuild:

```bash
pip install pyinstaller
python -m PyInstaller --onefile --windowed --name WallMotion --icon assets\icon.ico --add-data "assets;assets" --noconfirm main.py
```

### Autostart s Windows

Zkratku na `WallMotion.exe` (nebo `main.py`) dej do:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

### Známá omezení

- Primárně jeden (primární) monitor; roztažení přes víc monitorů jde doplnit.
- Formáty videa závisí na kodecích v Qt Multimedia (mp4/H.264 bez problémů; soubory AV1/VP9 se odmítnou s hláškou, stahovač tahá jen H.264).
- Video tapeta potřebuje běžící aplikaci – po restartu PC ji spusť znovu (nebo autostart).
- Stahování z YouTube potřebuje `yt-dlp` (je v `requirements.txt`, ffmpeg se přibalí přes `imageio-ffmpeg`).
- Bez Pythonu se obejdeš s hotovým `WallMotion.exe` ze záložky [Releases](https://github.com/Jurek1357/WallMotion-PC/releases).

### Příspěvky a licence

Příspěvky vítány — viz [CONTRIBUTING.md](CONTRIBUTING.md). Chyby hlas do
[Issues](https://github.com/Jurek1357/WallMotion-PC/issues) a přilož
`%TEMP%\live_wallpaper_debug.log`. Kód je pod [licencí MIT](LICENSE);
přibalená ffmpeg binárka si nese vlastní (L)GPL licenci.
