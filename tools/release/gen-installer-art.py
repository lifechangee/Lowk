#!/usr/bin/env python3
"""Generates the installer artwork from the brand SVGs in docs/design/logo/final (needs Pillow, no SVG renderer).

  crates/lowk-app/windows/nsis/sidebar.bmp   164x314, 24-bit  (welcome / finish page)
  crates/lowk-app/windows/nsis/header.bmp    150x57,  24-bit  (page header)
  crates/lowk-app/windows/dmg/background.png 660x400          (macOS dmg: app -> Applications)

The mark is read from lowk-mark.svg (M C H V L A Z paths and circles are rasterised here), the wordmark from the
lockup's <text> (colours, weight) and drawn with a system font: set LOWK_FONT=/path/to/font.ttf to pin one.
The outputs are committed; rerun only when the brand changes:  python3 tools/release/gen-installer-art.py
"""
import math
import os
import re
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
LOGO = ROOT / "docs/design/logo/final"
SS = 4  # supersampling for the mark

TOP, BOTTOM, GLOW = (0x14, 0x20, 0x36), (0x07, 0x0B, 0x13), (0x7F, 0xA3, 0xCF)


def hexcol(s):
    s = s.lstrip("#")
    return tuple(int(s[i : i + 2], 16) for i in (0, 2, 4))


# ---------- SVG mark -> polygons ----------
def _arc(p0, rx, ry, rot, large, sweep, p1, n=24):
    (x1, y1), (x2, y2) = p0, p1
    phi = math.radians(rot)
    dx, dy = (x1 - x2) / 2, (y1 - y2) / 2
    x1p = math.cos(phi) * dx + math.sin(phi) * dy
    y1p = -math.sin(phi) * dx + math.cos(phi) * dy
    lam = (x1p / rx) ** 2 + (y1p / ry) ** 2
    if lam > 1:
        rx, ry = rx * math.sqrt(lam), ry * math.sqrt(lam)
    num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p
    den = rx * rx * y1p * y1p + ry * ry * x1p * x1p
    co = math.sqrt(max(0, num / den)) * (-1 if large == sweep else 1)
    cxp, cyp = co * rx * y1p / ry, -co * ry * x1p / rx
    cx = math.cos(phi) * cxp - math.sin(phi) * cyp + (x1 + x2) / 2
    cy = math.sin(phi) * cxp + math.cos(phi) * cyp + (y1 + y2) / 2
    ang = lambda ux, uy, vx, vy: math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)
    t1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
    dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
    if not sweep and dt > 0:
        dt -= 2 * math.pi
    if sweep and dt < 0:
        dt += 2 * math.pi
    pts = []
    for i in range(1, n + 1):
        t = t1 + dt * i / n
        pts.append((cx + rx * math.cos(t) * math.cos(phi) - ry * math.sin(t) * math.sin(phi),
                    cy + rx * math.cos(t) * math.sin(phi) + ry * math.sin(t) * math.cos(phi)))
    return pts


def path_points(d):
    toks = re.findall(r"[MCHVLAZ]|-?\d*\.?\d+", d)
    pts, i, cmd, cur = [], 0, None, (0.0, 0.0)
    num = lambda: float(toks.pop(0))
    while toks:
        if re.match(r"[A-Z]", toks[0]):
            cmd = toks.pop(0)
            if cmd == "Z":
                continue
        if cmd == "M" or cmd == "L":
            cur = (num(), num())
            pts.append(cur)
        elif cmd == "H":
            cur = (num(), cur[1])
            pts.append(cur)
        elif cmd == "V":
            cur = (cur[0], num())
            pts.append(cur)
        elif cmd == "C":
            c1, c2, e = (num(), num()), (num(), num()), (num(), num())
            for k in range(1, 17):
                t = k / 16
                u = 1 - t
                pts.append(tuple(u**3 * cur[j] + 3 * u * u * t * c1[j] + 3 * u * t * t * c2[j] + t**3 * e[j] for j in (0, 1)))
            cur = e
        elif cmd == "A":
            rx, ry, rot, la, sw = num(), num(), num(), int(num()), int(num())
            e = (num(), num())
            pts += _arc(cur, rx, ry, rot, la, sw, e)
            cur = e
    return pts


def load_mark():
    svg = (LOGO / "lowk-mark.svg").read_text()
    vb = [float(v) for v in re.search(r'viewBox="([^"]+)"', svg).group(1).split()]
    shapes = []
    for m in re.finditer(r'<path d="([^"]+)" fill="(#[0-9a-f]{6})"', svg):
        shapes.append(("poly", path_points(m.group(1)), hexcol(m.group(2))))
    for m in re.finditer(r'<circle cx="([\d.]+)" cy="([\d.]+)" r="([\d.]+)" fill="(#[0-9a-f]{6})"', svg):
        shapes.append(("circle", (float(m.group(1)), float(m.group(2)), float(m.group(3))), hexcol(m.group(4))))
    return vb, shapes


