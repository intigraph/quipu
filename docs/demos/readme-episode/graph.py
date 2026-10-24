#!/usr/bin/env python3
"""Draw the demo store the way Quipu's Explore map draws a graph: as a quipu.

    python3 docs/demos/readme-episode/graph.py

Cords are type groups hanging from a main cord, knots are entities (sized by
how many links they have), and relations are threads between knots. Reads
shop.nt (the store's own `qp export`, written by record.py from the same run)
and writes graph.svg. Nothing is drawn that is not in the export.
"""

import html
import re
from collections import defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
NS = "https://example.org/shop/"
RDF_TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
LABEL = "http://www.w3.org/2000/01/rdf-schema#label"
GENERATED_BY = "http://www.w3.org/ns/prov#wasGeneratedBy"
ASSOCIATED = "http://www.w3.org/ns/prov#wasAssociatedWith"
ACTIVITY = "http://www.w3.org/ns/prov#Activity"
TRIPLE = re.compile(r'^<([^>]+)> <([^>]+)> (<[^>]+>|"(?:[^"\\]|\\.)*"(?:\^\^<[^>]+>|@\S+)?) \.$')

# Explore map palette (light theme): one colour per cord group.
GROUPS = [("Service", "Services", "#c88a00"), ("Database", "Databases", "#d6338a"),
          (ACTIVITY, "Episodes", "#2f9e6e")]
INK, MUTED, PAPER, CARD = "#2b2b2b", "#6f6a60", "#faf7f0", "#ffffff"


def local(iri):
    return iri[len(NS):] if iri.startswith(NS) else iri.rsplit("/", 1)[-1].rsplit("#", 1)[-1]


def literal(term):
    return term.split('"')[1] if term.startswith('"') else local(term.strip("<>"))


