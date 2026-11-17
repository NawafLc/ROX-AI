"""Screen capture of the Roblox client area, resized for Claude."""
from __future__ import annotations

import io
from dataclasses import dataclass

from PIL import Image, ImageDraw, ImageStat

from .window import Rect


@dataclass
class Shot:
    jpeg: bytes
    png_full: Image.Image  # full-res for saving to the vault
    width: int             # size Claude sees
    height: int
    scale: float           # screen px per image px
    rect: Rect
    mean_brightness: float


def grab(rect: Rect, out_width: int = 1280, quality: int = 70, grid: bool = False) -> Shot:
    import mss  # lazy: only on the gaming PC

    with mss.mss() as sct:
        raw = sct.grab({"left": rect.left, "top": rect.top, "width": rect.width, "height": rect.height})
        img = Image.frombytes("RGB", raw.size, raw.bgra, "raw", "BGRX")
    scale = 1.0
    small = img
    if img.width > out_width:
        scale = img.width / out_width
        small = img.resize((out_width, int(round(img.height / scale))), Image.LANCZOS)
    if grid:
        small = draw_grid(small)
    buf = io.BytesIO()
    small.save(buf, "JPEG", quality=quality, optimize=True)
    bright = ImageStat.Stat(small.convert("L").resize((64, 36))).mean[0]
    return Shot(buf.getvalue(), img, small.width, small.height, scale, rect, bright)


def draw_grid(img: Image.Image, step: int = 100) -> Image.Image:
    """Faint coordinate grid so Claude can pick click targets precisely."""
    img = img.copy()
    d = ImageDraw.Draw(img, "RGBA")
    for x in range(step, img.width, step):
        d.line([(x, 0), (x, img.height)], fill=(255, 0, 255, 60), width=1)
        d.text((x + 2, 2), str(x), fill=(255, 0, 255, 200))
    for y in range(step, img.height, step):
        d.line([(0, y), (img.width, y)], fill=(255, 0, 255, 60), width=1)
        d.text((2, y + 2), str(y), fill=(255, 0, 255, 200))
    return img
