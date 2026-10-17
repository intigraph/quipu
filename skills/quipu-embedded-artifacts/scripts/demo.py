#!/usr/bin/env python3
"""Build a public synthetic example using the actual native pendant producer."""

import argparse
import json
from pathlib import Path

import generate


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("out", "work", "bundle"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--quipu", default="quipu")
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=False)
    write = lambda path, value: path.write_text(json.dumps(value))
    write(args.work / "ranks.json", {"ranks": [{"rank": 1, "item": "sample-release"}]})
    write(args.work / "asks.json", {"asks": [{"order": 1, "item": "sample-release"}],
                                    "existing_doc_ids": ["release"]})
    write(args.work / "desk.json", {"ops": [{"op": "set", "data": {
        "bead": "sample-release", "ask": "Review the release proposal",
    }}]})
    write(args.work / "bindings.json", {"priorities": {
        "sample-release": {"doc_id": "release", "version": 7},
    }, "asks": {"sample-release": {"doc_id": "release", "version": 3}}})
    snapshot = args.work / "snapshot"
    (snapshot / "brief").mkdir(parents=True)
    (snapshot / "updates").mkdir()
    write(snapshot / "brief/today.json", {"headline": "Release ready for review",
          "points": ["Open its record to inspect the proposal."], "updated": "2026-01-01T00:00:00Z"})

    def uri(value):
        return {"type": "uri", "value": value}

    def text(value):
        return {"type": "literal", "value": value}

    subject = "https://example.org/item/sample-release"
    owner = "https://example.org/principal/maintainer"
    policy = "https://example.org/policy/private-identifiers"
    fields = [{"s": uri(subject), "id": text("sample-release"), "title": text("Prepare the release"),
               "priority": {**text("1"), "datatype": "http://www.w3.org/2001/XMLSchema#integer"},
               "status": text("open"), "owner": uri(owner),
               "description": text("Review the proposal, then use the normal release path.")}]
    responses = [fields,
                 [{"s": uri(subject), "p": uri(generate.SCHEMA + "agent"), "o": uri(owner)}],
                 [{"s": uri(owner), "p": uri(generate.RDFS + "label"), "o": text("Maintainer")}],
                 [{"s": uri(policy), "p": uri(predicate), "o": value} for predicate, value in (
                     (generate.RDF + "type", uri(generate.AEGIS + "InternalIdentifierPattern")),
                     (generate.AEGIS + "regex", text("private[.]example")),
                     (generate.AEGIS + "enforcementTier", text("block")),
                     (generate.RDFS + "label", text("Synthetic private identifiers")))]]

    class FixtureReader(generate.Reader):
        def query(self, query, limit):
            result = responses[self.number]
            self.number += 1
            if len(result) >= limit:
                raise ValueError("Fixture exceeds the production bound")
            (self.work / f"query-{self.number}.rq").write_text(query)
            write(self.work / f"query-{self.number}.json", {"results": {"bindings": result}})
            return result

    # Replace only the remote input with an explicit fixture. Native shape
    # loading, query registry, outward share and browser receiving stay real.
    generate.Reader = FixtureReader
    config = {"server": "https://example.org", "graph": "urn:example:project",
              "ranks_file": str(args.work / "ranks.json"), "asks_file": str(args.work / "asks.json"),
              "desk_file": str(args.work / "desk.json"), "snapshot_dir": str(snapshot),
              "row_bindings_file": str(args.work / "bindings.json")}
    generate.generate(config, args.out, args.work / "producer", args.bundle, args.quipu)
    (args.out / "before.html").write_text('''<title>Static brief fixture</title>
<style>body{font:20px system-ui;max-width:52rem;margin:3rem auto}</style>
<h1>Project brief · static fixture</h1><p>1 · Prepare the release</p>
<p>The row has no embedded graph or record drilldown.</p>''')
    (args.out / "index.html").write_text('''<title>Embedded Quipu fixture</title>
<link rel="stylesheet" href="./explorer.css">
<style>body{font:18px system-ui;max-width:52rem;margin:2rem auto;padding:0 1rem}</style>
<h1>Project brief · generated pendant</h1>
<p>1 · Prepare the release <button id="open" disabled>Open record</button></p>
<section id="graph"></section><script type="module">
import {attachExplorer} from './explorer.js';
window.explorer=await attachExplorer(document.getElementById('graph'));
document.getElementById('open').disabled=false;
document.getElementById('open').onclick=()=>window.explorer.openItem('sample-release','Prepare the release');
</script>''')


if __name__ == "__main__":
    main()
