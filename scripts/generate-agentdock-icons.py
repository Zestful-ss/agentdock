#!/usr/bin/env python3
"""Generate the AgentDock application mark and platform icon variants.

The previous S mark is kept in the *-v1 files as a migration reference; all
active icon paths are generated from this single source so the desktop bundle,
web favicon, and platform launchers stay visually consistent.

Requires Pillow (``python -m pip install Pillow``) when regenerating assets.
"""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SIZE = 1024
SCALE = 4


def _gradient(size: int) -> Image.Image:
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pixels = image.load()
    for y in range(size):
        t = y / max(1, size - 1)
        r = int(23 + (55 - 23) * t)
        g = int(204 + (45 - 204) * t)
        b = int(222 + (242 - 222) * t)
        for x in range(size):
            u = x / max(1, size - 1)
            pixels[x, y] = (
                int(r + (39 - r) * u),
                int(g + (113 - g) * u),
                int(b + (255 - b) * u),
                255,
            )
    return image


def _rounded_mask(size: int, radius: int) -> Image.Image:
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius=radius, fill=255)
    return mask


def _draw_mark(image: Image.Image, *, monochrome: bool = False) -> None:
    """Draw a high-contrast A/dock mark on a 1024px canvas."""
    draw = ImageDraw.Draw(image)
    scale = image.width / SOURCE_SIZE

    def point(x: float, y: float) -> tuple[int, int]:
        return round(x * scale), round(y * scale)

    def width(value: float) -> int:
        return max(1, round(value * scale))

    if monochrome:
        main = (244, 249, 255, 255)
        accent = (196, 181, 253, 255)
        base = (207, 250, 254, 255)
    else:
        main = (248, 252, 255, 255)
        accent = (167, 139, 250, 255)
        base = (222, 251, 255, 245)

    # A subtle offset shadow keeps the mark legible on both light and dark UI.
    shadow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    shadow_draw = ImageDraw.Draw(shadow)
    stroke = width(78)
    shadow_points = [point(294, 724), point(512, 282), point(730, 724)]
    shadow_draw.line(shadow_points, fill=(8, 25, 52, 70), width=stroke, joint="curve")
    shadow_draw.line([point(380, 535), point(644, 535)], fill=(8, 25, 52, 70), width=stroke)
    shadow_draw.rounded_rectangle((*point(260, 758), *point(764, 812)), radius=width(27), fill=(8, 25, 52, 70))
    shadow = shadow.filter(ImageFilter.GaussianBlur(width(13)))
    image.alpha_composite(shadow)

    # The A is intentionally geometric rather than font-dependent, so the mark
    # remains crisp in 16px tray icons and installer favicons.
    draw.line([point(294, 724), point(512, 282), point(730, 724)], fill=main, width=stroke, joint="curve")
    for x, y in ((294, 724), (512, 282), (730, 724)):
        cx, cy = point(x, y)
        radius = stroke // 2
        draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=main)

    draw.line([point(380, 535), point(644, 535)], fill=main, width=stroke)
    for x, y in ((380, 535), (644, 535)):
        cx, cy = point(x, y)
        radius = stroke // 2
        draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=main)

    # A dock shelf and a small gem make the mark distinct from a plain letter A.
    draw.rounded_rectangle((*point(260, 758), *point(764, 812)), radius=width(27), fill=base)
    gem = Image.new("RGBA", image.size, (0, 0, 0, 0))
    gem_draw = ImageDraw.Draw(gem)
    cx, cy = point(782, 760)
    r = width(62)
    gem_draw.polygon([(cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy)], fill=accent)
    image.alpha_composite(gem)


def _mark(size: int, *, monochrome: bool = False) -> Image.Image:
    canvas_size = size * SCALE
    if monochrome:
        image = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
    else:
        image = _gradient(canvas_size)
        mask = _rounded_mask(canvas_size, round(canvas_size * 0.22))
        image.putalpha(mask)
    _draw_mark(image, monochrome=monochrome)
    return image.resize((size, size), Image.Resampling.LANCZOS)


def _save_png(path: Path, size: int, *, monochrome: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    _mark(size, monochrome=monochrome).save(path, format="PNG", optimize=True)


def _save_ico(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    _mark(256).save(path, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])


def _save_icns(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    _mark(1024).save(path, format="ICNS", sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)])


def _dimensions(path: Path) -> tuple[int, int] | None:
    try:
        with Image.open(path) as image:
            return image.size
    except OSError:
        return None


def generate() -> None:
    _save_png(ROOT / "assets" / "icon.png", 128)
    _save_png(ROOT / "public" / "icons" / "32x32.png", 32)

    icon_dir = ROOT / "src-tauri" / "icons"
    for name, size in {
        "icon.png": 512,
        "32x32.png": 32,
        "64x64.png": 64,
        "128x128.png": 128,
        "128x128@2x.png": 256,
        "icon-source.png": 1024,
    }.items():
        _save_png(icon_dir / name, size)

    # Windows square logo assets are named for their pixel dimensions.
    for path in icon_dir.glob("Square*Logo.png"):
        dimensions = _dimensions(path)
        if dimensions:
            _save_png(path, max(dimensions))
    _save_png(icon_dir / "StoreLogo.png", 50)
    _save_ico(icon_dir / "icon.ico")
    _save_icns(icon_dir / "icon.icns")

    for path in (icon_dir / "ios").glob("*.png"):
        dimensions = _dimensions(path)
        if dimensions:
            _save_png(path, max(dimensions))

    for path in (icon_dir / "android").rglob("*.png"):
        dimensions = _dimensions(path)
        if dimensions:
            _save_png(
                path,
                max(dimensions),
                monochrome="foreground" in path.name,
            )


if __name__ == "__main__":
    generate()
