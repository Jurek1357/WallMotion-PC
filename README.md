# Live Wallpaper – jednoduchá náhrada Wallpaper Engine

Aplikace umí:
- nastavit **obrázek** jako klasickou tapetu plochy,
- nastavit **video (mp4 a další)** jako živou tapetu, která se smyčkově
  přehrává na pozadí, přesně jako u Wallpaper Engine.

Funguje pouze na **Windows** (10/11), protože využívá Windows API
(`SystemParametersInfo` a trik s oknem `WorkerW`).

## 1. Instalace

Potřebuješ Python 3.9+ nainstalovaný na Windows.

```bash
pip install -r requirements.txt
```

## 2. Spuštění

```bash
python main.py
```

Otevře se malé okno – vybereš soubor (obrázek nebo video) a klikneš
na „Nastavit jako tapetu“. U videa se aplikace nesmí úplně ukončit
(zůstává v systémové liště – zavřením okna křížkem se jen schová),
protože video přehrávač musí běžet, aby se tapeta pořád animovala.
Ukončit ji jde přes ikonu v systémové liště → „Ukončit“.

Tlačítko „Zastavit / obnovit původní“ video tapetu vypne a vrátí
tapetu, která byla nastavená před spuštěním appky.

## 3. Jak to funguje (video tapeta)

Windows si interně drží skryté okno `Progman` a po poslání speciální
zprávy vytvoří okno `WorkerW`, které leží mezi plochou a ikonami.
Aplikace do tohoto okna vloží vlastní okno s přehrávaným videem
(přes `SetParent`), takže vizuálně splyne s pozadím plochy a ikony
zůstávají navrchu, klikatelné.

## 4. Volitelné: sbalení do .exe

Aby appka nepotřebovala mít nainstalovaný Python u sebe, jde ji
zabalit přes PyInstaller:

```bash
pip install pyinstaller
pyinstaller --onefile --windowed --name LiveWallpaper main.py
```

Výsledné `.exe` najdeš ve složce `dist/`.

## 5. Volitelné: spouštět automaticky při startu Windows

Zkratku na `LiveWallpaper.exe` (nebo na `main.py`) stačí umístit do:

```
%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
```

## Známá omezení

- Testováno na jednom monitoru (primární obrazovka). Pro víc monitorů
  by šlo okno s videem roztáhnout přes všechny obrazovky – dá se
  doplnit, pokud to budeš potřebovat.
- Podporované video formáty závisí na kodecích dostupných přes Qt
  Multimedia (mp4/H.264 funguje bez problémů ve výchozím stavu).
- Video tapeta zmizí po restartu appky/PC, dokud appku znovu
  nespustíš (lze vyřešit přidáním do startupu, viz výše).
