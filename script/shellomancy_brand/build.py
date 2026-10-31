import os, re, sys, subprocess, math
sys.path.insert(0, os.path.dirname(__file__))
from mark import polygons, fit, path_d, bbox
REPO = sys.argv[1]
SVG = os.path.join(REPO, 'app/assets/bundled/svg')
ICON = os.path.join(REPO, 'app/channels/oss/icon/shellomancy.svg')

SMALL = dict(stroke=58, septum=20, step=1.25, cursor=False)   # <= ~40px renders, font glyph
LARGE = dict(stroke=44, septum=15, step=0.42, cursor=True)    # logos

def svg_head(path):
    s = open(path).read()
    m = re.search(r'<svg[^>]*>', s)
    vb = [float(v) for v in re.search(r'viewBox="([^"]+)"', m.group(0)).group(1).split()]
    return s, m.group(0), vb

def first_fill(s):
    m = re.search(r'<path[^>]*?fill="([^"]+)"(?:[^>]*?fill-opacity="([^"]+)")?', s)
    return m.group(1), m.group(2)

def write_tinted(name, rot=None, pad=0.06):
    path = os.path.join(SVG, name)
    s, head, (x0, y0, w, h) = svg_head(path)
    fill, op = first_fill(s)
    box = (x0 + w * pad, y0 + h * pad, x0 + w * (1 - pad), y0 + h * (1 - pad))
    polys = fit(polygons(**SMALL), box, rot_deg=rot or 0.0, spin=rot is not None)
    opa = f' fill-opacity="{op}"' if op else ''
    open(path, 'w').write(f'{head}\n<path d="{path_d(polys)}" fill="{fill}"{opa}/>\n</svg>\n')

# 1. tinted marks (keep viewBox + fill colour so callers/tints are unchanged)
for n in ['warp.svg', 'warp-2.svg', 'warp-3.svg', 'warp-drive.svg', 'warp-logo-neutral.svg']:
    write_tinted(n)
# 2. loading animation: the shell turns 30 degrees per frame
for i in range(12):
    write_tinted(f'warp-loading-{i}.svg', rot=30.0 * i, pad=0.04)

# 3. square full-colour logos: keep the drop-shadow filter wrapper, swap the paths
for n in ['warp-logo-light.svg', 'warp-logo-dark.svg']:
    path = os.path.join(SVG, n)
    s, head, (x0, y0, w, h) = svg_head(path)
    fill, _ = first_fill(s)
    xs = [float(v) for v in re.findall(r'd="M([\d.]+)', s)]
    paths = re.findall(r'<path d="[^"]*"[^>]*/>', s)
    # logo region of the original: centre of the viewBox minus shadow margin
    box = (w * 0.16, h * 0.12, w * 0.84, h * 0.80)
    polys = fit(polygons(**LARGE), box)
    new = f'<path d="{path_d(polys)}" fill="{fill}"/>'
    s = s.replace(paths[0], new, 1)
    for p in paths[1:]:
        s = s.replace(p, '', 1)
    open(path, 'w').write(s)

# 4. logo + wordmark (About page)
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
font = TTFont(os.path.join(REPO, 'app/assets/bundled/fonts/roboto/Roboto-Medium.ttf'))
gs = font.getGlyphSet(); cmap = font.getBestCmap(); upm = font['head'].unitsPerEm
def wordmark(text, x, baseline, em):
    pen = SVGPathPen(gs); k = em / upm; cx = x
    for ch in text:
        g = cmap[ord(ch)]
        gs[g].draw(TransformPen(pen, (k, 0, 0, -k, cx, baseline)))
        cx += gs[g].width * k
    return pen.getCommands(), cx
for n, colour in [('warp-logo-with-light-title.svg', '#FAF9F6'), ('warp-logo-with-dark-title.svg', '#121212')]:
    H = 179.0
    polys = fit(polygons(**LARGE), (18, 26, 18 + 150, 26 + 127))
    d_text, x_end = wordmark('shellomancy', 196, 122, 104)
    W = math.ceil(x_end + 18)
    open(os.path.join(SVG, n), 'w').write(
        f'<svg width="{W}" height="{int(H)}" viewBox="0 0 {W} {int(H)}" fill="none" xmlns="http://www.w3.org/2000/svg">\n'
        f'<path d="{path_d(polys)}" fill="{colour}"/>\n<path d="{d_text}" fill="{colour}"/>\n</svg>\n')

# 5. app-icon palettes (Dock tile alternates + bundled PNGs)
base = open(ICON).read()
PAL = {
  'shellomancy': {},
  'abyss':  {'#2a1650':'#0b2a4a','#140a2a':'#061a30','#06030d':'#020a14','#7dffd9':'#8fe9ff','#22c9a0':'#2aa8e0','#c49bff':'#4f7dff','#7a3cff':'#2445b8','#3a1f70':'#12305a'},
  'ember':  {'#2a1650':'#3a1408','#140a2a':'#1f0a04','#06030d':'#0a0302','#7dffd9':'#ffd27a','#22c9a0':'#ff7a1a','#c49bff':'#ff5a3c','#7a3cff':'#b8281a','#3a1f70':'#4a1a0c'},
  'bone':   {'#2a1650':'#f7f0e1','#140a2a':'#e9dec6','#06030d':'#cdbb97','#7dffd9':'#3b2a1c','#22c9a0':'#22160d','#c49bff':'#9a7a58','#7a3cff':'#6b4e33','#3a1f70':'#d9c8a6','#ffffff':'#000000'},
  'mono':   {'#2a1650':'#1c1c1c','#140a2a':'#0e0e0e','#06030d':'#000000','#7dffd9':'#ffffff','#22c9a0':'#d6d6d6','#c49bff':'#9a9a9a','#7a3cff':'#6a6a6a','#3a1f70':'#2a2a2a'},
  'toxic':  {'#2a1650':'#0f2410','#140a2a':'#081608','#06030d':'#020802','#7dffd9':'#d4ff4a','#22c9a0':'#7cff2a','#c49bff':'#38c95a','#7a3cff':'#1d8a3a','#3a1f70':'#163a18'},
}
import tempfile; out = tempfile.mkdtemp(prefix='shellomancy_brand_'); os.makedirs(out, exist_ok=True)
def render(svgtext, dst, size=1024):
    tmp = os.path.join(out, 'tmp.svg'); open(tmp, 'w').write(svgtext)
    subprocess.run(['rsvg-convert', '-w', str(size), '-h', str(size), tmp, '-o', dst], check=True)
def recolour(m):
    s = base
    for a, b in m.items(): s = re.sub(a, b, s, flags=re.I)
    return s
res = os.path.join(REPO, 'app/DockTilePlugin/Resources')
for f in os.listdir(res):
    if f.endswith('.png'): os.remove(os.path.join(res, f))
for name, m in PAL.items():
    render(recolour(m), os.path.join(res, f'{name}.png'))
# bundled PNGs: local.png is the debug-build Dock icon; dev/blue keep their names for callers
for n in ['blue', 'dev', 'local']:
    render(base, os.path.join(REPO, f'app/assets/bundled/png/{n}.png'))
print('built')
