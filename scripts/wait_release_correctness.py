#!/usr/bin/env python3
"""Wait for CI's exact-SHA release correctness check, failing closed."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any


CHECK_NAME = "Release correctness"
PENDING = 0
SUCCESS = 1
FAILURE = 2
DEFERRED_JOB = "Release deferred (CI not ready)"


def report(state: int, sha: str, detail: str) -> int:
    """Pending is not a test failure, and only SUCCESS permits publication."""
    ready = state == SUCCESS
    if output := os.environ.get("GITHUB_OUTPUT"):
        with Path(output).open("a") as stream:
            stream.write(f"ready={str(ready).lower()}\n")
    verdict = "READY" if ready else "FAILED" if state == FAILURE else "DEFERRED"
    message = f"{verdict}: {CHECK_NAME} on exact SHA {sha}: {detail}"
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(summary).open("a") as stream:
            stream.write(message + "\n\n")
    if state == PENDING:
        print(f"::warning title=Release deferred::{message}; publishing stays skipped")
    else:
        print(message)
    return 1 if state == FAILURE else 0


def classify(check_runs: list[dict[str, Any]]) -> tuple[int, str]:
    """Classify the newest genuine GitHub Actions correctness check."""
    matches = [
        run for run in check_runs
        if run.get("name") == CHECK_NAME
        and (run.get("app") or {}).get("slug") == "github-actions"
    ]
    if not matches:
        return PENDING, "check has not appeared"
    latest = max(matches, key=lambda run: int(run.get("id", 0)))
    status = latest.get("status")
    conclusion = latest.get("conclusion")
    if status != "completed":
        return PENDING, f"check {latest.get('id')} is {status or 'pending'}"
    if conclusion == "success":
        return SUCCESS, f"check {latest.get('id')} passed"
    return FAILURE, f"check {latest.get('id')} concluded {conclusion or 'without a verdict'}"


def fetch(repo: str, sha: str) -> list[dict[str, Any]]:
    result = subprocess.run(
        ["gh", "api", f"repos/{repo}/commits/{sha}/check-runs?per_page=100"],
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or "gh api failed without an error message")
    payload = json.loads(result.stdout)
    return payload.get("check_runs", [])


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <commit-sha>", file=sys.stderr)
        return 2
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    if not repo:
        print("GITHUB_REPOSITORY is required", file=sys.stderr)
        return 2
    sha = sys.argv[1]
    interval = float(os.environ.get("CI_GATE_INTERVAL_SECONDS", "10"))
    attempts = int(os.environ.get("CI_GATE_MAX_ATTEMPTS", "270"))

    for attempt in range(1, attempts + 1):
        try:
            state, detail = classify(fetch(repo, sha))
        except (RuntimeError, json.JSONDecodeError) as exc:
            state, detail = PENDING, f"could not read checks: {exc}"
        print(f"[{attempt}/{attempts}] {sha}: {detail}", flush=True)
        if state == SUCCESS:
            return report(state, sha, detail)
        if state == FAILURE:
            return report(state, sha, detail)
        if attempt < attempts:
            time.sleep(interval)

    return report(PENDING, sha, f"wait budget exhausted; {detail}")


if __name__ == "__main__":
    raise SystemExit(main())
