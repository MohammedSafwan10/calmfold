"""Deterministic exports from the generated Calmfold master.

Run from the repository root with Python + Pillow. No AI calls, no network.
The mask follows the master silhouette; mono/favicons preserve the diagonal
fold as negative space rather than relying on grey shading at tiny sizes.
"""
from pathlib import Path
import json
import shutil
from PIL import Image, ImageDraw, ImageFont, ImageFilter

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
SOURCE = OUT / 'source' / 'icon-master-original.png'
FONT = ROOT / 'crates/ui/assets/fonts/inter/Inter-Medium.ttf'
REGULAR = ROOT / 'crates/ui/assets/fonts/inter/Inter-Regular.ttf'
R = Image.Resampling.LANCZOS


def curve(points, a, b, c, d, count=48):
    for i in range(1, count + 1):
        t = i / count
        points.append(tuple((1-t)**3*a[k] + 3*(1-t)**2*t*b[k]
                            + 3*(1-t)*t*t*c[k] + t**3*d[k] for k in (0, 1)))


def silhouette():
    p = [(282, 480)]
    curve(p, p[-1], (282, 365), (345, 299), (448, 298))
    p.append((797, 297))
    curve(p, p[-1], (910, 297), (967, 350), (967, 469))
    p.append((967, 800))
    curve(p, p[-1], (967, 917), (912, 978), (798, 978))
    p.append((454, 978))
    curve(p, p[-1], (338, 978), (282, 922), (282, 800))
    return p


def canvas(size):
    return Image.new('RGBA', size, (0, 0, 0, 0))


def center(im, asset, xy):
    im.alpha_composite(asset, (round(xy[0]-asset.width/2), round(xy[1]-asset.height/2)))


def save(im, name):
    im.save(OUT / (name + '.png'), optimize=True)


def tracked(draw, xy, text, font, fill, tracking=1):
    x, y = xy
    baseline = y + font.getbbox(text)[3]
    for char in text:
        draw.text((x, baseline), char, font=font, fill=fill, anchor='ls')
        x += draw.textlength(char, font=font) + tracking
    return x


if not SOURCE.exists():
    SOURCE.parent.mkdir(exist_ok=True)
    shutil.copy2(OUT / 'icon-master.png', SOURCE)
original = Image.open(SOURCE).convert('RGBA')
assert original.size == (1254, 1254), 'Mask is tied to the original master size'
mask = Image.new('L', original.size, 0)
ImageDraw.Draw(mask).polygon(silhouette(), fill=255)
mask = mask.filter(ImageFilter.MinFilter(5))
original.putalpha(mask)
bbox = mask.getbbox()
symbol = original.crop(bbox)
mono_mask = mask.copy()
d = ImageDraw.Draw(mono_mask)
# Large, deliberate negative-space folds survive reduction to taskbar sizes.
d.line([(353, 947), (924, 407)], fill=0, width=27)
d.line([(701, 635), (908, 635)], fill=0, width=22)
mono_mask = mono_mask.crop(bbox)
mono = Image.new('RGBA', symbol.size, (0, 0, 0, 255))
mono.putalpha(mono_mask)
mono_white = Image.new('RGBA', symbol.size, (255, 255, 255, 255))
mono_white.putalpha(mono_mask)

save(Image.open(SOURCE).convert('RGBA').resize((1024, 1024), R), 'icon-master')
mac = canvas((1024, 1024))
tile_mask = Image.new('L', mac.size, 0)
ImageDraw.Draw(tile_mask).rounded_rectangle((100, 100, 923, 923), radius=185, fill=255)
shadow = Image.new('L', mac.size, 0)
ImageDraw.Draw(shadow).rounded_rectangle((101, 114, 922, 937), radius=185, fill=38)
shadow = shadow.filter(ImageFilter.GaussianBlur(13))
shade = Image.new('RGBA', mac.size, (20, 19, 18, 0))
shade.putalpha(shadow)
mac.alpha_composite(shade)
tile = Image.new('RGBA', mac.size, (248, 245, 239, 255))
tile.putalpha(tile_mask)
mac.alpha_composite(tile)
center(mac, symbol.resize((602, 598), R), (512, 512))
save(mac, 'icon-mac')
windows = canvas((1024, 1024))
center(windows, symbol.resize((820, 815), R), (512, 512))
save(windows, 'icon-windows')
for name, mark in [('icon-mono', mono), ('icon-mono-white', mono_white)]:
    im = canvas((1024, 1024))
    center(im, mark.resize((820, 815), R), (512, 512))
    save(im, name)

# A reduced two-plane mark: no tiny third fold, an open diagonal at pixel scale.
small_mask = mask.crop(bbox)
ds = ImageDraw.Draw(small_mask)
ds.line([(70, 650), (650, 100)], fill=0, width=66)
for size in (16, 32, 48, 64, 256):
    for dark in (False, True):
        glyph = Image.new('RGBA', small_mask.size, (255, 255, 255, 255) if dark else (0, 0, 0, 255))
        glyph.putalpha(small_mask)
        im = canvas((size, size))
        center(im, glyph.resize((size-2, size-2), R), (size/2, size/2))
        save(im, f'favicon-{size}' + ('-white' if dark else ''))