def draw_mark(img, cx, top, width):
    """Draws the mark with horizontal centre `cx`, top edge `top`, `width` px wide; returns its height."""
    vb, shapes = load_mark()
    k = width / vb[2]
    h = vb[3] * k
    layer = Image.new("RGBA", (int(width * SS) + 8, int(h * SS) + 8), (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    tx = lambda x: (x - vb[0]) * k * SS
    ty = lambda y: (y - vb[1]) * k * SS
    for kind, geo, col in shapes:
        if kind == "poly":
            d.polygon([(tx(x), ty(y)) for x, y in geo], fill=col + (255,))
        else:
            x, y, r = geo
            d.ellipse([tx(x - r), ty(y - r), tx(x + r), ty(y + r)], fill=col + (255,))
    layer = layer.resize((layer.width // SS, layer.height // SS), Image.LANCZOS)
    img.paste(layer, (int(cx - width / 2), int(top)), layer)
    return h


# ---------- background, text ----------
def background(w, h):
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        row = tuple(round(TOP[c] + (BOTTOM[c] - TOP[c]) * t) for c in range(3))
        for x in range(w):
            px[x, y] = row
    glow = Image.new("L", (w, h), 0)
    gd = ImageDraw.Draw(glow)
    r = int(max(w, h) * 0.7)
    for i in range(r, 0, -4):
        gd.ellipse([w / 2 - i, h * 0.9 - i, w / 2 + i, h * 0.9 + i], fill=int(70 * (1 - i / r) ** 2))
    return Image.composite(Image.new("RGB", (w, h), GLOW), img, glow)


def font(size):
    cands = [os.environ.get("LOWK_FONT", ""), "/System/Library/Fonts/HelveticaNeue.ttc",
             "/Library/Fonts/Arial Bold.ttf", "C:/Windows/Fonts/segoeuib.ttf", "C:/Windows/Fonts/arialbd.ttf",
             "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf"]
    for c in cands:
        if c and Path(c).exists():
            try:
                f = ImageFont.truetype(c, size)
                try:
                    f.set_variation_by_name("Bold")
                except Exception:
                    pass
                return f
            except OSError:
                continue
    sys.exit("no TrueType font found: set LOWK_FONT=/path/to/font.ttf")


def wordmark(img, cx, top, size, spacing=-1):
    """`Low` + accent `k` (colours from the lockup SVG), centred on cx."""
    svg = (LOGO / "lowk-lockup-dark.svg").read_text()
    base = hexcol(re.search(r'fill="(#[0-9a-f]{6})">Low', svg).group(1))
    accent = hexcol(re.search(r'<tspan fill="(#[0-9a-f]{6})">k', svg).group(1))
    f = font(size)
    d = ImageDraw.Draw(img)
    parts = [("Low", base), ("k", accent)]
    widths = [d.textlength(t, font=f) + spacing * len(t) for t, _ in parts]
    x = cx - sum(widths) / 2
    for (t, col), wd in zip(parts, widths):
        d.text((x, top), t, font=f, fill=col)
        x += wd


def text_center(img, cx, y, s, size, col):
    f = font(size)
    d = ImageDraw.Draw(img)
    d.text((cx - d.textlength(s, font=f) / 2, y), s, font=f, fill=col)


# ---------- outputs ----------
def sidebar():
    img = background(164, 314)
    h = draw_mark(img, 82, 78, 104)
    wordmark(img, 82, 78 + h + 18, 38)
    text_center(img, 82, 270, "lowk.app", 11, (0x8A, 0x97, 0xAB))
    return img


def header():
    img = background(150, 57)
    h = draw_mark(img, 36, 14, 44)
    wordmark(img, 105, 12, 28)
    return img


def dmg():
    w, hgt = 660, 400
    img = background(w, hgt)
    wordmark(img, w / 2, 26, 34)
    d = ImageDraw.Draw(img)
    # arrow between the app icon (x=180) and the Applications folder (x=480), both at y=210
    col, y = (0xA9, 0xC4, 0xE4), 210
    d.line([(250, y), (400, y)], fill=col, width=5)
    d.polygon([(412, y), (386, y - 16), (386, y + 16)], fill=col)
    text_center(img, w / 2, 318, "Drag Lowk to Applications", 18, (0xEC, 0xEC, 0xEE))
    return img


def main():
    nsis, dmgdir = ROOT / "crates/lowk-app/windows/nsis", ROOT / "crates/lowk-app/windows/dmg"
    sidebar().save(nsis / "sidebar.bmp", format="BMP")  # RGB -> 24-bit BMP
    header().save(nsis / "header.bmp", format="BMP")
    dmgdir.mkdir(parents=True, exist_ok=True)
    dmg().save(dmgdir / "background.png", format="PNG", optimize=True)
    for p in (nsis / "sidebar.bmp", nsis / "header.bmp", dmgdir / "background.png"):
        print(f"{p.relative_to(ROOT)}  {p.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
