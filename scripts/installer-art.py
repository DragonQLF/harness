#!/usr/bin/env python3
"""Draws the installer artwork the bundlers ask for, from the same mark and
the same fonts the app itself ships.

The bundlers take bitmaps at fixed sizes and nothing else — no SVG, no CSS. So
the geometry of the extruded R (`public/relay.svg`) and the type of the design
file live here once, in code, instead of in a set of binaries nobody can edit
later. Re-run it after a version bump: the version is drawn into the sidebar,
exactly as `docs/design/Relay Lifecycle.dc.html` has it.

    python3 -m pip install pillow fonttools brotli
    python3 scripts/installer-art.py

NSIS reads the two BMPs and silently shows nothing at all if they carry an
alpha channel or a palette, so they are written as flat 24-bit RGB.
"""

from __future__ import annotations

import json
import math
import pathlib
import tempfile

from PIL import Image, ImageDraw, ImageFont
from fontTools.ttLib import TTFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
FONT_SRC = ROOT / "src" / "assets" / "fonts"
OUT = ROOT / "src-tauri" / "icons"

# The palette of the tile, from `Relay Logo.dc.html`. Colour is deliberately
# withheld from the letter; if an accent is ever needed it goes on the tile.
TILE_TOP = (0x22, 0x26, 0x2E)
TILE_BOTTOM = (0x0D, 0x0F, 0x13)
INK_TOP = (0xFF, 0xFF, 0xFF)
INK_BOTTOM = (0xB9, 0xC0, 0xCC)
LIFT = (0x7E, 0x87, 0x98)
PAPER = (0xFF, 0xFF, 0xFF)
WORDMARK = (0xF5, 0xF7, 0xFA)
WORDMARK_LIFT = (0x45, 0x4E, 0x5E)
MONO_TEXT = (0x6B, 0x74, 0x84)
MONO_FAINT = (0x45, 0x4E, 0x5E)
DMG_CAPTION = (0x4E, 0x57, 0x66)
DMG_ARROW_LINE = (0x3A, 0x41, 0x50)
DMG_ARROW_HEAD = (0x5A, 0x64, 0x72)

# Everything is drawn this many times larger and shrunk at the end. The mark is
# all thin strokes and one arc; at 1× they alias into mush.
SS = 4


def ttf(stem: str) -> pathlib.Path:
    """The app's self-hosted woff2, unwrapped so Pillow can read it."""
    cache = pathlib.Path(tempfile.gettempdir()) / "relay-installer-fonts"
    cache.mkdir(parents=True, exist_ok=True)
    target = cache / f"{stem}.ttf"
    if not target.exists():
        font = TTFont(str(FONT_SRC / f"{stem}.woff2"))
        font.flavor = None
        font.save(str(target))
    return target


GROTESK = ttf("space-grotesk-700-latin")
MONO = ttf("ibm-plex-mono-400-latin")
INTER = ttf("inter-400-latin")


def font(path: pathlib.Path, px: float) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(str(path), int(round(px * SS)))


def lerp(a: tuple[int, int, int], b: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    t = min(1.0, max(0.0, t))
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))  # type: ignore[return-value]


def linear_gradient(size: tuple[int, int], start, end, a, b) -> Image.Image:
    """A gradient between two points, the way SVG's userSpaceOnUse means it:
    the colour at a pixel is its projection onto the start→end vector."""
    w, h = size
    img = Image.new("RGB", size)
    px = img.load()
    dx, dy = end[0] - start[0], end[1] - start[1]
    span = dx * dx + dy * dy or 1.0
    for y in range(h):
        for x in range(w):
            t = ((x - start[0]) * dx + (y - start[1]) * dy) / span
            px[x, y] = lerp(a, b, t)
    return img


def css_gradient(size: tuple[int, int], degrees: float, a, b) -> Image.Image:
    """`linear-gradient(<deg>, a, b)` as CSS defines it: 0deg points up, the
    line runs through the centre and is long enough to touch both corners."""
    w, h = size
    rad = math.radians(degrees)
    dx, dy = math.sin(rad), -math.cos(rad)
    length = abs(w * dx) + abs(h * dy)
    cx, cy = w / 2, h / 2
    start = (cx - dx * length / 2, cy - dy * length / 2)
    end = (cx + dx * length / 2, cy + dy * length / 2)
    return linear_gradient(size, start, end, a, b)


# ---- the mark ---------------------------------------------------------------
#
# The three strokes of `public/relay.svg`, in its own 64-unit box: a stem, a
# bowl (bar, semicircle, bar) and a leg. Stroke 7, butt caps, mitre joins —
# which is why they are drawn as plain rectangles rather than as a path.

STROKE = 7.0
SHADOW_OFFSET = 3.4