shutil.copy2(OUT / 'favicon-256.png', OUT / 'favicon.png')
Image.open(OUT/'favicon-256.png').save(OUT/'favicon.ico', sizes=[(16,16),(32,32),(48,48),(64,64),(256,256)])

for dark in (False, True):
    im = canvas((2000, 560))
    center(im, symbol.resize((310, 308), R), (285, 280))
    draw = ImageDraw.Draw(im)
    tracked(draw, (510, 194), 'Calmfold', ImageFont.truetype(str(FONT), 190),
            (245,243,238) if dark else (37,38,39), tracking=2)
    save(im, 'logo-horizontal-dark' if dark else 'logo-horizontal')
    word = canvas((2000, 440))
    draw = ImageDraw.Draw(word)
    font = ImageFont.truetype(str(FONT), 230)
    width = sum(draw.textlength(c, font=font) for c in 'Calmfold') + 2*7
    tracked(draw, ((2000-width)/2, 104), 'Calmfold', font,
            (245,243,238) if dark else (37,38,39), tracking=2)
    save(word, 'wordmark-dark' if dark else 'wordmark')

# Exact-pixel illustrative strips; these are mockups, never OS screenshots.
board = Image.new('RGBA', (2000, 1420), '#eeece7')
draw = ImageDraw.Draw(board)
font = lambda size: ImageFont.truetype(str(FONT), size)
reg = lambda size: ImageFont.truetype(str(REGULAR), size)
draw.text((80, 55), 'Calmfold', font=font(58), fill='#242628')
draw.text((82, 133), 'Nexdark Labs  /  Platform icon study', font=reg(24), fill='#676866')
for idx, (platform, dark) in enumerate([('macOS Dock',False),('macOS Dock',True),('Windows taskbar',False),('Windows taskbar',True)]):
    x = 70 + (idx%2)*950
    y = 220 + (idx//2)*360
    bg = '#17191c' if dark else '#faf9f6'
    fg = '#eeece7' if dark else '#25272a'
    draw.rounded_rectangle((x,y,x+910,y+310), radius=25, fill=bg)
    draw.text((x+30,y+26), platform + (' · dark' if dark else ' · light'), font=font(24),fill=fg)
    if idx < 2:
        draw.rounded_rectangle((x+125,y+135,x+785,y+227),radius=23,fill='#33363a' if dark else '#e4e3df')
        for k in range(7):
            cx=x+194+k*87
            if k == 3:
                center(board,mac.resize((64,64),R),(cx,y+178))
                draw.ellipse((cx-2,y+216,cx+2,y+220),fill=fg)
            else:
                draw.rounded_rectangle((cx-23,y+155,cx+23,y+201),radius=10,fill=['#82b3d1','#9bb2a3','#c5ad8c','#a4a5ad','#b9a7a2','#9fa8bb','#afba9a'][k])
        draw.text((x+30,y+268),'Calmfold at 64 px · center icon',font=reg(19),fill=fg)
    else:
        draw.rectangle((x+20,y+145,x+890,y+203),fill='#292b2e' if dark else '#e6e5e1')
        for k in range(7):
            cx=x+300+k*49
            if k == 3:
                center(board,windows.resize((32,32),R),(cx,y+174))
                draw.rounded_rectangle((cx-7,y+197,cx+7,y+200),radius=1,fill='#86a9c9')
            else:
                draw.rounded_rectangle((cx-11,y+163,cx+11,y+185),radius=5,fill='#8998a5' if dark else '#747e85')
        draw.text((x+30,y+268),'Calmfold at 32 px · center icon',font=reg(19),fill=fg)
draw.text((80,1000),'Small-size checks — actual pixels, plus enlarged inspection',font=font(28),fill='#25272a')
for j,dark in enumerate((False,True)):
    x=80+j*950
    draw.rounded_rectangle((x,1060,x+900,1290),radius=20,fill='#151719' if dark else '#ffffff')
    fg='#eeeae4' if dark else '#25272a'
    for k,size in enumerate((16,32)):
        glyph=Image.open(OUT/(f'favicon-{size}'+('-white' if dark else '')+'.png')).convert('RGBA')
        center(board,glyph,(x+65+k*130,1140))
        draw.text((x+45+k*130,1190),f'{size} px',font=reg(19),fill=fg)
        center(board,glyph.resize((128,128),Image.Resampling.NEAREST),(x+440+k*220,1165))
draw.text((80,1340),'Illustrative UI strips, not operating-system screenshots. View at 100% for pixel-size judging.',font=reg(20),fill='#676866')
save(board,'platform-mockup')
manifest={p.name:{'size':list(Image.open(p).size),'mode':Image.open(p).mode} for p in sorted(OUT.glob('*.png'))}
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps(manifest,indent=2))
