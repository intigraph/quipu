#!/usr/bin/env python3
"""Draw the relationship actually returned by the recorded native query."""
import hashlib
import html
import json
from pathlib import Path

root = Path(__file__).resolve().parent
manifest = root / "manifest.json"
m = json.loads(manifest.read_text())
result = m["commands"][1]
assert result["exit"] == 0
rows = [line.split() for line in result["stdout"].splitlines()
        if line.startswith("urn:demo:")]
assert rows == [["urn:demo:Ada", "urn:demo:Engine"]], "unexpected query result"
a, b = (html.escape(value.removeprefix("urn:demo:")) for value in rows[0])
svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="420" viewBox="0 0 1000 420" role="img" aria-labelledby="title desc">
<title id="title">One note, one connected fact</title>
<desc id="desc">The native query returns Ada studying Engine. Each knot is an entity; the strand is the studies relationship.</desc>
<rect width="1000" height="420" rx="16" fill="#101827"/>
<g fill="none" stroke="#6ee7b7" stroke-width="4">
<path d="M260 205C400 50 600 350 740 205"/>
<circle cx="260" cy="205" r="12"/><circle cx="740" cy="205" r="12"/>
<path d="M260 95V315M740 95V315" stroke="#475569"/>
<path d="M248 182L272 228M248 228L272 182M728 182L752 228M728 228L752 182"/>
</g>
<g fill="#f1f5f9" font-family="DejaVu Sans Mono, Menlo, monospace" font-size="28" text-anchor="middle">
<text x="260" y="78">{a}</text><text x="740" y="78">{b}</text>
<text x="500" y="140" fill="#6ee7b7">studies</text>
<text x="500" y="375" font-size="24">Read the relationship back with a query.</text>
</g></svg>
'''
(root / "graph.svg").write_text(svg)
(root / "graph.provenance.json").write_text(json.dumps({
    "input_manifest_sha256": hashlib.sha256(manifest.read_bytes()).hexdigest(),
    "source": "native query stdout, command index 1; exact one-row assertion",
    "row": rows[0], "relation": "urn:demo:studies",
    "graph_sha256": hashlib.sha256(svg.encode()).hexdigest()}, indent=2) + "\n")
