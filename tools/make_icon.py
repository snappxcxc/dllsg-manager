"""Generate the DLSSG Manager application icon from `assets/app-source.jpg`.

The source artwork is cropped to the rounded badge, made transparent outside it
and exported as ICO/RGBA at every size Windows asks for. Small sizes get a
slightly sharper pass so the mark stays readable in the taskbar.

Run from the repository root:

    python tools/make_icon.py

Produces:
    assets/app.ico      - multi-size icon embedded into the .exe (build.rs)
    assets/app-64.rgba  - raw RGBA used for the egui window icon and the header
    assets/app.png      - 512 px preview
"""

from __future__ import annotations

import statistics
import struct
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
SOURCE = ASSETS / "app-source.jpg"

ICON_SIZES = [16, 24, 32, 48, 64, 128, 256]


def background_luma(image: Image.Image) -> float:
    w, h = image.size
    samples = []
    for x, y in ((4, 4), (w - 5, 4), (4, h - 5), (w - 5, h - 5)):
        r, g, b = image.getpixel((x, y))
        samples.append((r + g + b) / 3)
    return statistics.median(samples)


def find_badge(image: Image.Image) -> tuple[int, int, int, int]:
    """Scans inwards from every side for the bright outline of the badge."""
    w, h = image.size
    bg = background_luma(image)
    threshold = bg + 18

    def luma(x: int, y: int) -> float:
        r, g, b = image.getpixel((x, y))
        return (r + g + b) / 3

    left = next((x for x in range(w) if luma(x, h // 2) > threshold), 0)
    right = next((x for x in range(w - 1, -1, -1) if luma(x, h // 2) > threshold), w - 1)
    top = next((y for y in range(h) if luma(w // 2, y) > threshold), 0)
    bottom = next((y for y in range(h - 1, -1, -1) if luma(w // 2, y) > threshold), h - 1)
    return left, top, right + 1, bottom + 1


def rounded_mask(size: tuple[int, int], radius: int) -> Image.Image:
    mask = Image.new("L", size, 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, size[0] - 1, size[1] - 1], radius=radius, fill=255
    )
    return mask.filter(ImageFilter.GaussianBlur(0.6))


def build_master() -> Image.Image:
    source = Image.open(SOURCE).convert("RGB")
    box = find_badge(source)
    badge = source.crop(box)
    w, h = badge.size
    radius = int(min(w, h) * 0.165)

    badge = badge.convert("RGBA")
    badge.putalpha(rounded_mask(badge.size, radius))

    # Square canvas, badge centred without stretching it.
    side = max(w, h)
    master = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    master.paste(badge, ((side - w) // 2, (side - h) // 2), badge)
    return master


def export(master: Image.Image) -> None:
    def render(size: int) -> Image.Image:
        img = master.resize((size, size), Image.LANCZOS)
        if size <= 48:
            # A touch of sharpening keeps 16-32 px from turning into mush.
            img = img.filter(ImageFilter.UnsharpMask(radius=0.6, percent=70, threshold=1))
        return img

    frames = {size: render(size) for size in ICON_SIZES}

    master.resize((512, 512), Image.LANCZOS).save(ASSETS / "app.png")

    frames[256].save(
        ASSETS / "app.ico",
        format="ICO",
        sizes=[(s, s) for s in ICON_SIZES],
        append_images=[frames[s] for s in ICON_SIZES if s != 256],
    )

    icon64 = frames[64].convert("RGBA")
    (ASSETS / "app-64.rgba").write_bytes(
        struct.pack("<II", 64, 64) + icon64.tobytes()
    )

    width = sum(ICON_SIZES) + 8 * len(ICON_SIZES) + 16
    sheet = Image.new("RGBA", (width, 300), (30, 33, 39, 255))
    x = 8
    for size in ICON_SIZES:
        sheet.alpha_composite(frames[size], (x, 8 + (280 - size) // 2))
        x += size + 8
    sheet.save(ROOT / "target" / "icon-sheet.png")
    print("badge size:", master.size, "ico bytes:", (ASSETS / "app.ico").stat().st_size)


if __name__ == "__main__":
    export(build_master())
