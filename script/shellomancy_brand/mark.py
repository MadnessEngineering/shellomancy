"""Shellomancy nautilus mark as filled outlines (no strokes), for tinted icons,
loading frames, and font glyphs. Geometry matches the app icon (concept A)."""
import math

B = math.log(3) / (2 * math.pi)   # nautilus growth: 3x radius per turn
A0 = 13.0
TMAX = 6.1 * math.pi
N = 700

def _raw(t):
    r = A0 * math.exp(B * t)
    return r * math.cos(t), r * math.sin(t)

def _norm(t):
    # unit normal of the spiral (pointing outward-ish), numerically
    e = 1e-4
    x1, y1 = _raw(t - e); x2, y2 = _raw(t + e)
    dx, dy = x2 - x1, y2 - y1; L = math.hypot(dx, dy)
    return dy / L, -dx / L

def _area(poly):
    return sum(x1 * y2 - x2 * y1 for (x1, y1), (x2, y2) in zip(poly, poly[1:] + poly[:1])) / 2

def _ccw(poly):
    return poly if _area(poly) > 0 else poly[::-1]

def _circle(cx, cy, r, n=24):
    return [(cx + r * math.cos(2 * math.pi * i / n), cy + r * math.sin(2 * math.pi * i / n)) for i in range(n)]

def _band(pts, widths):
    """Outline a polyline with per-point half-widths."""
    left, right = [], []
    for i, ((x, y), hw) in enumerate(zip(pts, widths)):
        a = pts[max(i - 1, 0)]; b = pts[min(i + 1, len(pts) - 1)]
        dx, dy = b[0] - a[0], b[1] - a[1]; L = math.hypot(dx, dy) or 1
        nx, ny = -dy / L, dx / L
        left.append((x + nx * hw, y + ny * hw)); right.append((x - nx * hw, y - ny * hw))
    return left + right[::-1]

def polygons(stroke=26.0, septum=9.0, cursor=True, septa=True, step=0.42):
    """Return list of CCW polygons in raw spiral units (same scale as the icon SVG)."""
    ts = [i * TMAX / N for i in range(N + 1)]
    pts = [_raw(t) for t in ts]
    # taper the spiral: thinner at the core so the inner turns stay open
    rmax = A0 * math.exp(B * TMAX)
    widths = [max(4.0, stroke / 2 * min(1.0, 0.35 + 0.65 * (A0 * math.exp(B * t) / rmax) ** 0.5)) for t in ts]
    polys = [_ccw(_band(pts, widths))]
    polys.append(_ccw(_circle(*pts[-1], widths[-1])))        # round outer cap
    if septa:
        t = TMAX - 0.45
        while t > TMAX - 2 * math.pi + 0.2:
            x1, y1 = _raw(t); x2, y2 = _raw(t - 2 * math.pi)
            mx, my = (x1 + x2) / 2, (y1 + y2) / 2
            qx, qy = mx * 0.78, my * 0.78          # bow toward spiral origin (0,0)
            curve = [((1 - u) ** 2 * x1 + 2 * (1 - u) * u * qx + u * u * x2,
                      (1 - u) ** 2 * y1 + 2 * (1 - u) * u * qy + u * u * y2) for u in [k / 16 for k in range(17)]]
            polys.append(_ccw(_band(curve, [septum / 2] * len(curve))))
            t -= step
        # aperture lip closes the outer whorl
        x1, y1 = _raw(TMAX); x2, y2 = _raw(TMAX - 2 * math.pi)
        polys.append(_ccw(_band([(x1, y1), (x2, y2)], [septum / 2] * 2)))
    if cursor:
        polys.append(_ccw([(-8, -13), (8, -13), (8, 13), (-8, 13)]))
    return polys

def bbox(polys):
    xs = [x for p in polys for x, _ in p]; ys = [y for p in polys for _, y in p]
    return min(xs), min(ys), max(xs), max(ys)

def fit(polys, box, rot_deg=0.0, flip_y=False, spin=False):
    """Scale+center polys into box=(x0,y0,x1,y1), optional rotation about mark center."""
    x0, y0, x1, y1 = bbox(polys)
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    a = math.radians(rot_deg); ca, sa = math.cos(a), math.sin(a)
    rp = [[(cx + (x - cx) * ca - (y - cy) * sa, cy + (x - cx) * sa + (y - cy) * ca) for x, y in p] for p in polys]
    # scale from the unrotated bbox so rotation frames stay the same size
    s = min((box[2] - box[0]) / (x1 - x0), (box[3] - box[1]) / (y1 - y0))
    if spin:
        rad = max(math.hypot(x - cx, y - cy) for p in polys for x, y in p)
        s = min(box[2] - box[0], box[3] - box[1]) / (2 * rad)
    bx, by = (box[0] + box[2]) / 2, (box[1] + box[3]) / 2
    out = [[(bx + (x - cx) * s, by + (y - cy) * s) for x, y in p] for p in rp]
    if flip_y:
        out = [[(x, box[1] + box[3] - y) for x, y in p][::-1] for p in out]
    return out

def path_d(polys, nd=2):
    f = f"{{:.{nd}f}}"
    return " ".join("M" + " L".join(f.format(x) + " " + f.format(y) for x, y in p) + " Z" for p in polys)
