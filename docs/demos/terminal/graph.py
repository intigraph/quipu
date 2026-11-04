#!/usr/bin/env python3
"""Draw the graph before and after the second fact, from the recorded native reads."""
import hashlib
import html
import json
from pathlib import Path

root = Path(__file__).resolve().parent
manifest = root / "manifest.json"
m = json.loads(manifest.read_text())
# Fixed knot positions, so the second picture differs only by what was added.
SPOTS = {"Ada": 200, "Engine": 500, "Babbage": 800}


def rows_of(index, expected):
    result = m["commands"][index]
    assert result["exit"] == 0 and result["args"][0] == "read"
    rows = [tuple(v.removeprefix("urn:demo:") for v in line.split("\t"))
            for line in result["stdout"].splitlines() if line.startswith("urn:demo:")]
    assert rows == expected, f"unexpected query result at command {index}: {rows}"
    return rows


def draw(title, desc, rows, caption):
    knots = sorted({n for s, _, o in rows for n in (s, o)}, key=SPOTS.__getitem__)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="420" viewBox="0 0 1000 420" '
           f'role="img" aria-labelledby="title desc">',
           f'<title id="title">{html.escape(title)}</title><desc id="desc">{html.escape(desc)}</desc>',
           '<rect width="1000" height="420" rx="16" fill="#101827"/>',
           '<g fill="none" stroke-width="4">']
    for s, p, o in rows:
        a, b = SPOTS[s], SPOTS[o]
        bend = 90 if a < b else 320
        out.append(f'<path d="M{a} 205C{a + (b - a) / 3:.0f} {bend} {a + 2 * (b - a) / 3:.0f} {410 - bend} {b} 205" '
                   f'stroke="#6ee7b7"/>')
    for name in knots:
        x = SPOTS[name]
        out.append(f'<path d="M{x} 95V315" stroke="#475569"/><circle cx="{x}" cy="205" r="12" stroke="#6ee7b7"/>')
    out.append('</g><g fill="#f1f5f9" font-family="DejaVu Sans Mono, Menlo, monospace" font-size="28" text-anchor="middle">')
    for name in knots:
        out.append(f'<text x="{SPOTS[name]}" y="78">{html.escape(name)}</text>')
    for s, p, o in rows:
        out.append(f'<text x="{(SPOTS[s] + SPOTS[o]) / 2:.0f}" y="{150 if SPOTS[s] < SPOTS[o] else 290}" '
                   f'fill="#6ee7b7" font-size="24">{html.escape(p)}</text>')
    out.append(f'<text x="500" y="380" font-size="24">{html.escape(caption)}</text></g></svg>\n')
    return "\n".join(out)


before = rows_of(6, [("Ada", "studies", "Engine")])
after = rows_of(5, [("Ada", "studies", "Engine"), ("Babbage", "designed", "Engine")])
svgs = {
    "graph-before.svg": draw("The graph after one fact", "The native read at transaction 1 returns Ada studies Engine.",
                             before, "tx 1: one fact"),
    "graph-after.svg": draw("The graph after two facts",
                            "The current native read returns Ada studies Engine and Babbage designed Engine.",
                            after, "tx 2: Babbage joins the Engine"),
}
for name, svg in svgs.items():
    (root / name).write_text(svg)
(root / "graph.provenance.json").write_text(json.dumps({
    "input_manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
    "source": "native read stdout: command 6 (--tx 1) for before, command 5 for after; exact row assertions",
    "before_rows": before, "after_rows": after,
    "graph_sha256": {n: hashlib.sha256(s.encode()).hexdigest() for n, s in svgs.items()}}, indent=2) + "\n")
