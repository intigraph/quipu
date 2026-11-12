#!/usr/bin/env python3
"""Separate stable, committed conformance evidence from per-run telemetry.

Raw ledgers remain CI artifacts. Adopt the stable artifact in the source PR;
there is no post-merge writer. Revisions remain in committed snapshots so the
separate provenance gate can compare the measured code with the candidate.
"""

import argparse
import json
from pathlib import Path


LEDGERS = (
    "sparql11-syntax.json", "sparql11-evaluation.json",
    "sparql11-entailment.json", "sparql11-federated-query.json",
    "shacl-core.json", "rdf11-syntax.json", "sparql10-evaluation.json",
    "sparql12.json",
)
TELEMETRY = frozenset({"generated_at", "generated_by", "duration_ms"})


def snapshot(value, *, outcomes=False):
    """Keep outcomes and their inventory; omit only explicit telemetry keys.

    Outcome comparison also omits the measured commit: the provenance check
    verifies source equivalence, while metadata-only commits and the queue's
    synthetic commit necessarily have different IDs.
    """
    if isinstance(value, dict):
        return {
            key: snapshot(child, outcomes=outcomes)
            for key, child in value.items()
            if key not in TELEMETRY and not (outcomes and key == "quipu_revision")
        }
    if isinstance(value, list):
        return [snapshot(child, outcomes=outcomes) for child in value]
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--output", type=Path)
    action.add_argument("--check", type=Path)
    args = parser.parse_args(argv)
    # Validate the full inventory before writing anything. Missing output is
    # an incomplete measurement, never a skipped or passing comparison.
    try:
        candidates = {name: json.loads((args.source / name).read_text()) for name in LEDGERS}
        for name, data in candidates.items():
            if not isinstance(data, dict) or not data.get("quipu_revision") or not data.get("results"):
                raise ValueError(f"{name}: missing revision or results")
        if args.check:
            drift = [name for name, data in candidates.items()
                     if snapshot(data, outcomes=True) != snapshot(
                         json.loads((args.check / name).read_text()), outcomes=True)]
            if drift:
                print("Conformance ledgers differ; adopt the stable CI artifact in this PR:")
                print("\n".join(drift))
                return 1
            print(f"{len(LEDGERS)} conformance ledgers match the measured candidate")
        else:
            args.output.mkdir(parents=True, exist_ok=True)
            for name, data in candidates.items():
                (args.output / name).write_text(json.dumps(snapshot(data), indent=2) + "\n")
    except (OSError, ValueError) as error:
        print(f"Conformance snapshot UNVERIFIED: {error}")
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
