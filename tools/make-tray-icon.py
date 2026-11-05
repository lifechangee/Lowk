#!/usr/bin/env python3
"""Extract the light ghost from the app icon as a black macOS template icon."""
from pathlib import Path

from PIL import Image

root = Path(__file__).resolve().parents[1]
icons = root / "crates/lowk-app/icons"
source = Image.open(icons / "icon.png").convert("RGBA")
# The ghost is the light region in the lower part of the icon. Keep its dark
# eyes transparent, and preserve antialiased edges via the luminance ramp.
mask = Image.new("L", source.size)
mask.putdata([
    max(0, min(255, (min(r, g, b) - 120) * 255 // 90)) if y >= 270 else 0
    for y in range(source.height)
    for r, g, b, a in [source.getpixel((x, y)) for x in range(source.width)]
])
mask = mask.crop(mask.getbbox())
mask.thumbnail((32, 32), Image.Resampling.LANCZOS)
canvas = Image.new("RGBA", (36, 36), (0, 0, 0, 0))
canvas.paste(Image.new("RGBA", mask.size, (0, 0, 0, 255)), ((36 - mask.width) // 2, (36 - mask.height) // 2), mask)
canvas.save(icons / "tray-template.png")