def main():
    props = defaultdict(lambda: defaultdict(list))
    for line in (HERE / "shop.nt").read_text().splitlines():
        m = TRIPLE.match(line.strip())
        if m:
            s, p, o = m.groups()
            props[s][p].append(o)

    def group_of(s):
        for t in props[s].get(RDF_TYPE, []):
            iri = t.strip("<>")
            for key, _name, _colour in GROUPS:
                if iri == key or local(iri) == key:
                    return key
        return None

    nodes = {s: group_of(s) for s in props if group_of(s)}
    relations, provenance = [], []
    for s in nodes:
        for p, objs in props[s].items():
            for o in objs:
                target = o.strip("<>")
                if o.startswith("<") and target in nodes:
                    (provenance if p == GENERATED_BY else relations).append((s, local(p), target))
    degree = defaultdict(int)
    for s, _p, o in relations + provenance:
        degree[s] += 1
        degree[o] += 1

    width, top, cord_bottom = 960, 120, 470
    groups = [g for g in GROUPS if any(v == g[0] for v in nodes.values())]
    gx = {g[0]: 190 + i * (width - 380) / max(len(groups) - 1, 1) for i, g in enumerate(groups)}
    pos = {}
    for key, _name, _colour in groups:
        members = sorted((s for s, g in nodes.items() if g == key),
                         key=lambda s: (-degree[s], literal(props[s][LABEL][0]) if props[s][LABEL] else s))
        for j, s in enumerate(members):
            pos[s] = (gx[key], top + 70 + j * 92)

    css, back, front = [], [], []
    n = 0

    def appear(delay):
        nonlocal n
        n += 1
        css.append(f".a{n}{{animation:tie 10s {delay:.2f}s both infinite}}")
        return f"a{n}"

    # Main cord and its group tabs.
    back.append(f'<line x1="70" y1="{top}" x2="{width - 70}" y2="{top}" stroke="{INK}" stroke-width="5" '
                'stroke-linecap="round"/>')
    for i, (key, name, colour) in enumerate(groups):
        x = gx[key]
        count = sum(1 for g in nodes.values() if g == key)
        back.append(f'<line class="{appear(0.15 * i)}" x1="{x:.0f}" y1="{top}" x2="{x:.0f}" '
                    f'y2="{cord_bottom}" stroke="{colour}" stroke-width="3"/>')
        front.append(f'<g class="{appear(0.15 * i)}"><rect x="{x - 70:.0f}" y="{top - 40}" width="140" '
                     f'height="26" rx="6" fill="{CARD}" stroke="#e6e0d2"/>'
                     f'<text x="{x:.0f}" y="{top - 22}" text-anchor="middle" class="t" fill="{colour}" '
                     f'font-size="13">{name} · {count}</text></g>')

    # Provenance strands first (faint), then relations (dark), then knots.
    for k, (s, _p, o) in enumerate(provenance):
        (x1, y1), (x2, y2) = pos[s], pos[o]
        back.append(f'<path class="{appear(1.2 + 0.08 * k)}" d="M{x1:.0f} {y1} C {(x1 + x2) / 2:.0f} {y1}, '
                    f'{(x1 + x2) / 2:.0f} {y2}, {x2:.0f} {y2}" fill="none" stroke="#b9c2d6" '
                    f'stroke-width="5" stroke-opacity=".45"/>')
    for k, (s, p, o) in enumerate(relations):
        (x1, y1), (x2, y2) = pos[s], pos[o]
        bend = 60 if x1 == x2 else 0
        cls = appear(1.8 + 0.25 * k)
        back.append(f'<path class="{cls}" d="M{x1:.0f} {y1} C {x1 + bend:.0f} {y1 + 30}, {x2 + bend:.0f} '
                    f'{y2 - 30}, {x2:.0f} {y2}" fill="none" stroke="#55555f" stroke-width="2.2"/>')
        mx, my = (x1 + x2) / 2 + bend * 0.75, (y1 + y2) / 2
        front.append(f'<text class="{cls} m" x="{mx + 6:.0f}" y="{my:.0f}" fill="{MUTED}" '
                     f'font-size="11">{html.escape(p)}</text>')
    colour_of = {key: colour for key, _name, colour in groups}
    for k, (s, (x, y)) in enumerate(sorted(pos.items(), key=lambda kv: kv[1])):
        r = 9 + 3 * degree[s]
        name = literal(props[s][LABEL][0]) if props[s][LABEL] else local(s)
        if nodes[s] == ACTIVITY:
            source = props[s].get(ASSOCIATED, ['"?"'])[0]
            name = f"{name} (source: {literal(source)})"
        cls = appear(0.6 + 0.12 * k)
        front.append(f'<g class="{cls}"><circle cx="{x:.0f}" cy="{y}" r="{r + 7}" fill="{colour_of[nodes[s]]}" '
                     f'opacity=".15"/><circle cx="{x:.0f}" cy="{y}" r="{r}" fill="{colour_of[nodes[s]]}"/>'
                     f'<text x="{x:.0f}" y="{y + r + 20}" text-anchor="middle" class="t" '
                     f'fill="{colour_of[nodes[s]]}" font-size="13">{html.escape(name)}</text></g>')

    height = cord_bottom + 70
    legend_y = height - 26
    legend = " · ".join(f'<tspan fill="{c}">●</tspan> {nm}' for _k, nm, c in groups)
    front.append(f'<text x="70" y="{legend_y}" class="t" fill="{MUTED}" font-size="12">{legend}'
                 '   —   Knot size: degree (links in this graph). Cords: type groups. '
                 'Faint strands: provenance.</text>')
    label = ("The graph the demo stored, drawn the way Quipu's Explore map draws a graph. Cords for "
             "Services, Databases and Episodes hang from a main cord; knots are entities sized by "
             "their links. checkout calls orders and payments, orders stores in orders-db, and faint "
             "strands tie every entity to the episode it came from.")
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" role="img" aria-label="{html.escape(label)}">
<style>
.t{{font-family:Inter,Segoe UI,Helvetica,Arial,sans-serif}}
.m{{font-family:JetBrains Mono,Menlo,Consolas,monospace}}
@keyframes tie{{0%{{opacity:0}}8%,95%{{opacity:1}}100%{{opacity:0}}}}
{chr(10).join(css)}
@media (prefers-reduced-motion: reduce){{[class*=a]{{animation:none!important;opacity:1!important}}}}
</style>
<rect width="{width}" height="{height}" rx="10" fill="{PAPER}" stroke="#e6e0d2"/>
{chr(10).join(back)}
{chr(10).join(front)}
</svg>
"""
    (HERE / "graph.svg").write_text(svg)
    print(f"wrote graph.svg ({len(groups)} cords, {len(nodes)} knots, "
          f"{len(relations)} relations, {len(provenance)} provenance strands)")


if __name__ == "__main__":
    main()
