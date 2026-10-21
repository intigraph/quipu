#!/usr/bin/env python3
"""Rearm a deferred Release after genuine CI completes, at most once.

# arming: ci; release-ci-ready.yml observes both completion orders.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys

from wait_release_correctness import DEFERRED_JOB, PENDING, classify, fetch


def api(path: str, *, method: str = "GET") -> dict:
    result = subprocess.run(
        ["gh", "api", "--method", method, path],
        capture_output=True, text=True, check=True,
    )
    return json.loads(result.stdout) if result.stdout.strip() else {}


def trusted(run: dict, repo: str, sha: str, workflow: str) -> bool:
    return (
        run.get("event") == "push" and run.get("head_branch") == "main"
        and run.get("head_sha") == sha
        and (run.get("head_repository") or {}).get("full_name") == repo
        and run.get("path") == f".github/workflows/{workflow}.yml"
    )


def latest(repo: str, sha: str, workflow: str) -> dict | None:
    data = api(f"repos/{repo}/actions/workflows/{workflow}.yml/runs"
               f"?head_sha={sha}&event=push&per_page=100")
    # Incomplete API coverage is UNKNOWN, never permission to rerun.
    if data.get("total_count") != len(data.get("workflow_runs", [])):
        raise RuntimeError("incomplete workflow-run inventory")
    runs = [r for r in data["workflow_runs"] if trusted(r, repo, sha, workflow)]
    return max(runs, key=lambda r: r["id"]) if runs else None


def rearm(repo: str, sha: str) -> str:
    def current() -> bool:
        return api(f"repos/{repo}/git/ref/heads/main")["object"]["sha"] == sha

    if not current():
        return "superseded main SHA; no retry"
    ci = latest(repo, sha, "ci")
    release = latest(repo, sha, "release")
    if not ci or ci.get("status") != "completed":
        return "CI not complete; no retry"
    if not release or release.get("status") != "completed":
        return "Release not complete; no retry"
    if release.get("conclusion") != "success" or release.get("run_attempt") != 1:
        return "not an initial non-failing deferred Release; no retry"
    run_id = release["id"]
    jobs = api(f"repos/{repo}/actions/runs/{run_id}/attempts/1/jobs?per_page=100")
    if jobs.get("total_count") != len(jobs.get("jobs", [])):
        raise RuntimeError("incomplete release-job inventory")
    if not any(j.get("name") == DEFERRED_JOB and j.get("conclusion") == "success"
               for j in jobs["jobs"]):
        return "Release was not deferred; no retry"
    if classify(fetch(repo, sha))[0] == PENDING:
        return "correctness check still pending or absent; no retry"
    # Recheck mutable admission immediately before the single write. Serialized
    # completion events then see the queued retry instead of launching another.
    fresh = api(f"repos/{repo}/actions/runs/{run_id}")
    if (not current() or not trusted(fresh, repo, sha, "release")
            or any(fresh.get(k) != release.get(k)
                   for k in ("id", "status", "conclusion", "run_attempt"))):
        return "Release or main changed during admission; no retry"
    api(f"repos/{repo}/actions/runs/{run_id}/rerun", method="POST")
    return f"rearmed deferred Release {run_id} once; its exact-SHA gate still decides"


def main() -> int:
    if len(sys.argv) != 2 or not re.fullmatch(r"[0-9a-f]{40}", sys.argv[1]):
        print("expected one full commit SHA", file=sys.stderr)
        return 2
    repo = os.environ.get("GITHUB_REPOSITORY", "")
    if not repo:
        print("GITHUB_REPOSITORY is required", file=sys.stderr)
        return 2
    print(rearm(repo, sys.argv[1]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
