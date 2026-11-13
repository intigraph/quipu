#!/usr/bin/env python3
"""Independent migration proof, without importing the generator or gate.

Anchor: full v1 contract at 1d3909ff61595b6156f806d95c17a62cd5d05d52.
The canonical SHA covers every suite name, revision, class, manifest, ID and
outcome. Intentional future expectation changes must update this reviewed
anchor too; CI never refreshes it automatically. --legacy additionally proves
literal expanded equality against an archived v1 document.
"""
import argparse
import hashlib
import json
from pathlib import Path

LEGACY_SHA256 = "874dca682d66ba2d325c5bac8c1b58ab731a45484a98a989e6c9d16dcd7f08e6"


def expanded(document):
    assert set(document) == {"schema", "suites"}
    assert document["schema"] == "quipu-expected-results/v2"
    suites = {}
    for name, suite in document["suites"].items():
        assert set(suite) == {"suite_revision", "groups"}
        cases = []
        seen = set()
        for cls, manifests in suite["groups"].items():
            for manifest, outcomes in manifests.items():
                for outcome, identifiers in outcomes.items():
                    assert outcome in {"pass", "xfail"} and isinstance(identifiers, list)
                    for identifier in identifiers:
                        assert all(isinstance(v, str) and v for v in (cls, manifest, identifier))
                        key = (cls, manifest, identifier)
                        assert key not in seen, f"duplicate case: {key}"
                        seen.add(key)
                        cases.append([*key, outcome])
        suites[name] = {"suite_revision": suite["suite_revision"], "cases": sorted(cases)}
    return {"schema": "quipu-expected-results/v1", "suites": suites}


def prove(document, legacy=None):
    result = expanded(document)
    canonical = json.dumps(result, sort_keys=True, separators=(",", ":")).encode()
    assert hashlib.sha256(canonical).hexdigest() == LEGACY_SHA256, "expanded contract differs from reviewed v1"
    assert len(result["suites"]) == 9
    assert sum(len(s["cases"]) for s in result["suites"].values()) == 2301
    if legacy is not None:
        assert result == legacy, "literal old/new equality failed"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compact", type=Path, default=Path(__file__).with_name("expected-results.json"))
    parser.add_argument("--legacy", type=Path)
    args = parser.parse_args()
    prove(json.loads(args.compact.read_text()), json.loads(args.legacy.read_text()) if args.legacy else None)
    print("Exact v1 equality: 9 suites, 2301 case identities/outcomes, all pins; SHA256", LEGACY_SHA256)


if __name__ == "__main__":
    main()