def _glyph_mask(box: float) -> Image.Image:
    """The letter alone, as an alpha mask at `box` pixels square."""
    k = box / 64.0
    mask = Image.new("L", (int(box), int(box)), 0)
    d = ImageDraw.Draw(mask)
    half = STROKE / 2

    d.rectangle([(23.5 - half) * k, 20 * k, (23.5 + half) * k, 45.5 * k], fill=255)
    d.rectangle([23.5 * k, (20 - half) * k, 32 * k, (20 + half) * k], fill=255)
    d.rectangle([23.5 * k, (35 - half) * k, 32 * k, (35 + half) * k], fill=255)
    # The bowl's semicircle: centre (32, 27.5), radius 7.5, the right half.
    d.arc(
        [(32 - 7.5 - half) * k, (27.5 - 7.5 - half) * k,
         (32 + 7.5 + half) * k, (27.5 + 7.5 + half) * k],
        start=-90, end=90, fill=255, width=int(round(STROKE * k)),
    )
    # The leg, a rotated bar with flat ends.
    ax, ay, bx, by = 30 * k, 35 * k, 41 * k, 45.5 * k
    vx, vy = bx - ax, by - ay
    length = math.hypot(vx, vy)
    nx, ny = -vy / length * half * k, vx / length * half * k
    d.polygon([(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)], fill=255)
    return mask


def tile_mark(box: int) -> Image.Image:
    """The mark on its dark tile, the installer/dock icon lockup."""
    size = box * SS
    k = size / 64.0
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))

    tile = linear_gradient((size, size), (6 * k, 6 * k), (58 * k, 58 * k), TILE_TOP, TILE_BOTTOM)
    rounded = Image.new("L", (size, size), 0)
    ImageDraw.Draw(rounded).rounded_rectangle([4 * k, 4 * k, 60 * k, 60 * k], radius=16 * k, fill=255)
    out.paste(tile, (0, 0), rounded)

    glyph = _glyph_mask(size)
    shadow = Image.new("L", (size, size), 0)
    shadow.paste(glyph, (int(round(SHADOW_OFFSET * k)), int(round(SHADOW_OFFSET * k))))
    out.paste(Image.new("RGB", (size, size), LIFT), (0, 0), shadow.point(lambda v: int(v * 0.55)))

    ink = linear_gradient((size, size), (20 * k, 16 * k), (46 * k, 50 * k), INK_TOP, INK_BOTTOM)
    out.paste(ink, (0, 0), glyph)
    return out.resize((box, box), Image.LANCZOS)


# ---- type ------------------------------------------------------------------


def tracked(draw: ImageDraw.ImageDraw, xy, text: str, fnt, fill, tracking: float = 0.0):
    """Type with letter-spacing, which Pillow has no notion of. Returns the
    width drawn, so a caller can centre it."""
    x, y = xy
    step = tracking * fnt.size
    for ch in text:
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += draw.textlength(ch, font=fnt) + step
    return x - xy[0] - (step if text else 0)


def tracked_width(draw: ImageDraw.ImageDraw, text: str, fnt, tracking: float = 0.0) -> float:
    step = tracking * fnt.size
    return sum(draw.textlength(ch, font=fnt) for ch in text) + step * max(0, len(text) - 1)


def relay_wordmark(draw, xy, px: float, tracking: float, lift: float):
    """RELAY, Space Grotesk 700, with the flat drop the identity always
    carries — the same 1px-ish offset the title bar draws in CSS."""
    fnt = font(GROTESK, px)
    x, y = xy[0] * SS, xy[1] * SS
    tracked(draw, (x + lift * SS, y + lift * SS), "RELAY", fnt, WORDMARK_LIFT, tracking)
    return tracked(draw, (x, y), "RELAY", fnt, WORDMARK, tracking)


# ---- the surfaces ----------------------------------------------------------


def version() -> str:
    return json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text())["version"]


def dark_sidebar(w: int, h: int) -> Image.Image:
    """NSIS's welcome/finish sidebar, and the same art for WiX's dialog: the
    panel that carries the whole brand while the dialog beside it stays the
    stock MUI2 one."""
    img = css_gradient((w * SS, h * SS), 165, TILE_TOP, TILE_BOTTOM)
    d = ImageDraw.Draw(img)

    mark = tile_mark(46 * SS)
    img.paste(mark, (18 * SS, 22 * SS), mark)

    block_top = h - 20 - 11 - 24 - 57
    relay_wordmark(d, (18, block_top), 17, 0.07, 1.5)
    tag = font(MONO, 10)
    d.text((18 * SS, (block_top + 27) * SS), "Desktop orchestrator", font=tag, fill=MONO_TEXT)
    d.text((18 * SS, (block_top + 42) * SS), "for Claude Code agents", font=tag, fill=MONO_TEXT)

    stamp = font(MONO, 9)
    d.text((18 * SS, (h - 20 - 11) * SS), f"{version()} · x64", font=stamp, fill=MONO_FAINT)
    return img.resize((w, h), Image.LANCZOS)


