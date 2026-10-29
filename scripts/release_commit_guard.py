#!/usr/bin/env python3
"""Decide what a push-to-main Release run may do with the package version.

Why (aegis-3xv): release-plz tags and publishes whatever commit the run checked
out. On 2026-10-27 the release PR merged as cb46a8bd (chore: release v0.16.4)
and #878 merged four seconds later as 6eacb65e. The newer push cancelled the
release commit's run, so the 6eacb65e run cut 0.16.4 and tagged 6eacb65e. The
0.16.4 crate therefore shipped #878, while its CHANGELOG section (generated for
the release PR) does not list it. From then on every push re-verified that
frozen section and failed "Verify changelog", painting main red for a defect
nothing in the lane can repair.

Outputs, written to $GITHUB_OUTPUT when set and printed either way:

  state=released   the version's tag already exists. Nothing to cut, and the
                   shipped section is history: the run that cut it verified it.
  state=release    HEAD is the release commit, or no run for the release commit
                   is still pending. This run may cut the release.
  state=defer      the version is not yet tagged, HEAD is a later commit, and the
                   release commit's own Release run is still queued or running.
                   Let it cut the release so the tag matches the changelog.

Usage: scripts/release_commit_guard.py <head-sha>
       scripts/release_commit_guard.py --self-test
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tomllib

PACKAGE_TAG = "quipu-ai-v{}"
PENDING = {"queued", "in_progress", "waiting", "requested", "pending"}


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout


def version_at(rev: str) -> str | None:
    try:
        text = git("show", f"{rev}:Cargo.toml")
    except subprocess.CalledProcessError:
        return None
    return tomllib.loads(text).get("package", {}).get("version")


def release_commit(head: str, version: str) -> str | None:
    """First-parent commit on HEAD's line that set the package version."""
    for sha in git("log", "--first-parent", "--format=%H", head, "--", "Cargo.toml").split():
        if version_at(sha) == version and version_at(f"{sha}^") != version:
            return sha
    return None


def tag_exists(version: str) -> bool:
    return subprocess.run(
        ["git", "rev-parse", "-q", "--verify", f"refs/tags/{PACKAGE_TAG.format(version)}"],
        capture_output=True,
    ).returncode == 0


def release_run_pending(repo: str, sha: str) -> bool:
    out = subprocess.run(
        ["gh", "api", f"repos/{repo}/actions/workflows/release.yml/runs"
         f"?head_sha={sha}&event=push&per_page=100"],
        capture_output=True, text=True, check=True,
    ).stdout
    return any(r.get("status") in PENDING for r in json.loads(out).get("workflow_runs", []))


def decide(*, tagged: bool, head: str, rel: str | None, rel_pending: bool) -> str:
    if tagged:
        return "released"
    if rel is None or rel == head or not rel_pending:
        return "release"
    return "defer"


def self_test() -> None:
    assert decide(tagged=True, head="b", rel="a", rel_pending=True) == "released"
    assert decide(tagged=False, head="a", rel="a", rel_pending=True) == "release"
    # The 0.16.4 race: a later push must not cut the release while the
    # release commit's own run is still alive.
    assert decide(tagged=False, head="b", rel="a", rel_pending=True) == "defer"
    # A stranded release commit (its run finished without tagging) must not
    # leave the version unreleased forever.
    assert decide(tagged=False, head="b", rel="a", rel_pending=False) == "release"
    assert decide(tagged=False, head="b", rel=None, rel_pending=False) == "release"
    print("release_commit_guard self-test: ok")


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        return 0
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    head = git("rev-parse", sys.argv[1]).strip()
    version = version_at(head)
    if not version:
        print("ERROR: no package version in Cargo.toml", file=sys.stderr)
        return 2
    tagged = tag_exists(version)
    rel = None if tagged else release_commit(head, version)
    rel_pending = False
    if rel and rel != head:
        rel_pending = release_run_pending(os.environ["GITHUB_REPOSITORY"], rel)
    state = decide(tagged=tagged, head=head, rel=rel, rel_pending=rel_pending)
    print(f"version={version} tagged={tagged} release_commit={rel} "
          f"release_run_pending={rel_pending} -> state={state}")
    if out := os.environ.get("GITHUB_OUTPUT"):
        with open(out, "a") as fh:
            fh.write(f"state={state}\nversion={version}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
