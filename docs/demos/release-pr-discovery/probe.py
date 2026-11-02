#!/usr/bin/env python3
"""Run real release-workflow discovery against disposable command fixtures."""
# arming: agent-invoked; no network, repository mutation or status publication.
import os
from pathlib import Path
import subprocess
import tempfile

import yaml


def workflow_body(path):
    jobs = yaml.safe_load(Path(path).read_text())["jobs"]
    return next(step["run"] for step in jobs["release-pr-freshness"]["steps"]
                if step.get("name") == "Check the open release PR against current main")


with tempfile.TemporaryDirectory() as directory:
    fixture = Path(directory)
    gh = fixture / "gh"
    gh.write_text('''#!/bin/sh
[ "$1 $2" = 'pr list' ] || exit 99
if [ "$CASE" = api-error ]; then echo 'synthetic API failure' >&2; exit 1; fi
printf '  \\n'
''')
    gh.chmod(0o700)
    for label, path in (("BASE", os.environ["BASE_RELEASE_WORKFLOW"]),
                        ("CANDIDATE", ".github/workflows/release.yml")):
        for case in ("known-empty", "api-error"):
            result = subprocess.run(["bash", "-c", workflow_body(path)], cwd=fixture,
                                    env={**os.environ, "CASE": case,
                                         "PATH": str(fixture) + os.pathsep + os.environ["PATH"]},
                                    capture_output=True, text=True)
            claims_absence = "No open release PR" in result.stdout
            print(f"{label} {case}: exit={result.returncode} claims_absence={claims_absence}")
            if label == "CANDIDATE" and case == "api-error":
                assert result.returncode != 0 and not claims_absence
            else:
                assert result.returncode == 0 and claims_absence
    print("Exact workflow bodies; disposable gh stub; no live API or status writes.")