def dark_header(w: int, h: int) -> Image.Image:
    """The strip NSIS puts at the top of every page after the first, and the
    right end of WiX's banner: mark and wordmark, centred, nothing else."""
    img = css_gradient((w * SS, h * SS), 120, TILE_TOP, TILE_BOTTOM)
    d = ImageDraw.Draw(img)

    mark_px = 26
    word = font(GROTESK, 12)
    word_w = tracked_width(d, "RELAY", word, 0.07) / SS
    total = mark_px + 9 + word_w
    x = (w - total) / 2

    mark = tile_mark(mark_px * SS)
    img.paste(mark, (int(x * SS), int((h - mark_px) / 2 * SS)), mark)
    relay_wordmark(d, (x + mark_px + 9, (h - 12 * 1.2) / 2), 12, 0.07, 1.2)
    return img.resize((w, h), Image.LANCZOS)


def wix_banner(w: int, h: int) -> Image.Image:
    """MSI draws the dialog's own title over the left of this banner in dark
    type, so the left stays paper and the brand takes the right end — which is
    the layout the design's progress page already shows."""
    img = Image.new("RGB", (w, h), PAPER)
    img.paste(dark_header(150, h), (w - 150, 0))
    return img


def wix_dialog(w: int, h: int) -> Image.Image:
    """Welcome and completion dialogs: MSI writes its text over the right, so
    the sidebar is the same 164 columns NSIS gets."""
    img = Image.new("RGB", (w, h), PAPER)
    img.paste(dark_sidebar(164, h), (0, 0))
    return img


def dmg_background(w: int, h: int) -> Image.Image:
    """The disk image window. It draws no icons: the app and the Applications
    alias are placed by `appPosition` / `applicationFolderPosition`, and a
    painted copy underneath them is how a DMG ends up with four."""
    size = (w * SS, h * SS)
    img = Image.new("RGB", size)
    px = img.load()
    # radial-gradient(120% 90% at 18% 8%, #1B1F27 0%, #0C0E12 62%)
    cx, cy = 0.18 * size[0], 0.08 * size[1]
    rx, ry = 1.2 * size[0], 0.9 * size[1]
    inner = (0x1B, 0x1F, 0x27)
    outer = (0x0C, 0x0E, 0x12)
    for y in range(size[1]):
        for x in range(size[0]):
            t = math.hypot((x - cx) / rx, (y - cy) / ry) / 0.62
            px[x, y] = lerp(inner, outer, t)
    d = ImageDraw.Draw(img)

    relay_wordmark(d, (26, 22), 14, 0.07, 1.4)
    stamp = font(MONO, 10.5)
    d.text((92 * SS, 25 * SS), version(), font=stamp, fill=(0x5A, 0x64, 0x72))

    caption = font(MONO, 11)
    text = "Drag Relay into Applications"
    d.text(((size[0] - d.textlength(text, font=caption)) / 2, (h - 26 - 11) * SS),
           text, font=caption, fill=DMG_CAPTION)

    # The dashed run between the two icons, centred on the midpoint of the
    # positions the bundle config places them at.
    y = 190 * SS
    x0, x1 = 304 * SS, 356 * SS
    step = 11 * SS
    x = x0
    while x < x1 - step * 0.45:
        d.line([(x, y), (min(x + 6 * SS, x1), y)], fill=DMG_ARROW_LINE, width=int(2.5 * SS))
        x += step
    d.line([(x1 - 8 * SS, y - 6 * SS), (x1, y), (x1 - 8 * SS, y + 6 * SS)],
           fill=DMG_ARROW_HEAD, width=int(2.5 * SS), joint="curve")

    return img.resize((w, h), Image.LANCZOS)


def save_bmp(img: Image.Image, name: str) -> None:
    # 24-bit, no alpha, no palette. NSIS shows nothing at all otherwise, and
    # says nothing about why.
    img.convert("RGB").save(OUT / name, "BMP")
    print("wrote", OUT / name, img.size)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    save_bmp(dark_sidebar(164, 314), "nsis-sidebar.bmp")
    save_bmp(dark_header(150, 57), "nsis-header.bmp")
    save_bmp(wix_banner(493, 58), "wix-banner.bmp")
    save_bmp(wix_dialog(493, 312), "wix-dialog.bmp")
    dmg = dmg_background(660, 400)
    dmg.save(OUT / "dmg-background.png")
    print("wrote", OUT / "dmg-background.png", dmg.size)


if __name__ == "__main__":
    main()
