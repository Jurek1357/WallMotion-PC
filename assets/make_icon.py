"""Vygeneruje ikonu WallMotion (monitor s play tlacitkem) do icon.png + icon.ico.

Spusteni:  python assets/make_icon.py
"""

import os

from PIL import Image, ImageDraw

HERE = os.path.dirname(os.path.abspath(__file__))
ACCENT_TOP = (124, 92, 255)
ACCENT_BOTTOM = (82, 58, 208)
WHITE = (255, 255, 255)


def make_icon(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    r = int(size * 0.22)

    # fialovy zaobleny podklad s vertikalnim gradientem
    for y in range(size):
        t = y / max(1, size - 1)
        c = tuple(int(a + (b - a) * t) for a, b in zip(ACCENT_TOP, ACCENT_BOTTOM))
        d.line([(0, y), (size, y)], fill=c + (255,))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, size, size], radius=r, fill=255)
    bg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    bg.paste(img, (0, 0))
    img = Image.composite(
        bg, Image.new("RGBA", (size, size), (0, 0, 0, 0)), mask
    )
    d = ImageDraw.Draw(img)

    # monitor: ramecek + stojan
    mx0, my0 = int(size * 0.19), int(size * 0.26)
    mx1, my1 = int(size * 0.81), int(size * 0.68)
    lw = max(2, int(size * 0.055))
    d.rounded_rectangle([mx0, my0, mx1, my1], radius=int(size * 0.05),
                        outline=WHITE, width=lw)
    # stojan
    sx = size // 2
    d.line([(sx, my1), (sx, int(size * 0.78))], fill=WHITE, width=lw)
    d.line([(int(size * 0.36), int(size * 0.78)),
            (int(size * 0.64), int(size * 0.78))], fill=WHITE, width=lw)

    # play trojuhelnik uprostred obrazovky
    cx, cy = (mx0 + mx1) // 2, (my0 + my1) // 2
    s = int(size * 0.13)
    d.polygon([(cx - s // 2, cy - s), (cx - s // 2, cy + s),
               (cx + s, cy)], fill=WHITE)
    return img


def main() -> None:
    big = make_icon(256)
    big.save(os.path.join(HERE, "icon.png"))
    big.save(
        os.path.join(HERE, "icon.ico"),
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48),
               (64, 64), (128, 128), (256, 256)],
    )
    print("assets/icon.png + assets/icon.ico hotovo")


if __name__ == "__main__":
    main()
