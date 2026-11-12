#!/usr/bin/env python3
"""Separate stable, committed conformance evidence from per-run telemetry.

Raw ledgers remain CI artifacts. Adopt the stable artifact in the source PR;
there is no post-merge writer. Revisions remain in committed snapshots so the
separate provenance gate can compare the measured code with the candidate.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from conformance_report import CODE_PATHS


LEDGERS = (
    "sparql11-syntax.json", "sparql11-evaluation.json",
    "sparql11-entailment.json", "sparql11-federated-query.json",
    "shacl-core.json", "rdf11-syntax.json", "sparql10-evaluation.json",
    "sparql12.json",
)
TELEMETRY = frozenset({"generated_at", "generated_by", "duration_ms"})


def code_digest(root=None):
    """Hash the tracked code inputs, independent of commit identity.

    Frame path and bytes separately; directory enumeration order and ambiguous
    concatenations cannot change or alias the digest. Read failures propagate.
    """
    root = Path(root) if root is not None else Path(__file__).resolve().parents[2]
    paths = subprocess.run(
        ["git", "ls-files", "-z", "--", *CODE_PATHS], cwd=root,
        check=True, capture_output=True,
    ).stdout.split(b"\0")
    paths = sorted(path for path in paths if path)
    if not paths:
        raise ValueError("no tracked code inputs")
    digest = hashlib.sha256(b"quipu-conformance-code-v1\0")
    for path in paths:
        for part in (path, (root / path.decode()).read_bytes()):
            digest.update(len(part).to_bytes(8, "big"))
            digest.update(part)
    return "sha256:" + digest.hexdigest()


def measured_digest(candidates):
    root = Path(__file__).resolve().parents[2]
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root,
                          check=True, text=True, capture_output=True).stdout.strip()
    subprocess.run(["git", "diff", "--exit-code", "HEAD", "--", *CODE_PATHS],
                   cwd=root, check=True, capture_output=True)
    if any(data["quipu_revision"] != head for data in candidates.values()):
        raise ValueError("cannot stamp old evidence: raw ledgers must measure the current clean HEAD")
    return code_digest(root)


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
    parser.add_argument("--record-code", action="store_true",
                        help="record clean measured inputs; raw revisions must all equal HEAD")
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
        if args.record_code:
            if args.check:
                raise ValueError("--record-code requires --output")
            measured = measured_digest(candidates)
            candidates = {name: {**data, "code_digest": measured}
                          for name, data in candidates.items()}
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
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"Conformance snapshot UNVERIFIED: {error}")
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
