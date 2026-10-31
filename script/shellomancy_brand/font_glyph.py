import sys, os, glob
sys.path.insert(0, os.path.dirname(__file__))
from mark import polygons, fit
from fontTools.ttLib import TTFont
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables._g_l_y_f import flagOverlapSimple
REPO = sys.argv[1]
SMALL = dict(stroke=58, septum=20, step=1.25, cursor=False)
BOX = (85, 0, 1963, 1502)
for path in sorted(glob.glob(os.path.join(REPO, 'app/assets/bundled/fonts/roboto/*.ttf'))):
    f = TTFont(path, recalcTimestamp=False)
    name = f.getBestCmap()[0xE500]
    old = f['glyf'][name]; adv, _ = f['hmtx'][name]
    # Fixed box: the original Warp glyph's bounds (baseline to ~cap height).
    # Not read from the font, so re-running never shrinks the glyph.
    polys = fit(polygons(**SMALL), BOX, flip_y=True)
    pen = TTGlyphPen(None)
    for p in polys:
        pts = [(round(x), round(y)) for x, y in p]
        pen.moveTo(pts[0])
        for q in pts[1:]: pen.lineTo(q)
        pen.closePath()
    g = pen.glyph()
    g.flags[0] |= flagOverlapSimple
    g.recalcBounds(f['glyf'])
    f['glyf'][name] = g
    f['hmtx'][name] = (adv, g.xMin)
    f.save(path)
    print(os.path.basename(path), name, 'adv', adv, 'bbox', (g.xMin, g.yMin, g.xMax, g.yMax), 'contours', g.numberOfContours)
