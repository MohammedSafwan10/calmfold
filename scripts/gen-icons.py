#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow>=11", "numpy>=2"]
# ///
"""Builds every Ottero icon file from assets/brand/source/icon-master.png.

    uv run scripts/gen-icons.py

The master is the approved Quiet Loop artwork: the charcoal otter on a flat
ivory square (assets/brand/ottero/README.md). This script:
1. isolates the otter as a clean alpha mask (its ivory eye stays a hole),
2. recomposes it on a rounded ivory tile at every size; small sizes give the
   otter more of the tile so it still reads at 16 px,
3. a macOS icon on Apple's grid (824 px tile in 1024, soft shadow) and Ottero.icns,
4. a Windows icon (tile fills the square) and ottero.ico,
5. web favicons and touch icons (assets/brand/favicon/),
6. symbol-only files: transparent PNGs in charcoal and ivory, and a traced
   one-colour SVG (needs `potrace`, `brew install potrace`),
7. the in-app copy used on the About screen (crates/ui/assets/brand/).
Re-run it whenever the master changes.
"""

import os
import shutil
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BRAND = os.path.join(ROOT, "assets", "brand")
MASTER = os.path.join(BRAND, "source", "icon-master.png")
IVORY = (248, 242, 233)
CHARCOAL = (48, 48, 50)


def otter_mask(path):
    """The otter as alpha (1 = charcoal), cropped to its own square box."""
    img = np.asarray(Image.open(path).convert("L")).astype(np.float64)
    bg = np.median(np.concatenate([img[:8].ravel(), img[-8:].ravel()]))
    ink = np.percentile(img, 2)
    alpha = np.clip((bg - img) / max(bg - ink, 1), 0, 1)
    alpha[alpha < 0.04] = 0
    ys, xs = np.where(alpha > 0.5)
    cx, cy = (xs.min() + xs.max()) / 2, (ys.min() + ys.max()) / 2
    side = max(xs.max() - xs.min(), ys.max() - ys.min()) + 8
    box = tuple(round(v) for v in (cx - side / 2, cy - side / 2, cx + side / 2, cy + side / 2))
    return Image.fromarray((alpha * 255).astype(np.uint8), "L").crop(box)


def squircle(size, n=5.0):
    """An Apple-like superellipse mask, supersampled for soft edges."""
    s = size * 4
    t = (np.arange(s) + 0.5) / s * 2 - 1
    x, y = np.meshgrid(t, t)
    inside = (np.abs(x) ** n + np.abs(y) ** n) <= 1
    return Image.fromarray((inside * 255).astype(np.uint8), "L").resize((size, size), Image.LANCZOS)


def tile(mask, size, ground=IVORY, ink=CHARCOAL, share=None):
    """The otter centred on a rounded tile of `size` px."""
    share = share or (0.78 if size <= 48 else 0.68)
    out = Image.new("RGBA", (size, size), ground + (255,))
    m = mask.resize((round(size * share),) * 2, Image.LANCZOS)
    off = (size - m.width) // 2
    out.paste(Image.new("RGBA", m.size, ink + (255,)), (off, off), m)
    out.putalpha(squircle(size))
    return out


def symbol(mask, size, ink):
    """The otter alone on transparency."""
    m = mask.resize((size, size), Image.LANCZOS)
    out = Image.new("RGBA", (size, size), ink + (0,))
    out.putalpha(m)
    return out


def pad(img, canvas, fill):
    out = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    t = img.resize((round(canvas * fill),) * 2, Image.LANCZOS)
    off = (canvas - t.width) // 2
    out.alpha_composite(t, (off, off))
    return out


def mac_icon(mask):
    """Apple's grid: 824 px tile in 1024 with a soft drop shadow."""
    art = pad(tile(mask, 824), 1024, 824 / 1024)
    shadow = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    shadow.putalpha(art.getchannel("A").filter(ImageFilter.GaussianBlur(14)).point(lambda a: int(a * 0.28)))
    out = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    out.alpha_composite(shadow, (0, 10))
    out.alpha_composite(art)
    return out


