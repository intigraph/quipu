#!/usr/bin/env python3
"""Render the existing report's bounded semantic neighborhoods as offline HTML.

No RDF parsing, remote queries, scripts, external assets, or entity URL fetching.
Display labels are escaped; identity and graph keys determine edge comparison.
"""
import argparse
import html
import json
import math
from pathlib import Path


def escape(value):
    return html.escape(str(value), quote=True)


def key(edge):
    return tuple(edge[k] for k in ("subject", "predicate", "object", "graph", "multiplicity"))


def positions(before, after, touched=()):
    all_nodes = {edge[field] for side in (before, after)
                 for edge in side["edges"] for field in ("subject", "object")}
    focus = all_nodes.intersection(touched)
    nodes = (sorted(focus) + sorted(all_nodes - focus))[:40]
    return {node: (350 + 245 * math.cos(2 * math.pi * i / max(1, len(nodes))),
                   320 + 245 * math.sin(2 * math.pi * i / max(1, len(nodes))))
            for i, node in enumerate(nodes)}


def diagram(side, other, points, touched, marker):
    other_keys = {key(edge) for edge in other["edges"]}
    edges = [e for e in side["edges"] if e["subject"] in points and e["object"] in points][:80]
    out = [f'<svg viewBox="0 0 700 640" role="img" aria-label="Directed neighborhood graph">'
           f'<defs><marker id="{marker}" viewBox="0 0 10 10" refX="19" refY="5" '
           'markerWidth="6" markerHeight="6" orient="auto-start-reverse">'
           '<path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke"/></marker></defs>']
    labels = {}
    for edge in edges:
        for field in ("subject", "object"):
            labels[edge[field]] = edge[field + "_label"]
        x1, y1 = points[edge["subject"]]
        x2, y2 = points[edge["object"]]
        color = "#76889b" if key(edge) in other_keys else (
            "#916600" if other["truncated"] else "#ab2828" if marker == "before" else "#19733b")
        label = escape(edge["predicate_label"] + " @ " + edge["graph_label"])
        path = (f'M{x1:.1f},{y1:.1f} C{x1+60:.1f},{y1-60:.1f} '
                f'{x1-60:.1f},{y1-60:.1f} {x1:.1f},{y1:.1f}' if edge["subject"] == edge["object"]
                else f'M{x1:.1f},{y1:.1f} Q350,320 {x2:.1f},{y2:.1f}')
        out.append(f'<path d="{path}" '
                   f'fill="none" stroke="{color}" stroke-width="2" marker-end="url(#{marker})">'
                   f'<title>{label}</title></path>')
    for node, label in labels.items():
        x, y = points[node]
        fill = "#bcd9f7" if node in touched else "#e8edf3"
        out.append(f'<g><title>{escape(label)} — {escape(node)}</title>'
                   f'<circle cx="{x:.1f}" cy="{y:.1f}" r="10" fill="{fill}" stroke="#334b63"/>'
                   f'<text x="{x:.1f}" y="{y+24:.1f}" text-anchor="middle">'
                   f'{escape(label[:30])}{"…" if len(label) > 30 else ""}</text></g>')
    out.append('</svg>')
    out.append(f'<p>Diagram: {len(labels)} nodes, {len(edges)} of {side["total_edges"]} context edges. '
               'At most 40 nodes and 80 edges; full retained edge details follow. '
               'Blue nodes are touched subjects; hover nodes and arrows for identity and predicate.</p>')
    return "\n".join(out)


def panel(side, other, title, changed, points, touched):
    # HTML arrows form a directed edge list: it remains readable with CSS off,
    # on a narrow screen, and to screen readers, without a layout dependency.
    other_keys = {key(edge) for edge in other["edges"]}
    out = [f'<section><h2>{title}</h2>']
    count = side["total_edges"]
    out.append(f'<p>{len(side["edges"])} of {count} context edges shown.</p>')
    if side["truncated"]:
        out.append('<p class="warning">TRUNCATED: omitted context may contain additional changes.</p>')
    if not side["edges"]:
        out.append('<p>No context edges on this side.</p>')
    else:
        out.append(diagram(side, other, points, touched, title.lower()))
    for edge in side["edges"]:
        # If the other side is truncated, absence cannot prove add/remove.
        status = "unchanged" if key(edge) in other_keys else (
            "uncompared" if other["truncated"] else changed)
        identity = " | ".join(str(edge[k]) for k in ("subject", "predicate", "object", "graph"))
        out.append(
            f'<article class="{status}"><strong>{status}</strong>'
            f'<div class="edge"><span>{escape(edge["subject_label"])}</span>'
            f'<span class="arrow">→ {escape(edge["predicate_label"])} →</span>'
            f'<span>{escape(edge["object_label"])}</span></div>'
            f'<small>Graph: {escape(edge["graph_label"])}</small>'
            f'<details><summary>Identity keys</summary><code>{escape(identity)}</code></details>'
            '</article>'
        )
    out.append('</section>')
    return "\n".join(out)


def render(report):
    neighborhood = report["neighborhood"]  # missing context fails, never a blank successful view
    before, after = neighborhood["before"], neighborhood["after"]
    touched = neighborhood["touched"]
    points = positions(before, after, touched)
    body = panel(before, after, "Before", "removed", points, touched) + panel(
        after, before, "After", "added", points, touched)
    return f'''<!doctype html>
<html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Pendant graph diff</title>
<style>
body {{ font: 16px system-ui,sans-serif; margin: 2rem auto; padding: 0 1rem; max-width: 100rem; color: #182b3a; background: #f7f9fc; }}
main {{ display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 1.5rem; }}
svg {{ width: 100%; background: white; border-radius: .5rem; }}
svg text {{ font: 12px system-ui,sans-serif; }}
article {{ padding: 1rem; margin: .7rem 0; border: 2px solid #9ba9b6; border-radius: .5rem; background: white; overflow-wrap: anywhere; }}
.edge {{ display: flex; flex-wrap: wrap; align-items: center; gap: .7rem; padding: .5rem 0; }}
.edge span {{ padding: .4rem; background: #eef2f7; border-radius: .3rem; }}
.added {{ border-color: #19733b; }} .removed {{ border-color: #ab2828; }}
.uncompared,.warning {{ border-color: #916600; color: #654600; }}
small,details {{ display: block; margin-top: .5rem; }}
@media(max-width: 700px) {{ main {{ grid-template-columns: 1fr; }} }}
</style>
<h1>Pendant graph diff</h1>
<p>{escape(report["summary"])}</p>
<p>One-hop incoming and outgoing context around {len(neighborhood["touched"])} touched subjects.
Unchanged adjacent facts remain visible. Arrows show subject → predicate → object;
graph names and exact identity keys remain available on every edge.</p>
<p>Semantic projection: referenced blank nodes are folded inline, with the same structural
identity and multiplicity limits as the text diff. This is not a complete raw RDF graph.</p>
<p><strong>Alias caveat:</strong> different IRIs for one real entity can remain invisible
to triple-level comparison. This view grants no merge, import, shape adoption or class-B authority;
hardware-signed human verdicts remain separate.</p>
<main>{body}</main></html>
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("report", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_text(render(json.loads(args.report.read_text())), encoding="utf-8")


if __name__ == "__main__":
    main()
