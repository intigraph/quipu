#!/usr/bin/env python3
"""Run pinned suites in CI; keep measured ledgers and provenance as artifacts."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import conformance_report
import expected_results


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rdf-suite", type=Path, required=True)
    parser.add_argument("--shacl-suite", type=Path, required=True)
    parser.add_argument("--quipu", type=Path, required=True)
    parser.add_argument("--shacl", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    expected = json.loads((root / "expected-results.json").read_text())
    args.output.mkdir(parents=True, exist_ok=False)
    start = time.monotonic()
    jobs = [
        ("sparql11_syntax.py", "sparql11-syntax.json", args.rdf_suite / "sparql/sparql11/syntax-query", []),
        ("sparql11_evaluation.py", "sparql11-evaluation.json", args.rdf_suite / "sparql/sparql11", []),
        ("sparql11_federated.py", "sparql11-federated-query.json", args.rdf_suite / "sparql/sparql11", []),
        ("shacl_core.py", "shacl-core.json", args.shacl_suite / "data-shapes-test-suite/tests", []),
        ("rdf11_syntax.py", "rdf11-syntax.json", args.rdf_suite, []),
        ("sparql11_evaluation.py", "sparql11-entailment.json", args.rdf_suite / "sparql/sparql11", ["--class", "entailment"]),
        ("sparql11_evaluation.py", "sparql10-evaluation.json", args.rdf_suite / "sparql/sparql11", ["--class", "sparql10-query"]),
        ("sparql12.py", "sparql12.json", args.rdf_suite, []),
        ("rdf12_syntax.py", "rdf12-syntax.json", args.rdf_suite, []),
    ]
    for script, name, suite, extra in jobs:
        command = [sys.executable, str(root / script), "--suite", str(suite), "--output", str(args.output / name), *extra]
        if script != "rdf12_syntax.py":
            command += ["--quipu", str(args.shacl if script == "shacl_core.py" else args.quipu)]
        result = subprocess.run(command, check=False)
        # Known conformance failures may exit 1. Missing output, unknown statuses,
        # suite drift and exact outcome drift are checked independently below.
        if result.returncode not in (0, 1):
            raise RuntimeError(f"runner {script} failed with {result.returncode}")
    failures = expected_results.check(expected, args.output)
    if failures:
        raise RuntimeError("\n".join(failures))
    # This is a separately tested structural inventory, not timed run evidence.
    shutil.copyfile(root / "results/shacl-write-path.json", args.output / "shacl-write-path.json")
    report = args.output / "report"
    if conformance_report.main(["--results-dir", str(args.output), "--docs-dir", str(report)]) != 0:
        raise RuntimeError("release report generation failed")
    provenance = {
        "schema": "quipu-release-conformance/v1",
        "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "release_tag": os.environ.get("CONFORMANCE_RELEASE_TAG") or None,
        "run_url": os.environ.get("CONFORMANCE_RUN_URL") or None,
        "duration_seconds": time.monotonic() - start,
    }
    (report / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    page = report / "conformance.md"
    page.write_text(page.read_text() + "\n## Release measurement provenance\n\n"
                    + f"Release: `{provenance['release_tag']}`; revision: `{provenance['revision']}`.\n\n"
                    + f"CI run: {provenance['run_url']}\n")
    print(json.dumps(provenance))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
