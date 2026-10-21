#!/usr/bin/env python3
"""Render measured results; never substitute an absent value with zero."""
import argparse
import json
from pathlib import Path


def values(node, key):
    if isinstance(node, dict):
        if key in node:
            yield node[key]
        for v in node.values():
            yield from values(v, key)
    elif isinstance(node, list):
        for v in node:
            yield from values(v, key)


def maximum(row, key):
    return max(values(row, key), default="UNKNOWN")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    args = parser.parse_args()
    result = json.loads(args.results.read_text())
    print(f"# Merge load: seed {result['seed']}\n")
    print(f"Source `{result['source']}`; requested sizes `{result['sizes']}`, paths `{result['paths']}`.\n")
    print("| Path | Edits/replica | Outcome | Wall s (setup included) | Largest child RSS KiB | Largest decisions file bytes | Largest decision rows | Largest label count | Largest merge DB growth bytes |")
    print("|---|---:|---|---:|---:|---:|---:|---:|---:|")
    for r in result["rows"]:
        rss = r.get("max_child_peak_rss_kib", r.get("peak_rss_kib", "UNKNOWN"))
        print(f"| {r['path']} | {r['edits_per_replica']} | {r['status']} | {r['wall_s']:.3f} | {rss} | {maximum(r,'decision_bytes')} | {maximum(r,'decision_rows')} | {maximum(r,'decision_labels')} | {maximum(r,'store_growth_bytes')} |")
    print("\nUNKNOWN means unmeasured or inapplicable, never zero. Git artifacts are unresolved/base-held; their mandatory gate must refuse. Child RSS is per-process, not concurrent aggregate. Timings are not a production SLO or native/WASM speed comparison. Raw results retain scope, per-operation samples and limitations.")


if __name__ == "__main__":
    main()
