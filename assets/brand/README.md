# Ottero brand artwork

Ottero by Nexdark Labs. The master is `source/icon-master.png`; everything
else is generated from it:

    uv run scripts/gen-icons.py      # all sizes, Ottero.icns, ottero.ico, favicons
    scripts/bundle-mac.sh            # target/release/Ottero.app (unsigned)

Windows embeds `ottero.ico` into ottero.exe at build time
(`apps/desktop/build.rs`). The About screen uses `crates/ui/assets/brand/icon-256.png`.
Optional extra artwork (logos, mono symbol) can be added with these names:

| File | What | Size |
| --- | --- | --- |
| `source/icon-master.png` | App icon, full colour, transparent outside the rounded shape | 1024×1024 |
| `source/icon-master.svg` | Same, as vector (if you have it) | — |
| `source/icon-mac.png` | macOS version: the shape inside Apple's grid (about 824×824 squircle centred, soft shadow) | 1024×1024 |
| `source/icon-windows.png` | Windows version: no squircle, the symbol fills more of the square, transparent background | 1024×1024 |
| `source/icon-mono.svg` | One-colour symbol only (menu bar / tray, monochrome contexts) | — |
| `source/logo-horizontal.svg` (or `.png`, 2000 px wide) | Symbol + "Ottero" wordmark side by side | — |
| `source/logo-horizontal-dark.svg` | Same for dark backgrounds | — |
| `source/wordmark.svg` | "Ottero" text only | — |
| `favicon/favicon.svg` | Simplified symbol that reads at 16 px | — |
| `png/` | Leave empty: generated sizes go here | — |

Rules:
- No text inside the app icon.
- The symbol must still read at 16×16 (favicon, taskbar).
- Light and dark: the icon must look good on both a white and a black background.
