#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11", "numpy>=2", "scipy>=1.14"]
# ///
"""Builds every icon file from assets/brand/source/icon-master.png.

    uv run scripts/gen-icons.py

The master is the artwork on a flat cream background. This script:
1. lifts the rounded tile off the background (soft, un-mixed edges),
2. writes the tile as transparent PNGs in every size (assets/brand/png/),
3. a macOS icon on Apple's grid (824 px tile in 1024, soft shadow) and
   Calmfold.icns,
4. a Windows icon (tile fills the square) and calmfold.ico,
5. web favicons and touch icons (assets/brand/favicon/),
6. the in-app copy used on the About screen (crates/ui/assets/brand/).
Re-run it whenever the master changes.
"""

import os
import shutil
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image, ImageFilter
from scipy import ndimage

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BRAND = os.path.join(ROOT, "assets", "brand")
MASTER = os.path.join(BRAND, "source", "icon-master.png")
CREAM = (250, 246, 240)


def lift_tile(path):
    """The tile on transparency, cropped to a square around it."""
    rgb = np.asarray(Image.open(path).convert("RGB")).astype(np.float64)
    h, w, _ = rgb.shape
    corners = np.array([rgb[4, 4], rgb[4, -5], rgb[-5, 4], rgb[-5, -5]])
    bg = corners.mean(axis=0)
    dist = np.sqrt(((rgb - bg) ** 2).sum(-1))
    # Background = near-bg pixels connected to the border.
    near = dist < 10
    labels, _ = ndimage.label(near)
    border = set(np.unique(np.concatenate([labels[0], labels[-1], labels[:, 0], labels[:, -1]])))
    border.discard(0)
    background = np.isin(labels, list(border))
    inside = ~background
    # Solid interior: shrink the inside a little; edge ring gets un-mixed alpha.
    core = ndimage.binary_erosion(inside, iterations=3)
    _, (iy, ix) = ndimage.distance_transform_edt(~core, return_indices=True)
    nearest = rgb[iy, ix]
    diff = nearest - bg
    denom = (diff**2).sum(-1)
    alpha = np.where(denom > 1e-6, ((rgb - bg) * diff).sum(-1) / np.maximum(denom, 1e-6), 0.0)
    alpha = np.clip(alpha, 0, 1)
    alpha[core] = 1.0
    alpha[ndimage.binary_dilation(background, iterations=0) & ~ndimage.binary_dilation(inside, iterations=4)] = 0
    color = np.where(core[..., None], rgb, nearest)
    rgba = np.dstack([color, alpha * 255]).round().clip(0, 255).astype(np.uint8)
    img = Image.fromarray(rgba, "RGBA")
    ys, xs = np.where(alpha > 0.02)
    cx, cy = (xs.min() + xs.max()) / 2, (ys.min() + ys.max()) / 2
    side = max(xs.max() - xs.min(), ys.max() - ys.min()) + 4
    box = (round(cx - side / 2), round(cy - side / 2), round(cx + side / 2), round(cy + side / 2))
    return img.crop(box)


def fit(tile, canvas, fill):
    """`tile` scaled to `fill` of a transparent `canvas`-px square, centred."""
    size = round(canvas * fill)
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    t = tile.resize((size, size), Image.LANCZOS)
    off = (canvas - size) // 2
    out.alpha_composite(t, (off, off))
    return out


def mac_icon(tile):
    """Apple's grid: 824 px tile in 1024 with a soft drop shadow."""
    canvas = 1024
    art = fit(tile, canvas, 824 / 1024)
    shadow_alpha = art.getchannel("A").filter(ImageFilter.GaussianBlur(14))
    shadow = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    shadow.putalpha(shadow_alpha.point(lambda a: int(a * 0.28)))
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    out.alpha_composite(shadow, (0, 10))
    out.alpha_composite(art)
    return out


def save(img, *parts):
    path = os.path.join(*parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path, optimize=True)
    return path


def main():
    if not os.path.exists(MASTER):
        sys.exit(f"missing {MASTER}")
    tile = lift_tile(MASTER)
    save(tile, BRAND, "source", "icon-tile.png")

    # Transparent tile, every common size (small sizes: almost full bleed).
    for s in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        save(fit(tile, s, 0.94 if s <= 64 else 0.9), BRAND, "png", f"icon-{s}.png")

    # macOS.
    mac = mac_icon(tile)
    save(mac, BRAND, "source", "icon-mac.png")
    if sys.platform == "darwin" and shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = os.path.join(tmp, "Calmfold.iconset")
            os.makedirs(iconset)
            for s in (16, 32, 128, 256, 512):
                mac.resize((s, s), Image.LANCZOS).save(os.path.join(iconset, f"icon_{s}x{s}.png"))
                mac.resize((s * 2, s * 2), Image.LANCZOS).save(
                    os.path.join(iconset, f"icon_{s}x{s}@2x.png")
                )
            out = os.path.join(BRAND, "Calmfold.icns")
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", out], check=True)
            print("wrote", out)

    # Windows: the tile itself fills the square; small sizes get no padding.
    win = fit(tile, 256, 0.96)
    save(win, BRAND, "source", "icon-windows.png")
    sizes = [(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (256, 256)]
    ico = os.path.join(BRAND, "calmfold.ico")
    fit(tile, 1024, 0.96).save(ico, sizes=sizes)
    print("wrote", ico)

    # Web.
    fav = os.path.join(BRAND, "favicon")
    for s in (16, 32, 48):
        save(fit(tile, s, 1.0), fav, f"favicon-{s}.png")
    fit(tile, 256, 1.0).save(os.path.join(fav, "favicon.ico"), sizes=[(16, 16), (32, 32), (48, 48)])
    for s in (192, 512):
        save(fit(tile, s, 0.9), fav, f"icon-{s}.png")
    # Apple touch icon: opaque (iOS adds its own rounding).
    touch = Image.new("RGBA", (180, 180), CREAM + (255,))
    touch.alpha_composite(fit(tile, 180, 0.8))
    save(touch.convert("RGB"), fav, "apple-touch-icon.png")
    with open(os.path.join(fav, "site.webmanifest"), "w") as f:
        f.write(
            '{\n  "name": "Calmfold",\n  "short_name": "Calmfold",\n'
            '  "icons": [\n'
            '    { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },\n'
            '    { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" }\n'
            "  ],\n"
            '  "theme_color": "#2E2F33",\n  "background_color": "#FAF6F0",\n'
            '  "display": "standalone"\n}\n'
        )

    # In-app copy (About screen).
    save(fit(tile, 256, 1.0), ROOT, "crates", "ui", "assets", "brand", "icon-256.png")
    print("done")


if __name__ == "__main__":
    main()