def save(img, *parts):
    path = os.path.join(*parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    img.save(path, optimize=True)
    return path


def trace(mask, out):
    """One-colour SVG of the otter (eye as a hole) through potrace."""
    if not shutil.which("potrace"):
        print("skipped", out, "(install potrace)")
        return
    with tempfile.TemporaryDirectory() as tmp:
        pbm = os.path.join(tmp, "otter.pbm")
        big = mask.resize((1600, 1600), Image.LANCZOS).point(lambda a: 0 if a > 127 else 255).convert("1")
        big.save(pbm)
        subprocess.run(["potrace", pbm, "-s", "-o", out, "--flat", "-t", "6", "-O", "0.4",
                        "-C", "#303032"], check=True)
    print("wrote", out)


def main():
    if not os.path.exists(MASTER):
        sys.exit(f"missing {MASTER}")
    mask = otter_mask(MASTER)

    for s in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        save(tile(mask, s), BRAND, "png", f"icon-{s}.png")
    save(tile(mask, 1024), BRAND, "source", "icon-tile.png")
    save(tile(mask, 1024, ground=CHARCOAL, ink=IVORY), BRAND, "source", "icon-tile-dark.png")
    save(symbol(mask, 1024, CHARCOAL), BRAND, "source", "symbol.png")
    save(symbol(mask, 1024, IVORY), BRAND, "source", "symbol-light.png")
    trace(mask, os.path.join(BRAND, "source", "symbol.svg"))

    mac = mac_icon(mask)
    save(mac, BRAND, "source", "icon-mac.png")
    if sys.platform == "darwin" and shutil.which("iconutil"):
        with tempfile.TemporaryDirectory() as tmp:
            iconset = os.path.join(tmp, "Ottero.iconset")
            os.makedirs(iconset)
            for s in (16, 32, 128, 256, 512):
                mac.resize((s, s), Image.LANCZOS).save(os.path.join(iconset, f"icon_{s}x{s}.png"))
                mac.resize((s * 2, s * 2), Image.LANCZOS).save(os.path.join(iconset, f"icon_{s}x{s}@2x.png"))
            out = os.path.join(BRAND, "Ottero.icns")
            subprocess.run(["iconutil", "-c", "icns", iconset, "-o", out], check=True)
            print("wrote", out)

    # Windows: each size drawn on its own so the otter stays crisp when small.
    save(tile(mask, 256), BRAND, "source", "icon-windows.png")
    ico = os.path.join(BRAND, "ottero.ico")
    sizes = [16, 20, 24, 32, 40, 48, 64, 256]
    frames = [tile(mask, s) for s in sizes]
    frames[-1].save(ico, sizes=[(s, s) for s in sizes], append_images=frames[:-1])
    print("wrote", ico)

    fav = os.path.join(BRAND, "favicon")
    for s in (16, 32, 48):
        save(tile(mask, s), fav, f"favicon-{s}.png")
    small = [tile(mask, s) for s in (16, 32, 48)]
    small[-1].save(os.path.join(fav, "favicon.ico"), sizes=[(16, 16), (32, 32), (48, 48)], append_images=small[:-1])
    for s in (192, 512):
        save(tile(mask, s), fav, f"icon-{s}.png")
    # Apple touch icon: opaque square (iOS rounds it itself).
    touch = Image.new("RGBA", (180, 180), IVORY + (255,))
    m = mask.resize((122, 122), Image.LANCZOS)
    touch.paste(Image.new("RGBA", m.size, CHARCOAL + (255,)), (29, 29), m)
    save(touch.convert("RGB"), fav, "apple-touch-icon.png")
    with open(os.path.join(fav, "site.webmanifest"), "w") as f:
        f.write(
            '{\n  "name": "Ottero",\n  "short_name": "Ottero",\n'
            '  "icons": [\n'
            '    { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },\n'
            '    { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" }\n'
            "  ],\n"
            '  "theme_color": "#303032",\n  "background_color": "#F8F2E9",\n'
            '  "display": "standalone"\n}\n'
        )

    save(tile(mask, 256), ROOT, "crates", "ui", "assets", "brand", "icon-256.png")
    print("done")


if __name__ == "__main__":
    main()
