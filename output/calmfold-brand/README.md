# Calmfold artwork — Nexdark Labs

One generated master, with repeatable scripted exports to preserve the same symbol.
These are design assets for review; they have not been installed into the app.

## Files

- `icon-master.png`: 1024 × 1024, off-white background.
- `icon-mac.png`: 1024 × 1024, 824 × 824 rounded tile, transparent surround and soft shadow.
- `icon-windows.png`: 1024 × 1024, symbol only, transparent background.
- `icon-mono.png` / `icon-mono-white.png`: black/white flat marks, transparent.
- `logo-horizontal.png` / `logo-horizontal-dark.png`: 2000 × 560, transparent, dark/light Inter Medium text.
- `wordmark.png` / `wordmark-dark.png`: 2000 × 440, transparent.
- `favicon-16.png`, `favicon-32.png`, and larger sizes: simplified black marks; matching `-white` versions for dark backgrounds.
- `favicon.ico`: multi-resolution favicon.
- `platform-mockup.png`: illustrative Dock/taskbar strips on light/dark backgrounds, plus small-size checks. Not actual OS screenshots.
- `source/icon-master-original.png`: original generated artwork retained unchanged.
- `manifest.json`: output image dimensions and modes.

## Rebuild

Run `python3 output/calmfold-brand/export_brand.py` from the repository root with Pillow installed. It uses the repository's bundled Inter fonts. No generation calls or network access are needed to rebuild the variants. The silhouette mask is tied to this master; replacing the artwork requires adjusting the mask. Transparent edges use a traced silhouette, so inspect against the intended background before publishing.

The icon exports are raster PNGs. The small monochrome variants deliberately simplify the fold into negative space. Platform mockups show the intended sizes at 100% image scale; actual display scaling may differ. These files are not a claim of Apple or Microsoft certification.

## Generation prompt

Use case: logo-brand. Create ONE final app-icon image, exactly 1024 x 1024, no presentation board, no words. Brand: Calmfold by Nexdark Labs, a calm private native personal workspace. Design an original premium minimalist symbol: a gently folded sheet forming a compact balanced near-square silhouette, just 2–3 large geometric planes, rounded outer corners and one readable diagonal fold. It must be recognisable at tiny sizes, no hairline strokes, no letter or monogram, not a paper airplane, not an envelope. Charcoal main plane, warm grey folded plane, off-white highlight, extremely restrained muted sage on one plane if needed. Flat vector-like graphic with subtle tonal depth only, no clay, no photorealism, no glow, no dramatic gradient. Center the folded symbol with generous breathing space on a full-bleed soft off-white square background tile. Refined like a considered macOS productivity icon. This is the master to derive a consistent eight-asset family from. No mockup, no text, no extra icons.

The generator returned 1254 × 1254; the scripted master export normalizes it to 1024 × 1024. A separately generated macOS experiment was discarded in favor of deterministic variants from this master.
