#!/usr/bin/env python3
"""Round 2, supergraphics: qp-super.pxl. Big flat fields, bright bands, full bleed.

q and p flipped top-to-bottom are d and b, so the shadow spells db.
Bitmaps are drawn here and run-length encoded into pixelsrc rect unions.
"""
import json

W = H = 64
T, BOWL, DESC = 6, 20, 8
TALL = BOWL + DESC

def glyphs(x0, knots=True):
    """{(x,y): (kind, stripe)}: kind body|knot, stripe 0..2 counted from the outer edge."""
    px = {}
    def letter(bx, stem_right):
        sx = bx + BOWL - T if stem_right else bx
        for y in range(BOWL):
            for x in range(bx, bx + BOWL):
                dx, dy = x - bx, y
                depth = min(dx, dy, BOWL - 1 - dx, BOWL - 1 - dy)
                if depth >= T:
                    continue
                far = (BOWL - 1 - dx) if stem_right else dx      # distance from the stem side
                cx, cy = min(dx, BOWL - 1 - dx), min(dy, BOWL - 1 - dy)
                if cx + cy < 3 and far >= T:                      # chamfer the two free corners
                    continue
                px[(x, y)] = ("body", depth // 2)
        for y in range(BOWL - T, TALL):                           # stem stripes run straight down
            for x in range(sx, sx + T):
                edge = (sx + T - 1 - x) if stem_right else (x - sx)
                px[(x, y)] = ("body", edge // 2)
        return sx
    qs = letter(x0, True)
    ps = letter(x0 + BOWL + 4, False)
    if knots:
        for sx, rows in ((qs, (BOWL + 1, BOWL + 5)), (ps, (BOWL + 3,))):
            for y0 in rows:
                for y in (y0, y0 + 1):
                    for x in range(sx - 1, sx + T + 1):
                        px[(x, y)] = ("knot", 0)
    return px

def shadow(grid, stems, ground, sh_h, shear, tok):
    for r in range(sh_h):
        src = TALL - 1 - (r * TALL) // sh_h
        dx = -(r // shear) if shear else 0
        for (x, y) in stems:
            if y == src and 0 <= x + dx < W and ground + r < H:
                grid[ground + r][x + dx] = tok(r) if callable(tok) else tok

def fields(grid, ground, wall, floor):
    for y in range(H):
        for x in range(W):
            grid[y][x] = wall if y < ground else floor

def build(name):
    grid = [["_"] * W for _ in range(H)]
    if name == "qp-sunset":      # the shadow recedes in sunset bands
        x0, y0 = 10, 4; g = y0 + TALL
        fields(grid, g, "wall", "floor")
        bands = ["band1", "band2", "band3", "band4"]
        shadow(grid, glyphs(x0, False), g, TALL, 0, lambda r: bands[min(r // 7, 3)])
        paint = lambda k, s: "knot" if k == "knot" else "body"
    elif name == "qp-echo":      # a cast shadow in three bands
        x0, y0 = 18, 5; g = y0 + TALL
        fields(grid, g, "wall", "floor")
        bands = ["band1", "band2", "band3"]
        shadow(grid, glyphs(x0, False), g, 24, 2, lambda r: bands[r // 8])
        paint = lambda k, s: "knot" if k == "knot" else "body"
    elif name == "qp-sun":       # a red sun on the horizon behind, a black shadow in front
        x0, y0 = 10, 4; g = y0 + TALL
        fields(grid, g, "wall", "floor")
        for y in range(g):
            for x in range(W):
                if (x - 31.5) ** 2 + (y - g) ** 2 <= 27 ** 2:
                    grid[y][x] = "sun"
        shadow(grid, glyphs(x0, False), g, TALL, 0, "shadow")
        paint = lambda k, s: "knot" if k == "knot" else "body"
    else:                        # qp-lines: three-line lettering, shadow in the wall colour
        x0, y0 = 10, 4; g = y0 + TALL
        fields(grid, g, "wall", "floor")
        shadow(grid, glyphs(x0, False), g, TALL, 0, "wall")
        paint = lambda k, s: "knot" if k == "knot" else ("line1", "line2", "line3")[s]
    for (x, y), (k, s) in glyphs(x0).items():
        grid[y0 + y][x] = paint(k, s)
    return grid

def rle(grid):
    out, open_runs = {}, {}
    for y in range(H + 1):
        runs = set()
        if y < H:
            x = 0
            while x < W:
                t = grid[y][x]; s = x
                while x < W and grid[y][x] == t:
                    x += 1
                if t != "_":
                    runs.add((t, s, x - s))
        for key in sorted(open_runs):
            if key not in runs:
                t, s, w = key; ys = open_runs.pop(key)
                out.setdefault(t, []).append({"rect": [s, ys, w, y - ys]})
        for key in sorted(runs):
            open_runs.setdefault(key, y)
    return out

BLUE, ORANGE, YELLOW, PINK, VIOLET, CREAM, NIGHT, RED, TEAL = (
    "#2340E8", "#FF5A1F", "#FFC914", "#FF2E88", "#7A2BF0", "#FFF4DC", "#14121F", "#F0302B", "#00C2A8")
PALETTES = {
    "qp-sunset": {"wall": BLUE, "floor": NIGHT, "band1": YELLOW, "band2": ORANGE, "band3": PINK, "band4": VIOLET, "body": CREAM, "knot": YELLOW},
    "qp-echo":   {"wall": ORANGE, "floor": YELLOW, "band1": PINK, "band2": BLUE, "band3": NIGHT, "body": CREAM, "knot": NIGHT},
    "qp-sun":    {"wall": CREAM, "floor": BLUE, "sun": RED, "shadow": NIGHT, "body": NIGHT, "knot": YELLOW},
    "qp-lines":  {"wall": NIGHT, "floor": TEAL, "line1": YELLOW, "line2": ORANGE, "line3": PINK, "knot": CREAM},
}
ROLE = lambda t: {"body": "anchor", "knot": "highlight", "shadow": "shadow"}.get(t, "anchor" if t.startswith("line") else "shadow" if t.startswith("band") else "fill")

lines = ["// qp casts db, supergraphics round. Generated by gen2.py."]
for name, cols in PALETTES.items():
    lines.append(json.dumps({"type": "palette", "name": name, "colors": {"_": "transparent", **cols},
                             "roles": {k: ROLE(k) for k in cols}}, separators=(",", ":")))
for name, cols in PALETTES.items():
    rects = rle(build(name))
    regions = {t: {"union": rects[t], "z": i + 1} for i, t in enumerate(cols) if t in rects}
    lines.append(json.dumps({"type": "sprite", "name": name, "size": [W, H], "palette": name, "regions": regions},
                            separators=(",", ":")))
open("qp-super.pxl", "w").write("\n".join(lines) + "\n")
