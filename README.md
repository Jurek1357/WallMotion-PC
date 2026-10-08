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

### Native app (beta, ships from v1.1.0)

[Releases](https://github.com/Jurek1357/WallMotion-PC/releases) contain
two exes sharing one library folder and one config:

- **WallMotion.exe** – the classic Python app above (full features).
- **WallMotion-Settings.exe** – the native Rust rewrite (~28 MB, no
  Python, no console popups): tabbed window (**Settings** +
  **Wallpaper library**, Lively-style grid with search, favorites and
  detail panel), CZ/EN switch, light/dark theme + follow-system toggle,
  YouTube downloads with one-click yt-dlp/ffmpeg setup, playlist picker,
  per-monitor selection, rotation playlist, per-wallpaper volume memory,
  pause on fullscreen/battery, CLI (`--set/--stop/--mute/--volume`),
  hide-to-tray on X.

Video wallpapers need `mpv` on PATH (the classic exe brings its own
decoder; the native one drives mpv). Everything else the native app
downloads itself on first click. Build from source with a Rust
toolchain (`winget install Rustlang.Rustup.Beta` + MinGW-w64 or MSVC):

```bash
cargo build --release -p wallmotion-settings --manifest-path wallmotion-rs/Cargo.toml
```

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
python -m PyInstaller --onefile --windowed --name WallMotion --icon assets\icon.ico --add-data "assets;assets" --add-data "locales;locales" --noconfirm main.py
```

### Autostart with Windows

Place a shortcut to `WallMotion.exe` (or `main.py`) in:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

### Linux support (v1)

The same UI runs on Linux; rendering is delegated to system tools
detected from `$XDG_SESSION_TYPE` / `$XDG_CURRENT_DESKTOP`:

| Session | Image | Video |
|---|---|---|
| X11 | `feh --bg-fill` | `xwinwrap` + `mpv` fullscreen |
| Wayland + KDE / sway / Hyprland / wlroots | `plasma-apply-wallpaperimage` / `swww` | `mpvpaper` (all outputs) |
| Wayland + GNOME | `gsettings` | via the Hanabi extension — the app offers to install it (bundled zip); it activates after sign-out/sign-in |

The AppImage bundles `feh`, `mpv`, `xwinwrap`, `mpvpaper`, `swww` and
`swww-daemon` — no manual installs on any supported session. The only
tools taken from the system are the desktop's own (`gsettings`,
`plasma-apply-wallpaperimage`). Host Mesa/GL is still needed
(`libgl1 libegl1` — preinstalled on practically every desktop distro).
Running from source instead? Install the renderers via your package
manager or drop them into `~/.local/share/wallmotion/bin` — bundled
and drop-in tools are always preferred over `$PATH`.

Volume, mute and pause go through mpv JSON IPC; battery auto-pause
reads `/sys/class/power_supply`; fullscreen auto-pause probes the
active window on X11 via `xprop`/`xwininfo`/`xrandr` (Wayland has no
compositor-neutral equivalent — the rule is a no-op there). Stop
restores the previous wallpaper on GNOME (`gsettings`), X11
(`~/.fehbg`) and wlroots (`swww query`).

Status: verified on GNOME Wayland (Ubuntu 26.04) — see
[docs/LINUX_TESTING.md](docs/LINUX_TESTING.md). Reports for the other
sessions welcome (attach `~/.local/state/wallmotion/debug.log`).

### Known limitations

- Per-monitor video/image selection in the UI; true per-monitor static
  images on Windows still use one fitted image for the whole desktop.
- Video formats depend on Qt Multimedia codecs (mp4/H.264 works out of the box; AV1/VP9 files are refused with a message, the downloader only ever fetches H.264).
- The video wallpaper needs the app running – after a reboot, launch it again (or use autostart).
- YouTube downloads need `yt-dlp` (`pip install -r requirements.txt` includes it, ffmpeg rides along via `imageio-ffmpeg`).
- No Python needed if you use the ready-made `WallMotion.exe` from [Releases](https://github.com/Jurek1357/WallMotion-PC/releases).

### Windows blocked the app?
Smart App Control may block the unsigned `.exe` ("couldn't verify
its publisher"). The app is safe (MIT-licensed, source above) – pick one:
- turn Smart App Control off: Settings → Privacy & Security →
  Windows Security → App & browser control → Smart App Control → Off,
- or run from source (`pip install -r requirements.txt`,
  `python main.py`) – scripts launched by the signed Python are allowed.

### Contributing & license

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Report bugs via
[Issues](https://github.com/Jurek1357/WallMotion-PC/issues) and attach
`%TEMP%\live_wallpaper_debug.log`. Planned work: [TODO.md](TODO.md);
stack analysis and Linux plan: [docs/STACK_ANALYSIS.md](docs/STACK_ANALYSIS.md).
Released under the [MIT license](LICENSE); the bundled ffmpeg binary keeps
its own (L)GPL license, and the bundled Hanabi GNOME Shell extension stays
GPL-3.0 (`assets/hanabi-extension-LICENSE.txt`).

---

## Čeština

Lehká náhrada Wallpaper Engine pro **Windows 10/11**:

- **Tapeta z obrázku** – automaticky upravená přesně na rozlišení obrazovky
- **Tapeta z videa** – smyčkově hraje za ikonami plochy, kliky propadají skrz
- **YouTube podpora** – vlož odkaz, video se stáhne na pozadí a samo nastaví jako tapeta (jen H.264, do 1080p, soubory do 500 MB, vždy se zvukovou stopou)
- **Ovládání hlasitosti** – ztlumení i slider hlasitosti fungují okamžitě, i za běhu videa
- **Tmavý / světlý motiv** a **čeština / angličtina**, pamatuje se pro příště
- **Měření obrazovky** – zjistí rozlišení všech monitorů (fyzické pixely, HiDPI aware)

### Nativní apka (beta, v releasích od v1.1.0)

V [Releases](https://github.com/Jurek1357/WallMotion-PC/releases) jsou
dva exe soubory se společnou složkou knihovny a jedním configem:

- **WallMotion.exe** – klasická Python apka výše (plná funkčnost).
- **WallMotion-Settings.exe** – nativní přepis v Rustu (cca 28 MB, bez
  Pythonu, bez vyskakujících konzolí): okno se záložkami (**Nastavení** +
  **Knihovna tapet**, mřížka ala Lively s hledáním, oblíbenými a
  detailem), přepínač CZ/EN, světlý/tmavý motiv + následování systému,
  stahování z YouTube s instalací yt-dlp/ffmpeg na jedno kliknutí,
  výběr playlistu, výběr monitoru, rotace, paměť hlasitosti na soubor,
  pauza na fullscreen/baterii, CLI (`--set/--stop/--mute/--volume`),
  schování do traye křížkem.

Video tapety potřebují `mpv` v PATH (klasické exe má dekodér vlastní;
nativní řídí mpv). Vše ostatní si nativní apka stáhne sama na první
kliknutí. Build ze zdrojáků s Rust toolchainem (plus MinGW-w64 nebo
MSVC):

```bash
cargo build --release -p wallmotion-settings --manifest-path wallmotion-rs/Cargo.toml
```

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
python -m PyInstaller --onefile --windowed --name WallMotion --icon assets\icon.ico --add-data "assets;assets" --add-data "locales;locales" --noconfirm main.py
```

### Autostart s Windows

Zkratku na `WallMotion.exe` (nebo `main.py`) dej do:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

### Podpora Linuxu (v1)

Na Linuxu běží stejné UI; vykreslování řeší systémové nástroje
detekované z `$XDG_SESSION_TYPE` / `$XDG_CURRENT_DESKTOP`:

| Prostředí | Obrázek | Video |
|---|---|---|
| X11 | `feh --bg-fill` | `xwinwrap` + `mpv` fullscreen |
| Wayland + KDE / sway / Hyprland / wlroots | `plasma-apply-wallpaperimage` / `swww` | `mpvpaper` (všechny výstupy) |
| Wayland + GNOME | `gsettings` | přes rozšíření Hanabi — aplikace nabídne instalaci (přibalený zip); aktivuje se po odhlášení a přihlášení |

AppImage má v sobě `feh`, `mpv`, `xwinwrap`, `mpvpaper`, `swww` a
`swww-daemon` — žádné ruční instalace na žádné podporované session.
Ze systému se berou jen nástroje samotného desktopu (`gsettings`,
`plasma-apply-wallpaperimage`). Pořád je potřeba host Mesa/GL
(`libgl1 libegl1` — předinstalované prakticky všude). Při běhu ze
zdrojáku nástroje nainstaluj přes balíčkovací systém, nebo je hoď do
`~/.local/share/wallmotion/bin` — přibalené a doplňkové nástroje mají
přednost před `$PATH`.

Hlasitost, ztlumení a pauza jdou přes mpv JSON IPC; pauza na baterii
čte `/sys/class/power_supply`; pauza na fullscreen na X11 zjišťuje
aktivní okno přes `xprop`/`xwininfo`/`xrandr` (Wayland nemá obdobný
protokol — tam je pravidlo no-op). Stop vrátí předchozí tapetu na
GNOME (`gsettings`), X11 (`~/.fehbg`) a wlroots (`swww query`).

Stav: ověřeno na GNOME Wayland (Ubuntu 26.04) — viz
[docs/LINUX_TESTING.md](docs/LINUX_TESTING.md). Hlášení z ostatních
session vítána (přilož `~/.local/state/wallmotion/debug.log`).

### Známá omezení

- V UI jde vybrat monitor pro video i obrázek; statický obrázek na
  Windows se pořád nastavuje jeden pro celou plochu.
- Formáty videa závisí na kodecích v Qt Multimedia (mp4/H.264 bez problémů; soubory AV1/VP9 se odmítnou s hláškou, stahovač tahá jen H.264).
- Video tapeta potřebuje běžící aplikaci – po restartu PC ji spusť znovu (nebo autostart).
- Stahování z YouTube potřebuje `yt-dlp` (je v `requirements.txt`, ffmpeg se přibalí přes `imageio-ffmpeg`).
- Bez Pythonu se obejdeš s hotovým `WallMotion.exe` ze záložky [Releases](https://github.com/Jurek1357/WallMotion-PC/releases).

### Windows aplikaci zablokoval?
Smart App Control umí zablokovat nepodepsaný `.exe` („nelze ověřit
vydavatele"). Appka je bezpečná (MIT licence, zdrojáky výše) – vyber si:
- Smart App Control vypni: Nastavení → Soukromí a zabezpečení →
  Zabezpečení Windows → Řízení aplikací a prohlížeče →
  Inteligentní řízení aplikací → Vypnuto,
- nebo spusť ze zdrojáků (`pip install -r requirements.txt`,
  `python main.py`) – skripty puštěné podepsaným Pythonem procházejí.

### Příspěvky a licence

Příspěvky vítány — viz [CONTRIBUTING.md](CONTRIBUTING.md). Chyby hlas do
[Issues](https://github.com/Jurek1357/WallMotion-PC/issues) a přilož
`%TEMP%\live_wallpaper_debug.log`. Plán práce: [TODO.md](TODO.md);
analýza stacku a Linux plán: [docs/STACK_ANALYSIS.md](docs/STACK_ANALYSIS.md).
Kód je pod [licencí MIT](LICENSE); přibalená ffmpeg binárka si nese vlastní
(L)GPL licenci a přibalené rozšíření Hanabi pro GNOME Shell zůstává pod
GPL-3.0 (`assets/hanabi-extension-LICENSE.txt`).
