#!/usr/bin/env python3
"""Strict ledger admission on GitHub's combined merge-group tree (read only)."""
import ast
import json
import os
from pathlib import Path
import re
import subprocess


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def code_paths(revision):
    """Read the existing provenance path contract without executing base code."""
    source = git("show", f"{revision}:benchmark/public/conformance_report.py")
    tree = ast.parse(source)
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "CODE_PATHS"
            for target in node.targets
        ):
            paths = ast.literal_eval(node.value)
            if isinstance(paths, (tuple, list)) and paths and all(
                isinstance(path, str) and path for path in paths
            ):
                return set(paths)
    raise ValueError("cannot verify provenance CODE_PATHS")


def validate(event):
    group = event["merge_group"]
    head, base = group["head_sha"], group["base_sha"]
    if not all(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha)
               for sha in (head, base)):
        raise ValueError("invalid merge-group commit identity")
    if git("rev-parse", "HEAD") != head:
        raise ValueError("checkout is not the merge group's head")
    subprocess.run(["git", "merge-base", "--is-ancestor", base, head], check=True)
    # Same contract as landing-helper ledger_stamp: one full, resolvable stamp
    # across the ledgers, with no code drift. No stale-stamp waiver in a queue.
    directory = "benchmark/public/results"
    names = git("ls-tree", "-r", "--name-only", head, "--", directory).splitlines()
    stamps = set()
    for name in names:
        if not name.endswith(".json"):
            continue
        data = json.loads(git("show", f"{head}:{name}"))
        if isinstance(data, dict):
            stamps.update(value for key, value in data.items()
                          if key.endswith("quipu_revision")
                          and isinstance(value, str) and value)
    if len(stamps) != 1:
        raise ValueError("ledgers must name exactly one quipu_revision")
    stamp = stamps.pop()
    if not re.fullmatch(r"[0-9a-f]{40}", stamp):
        raise ValueError("ledger revision must be a full commit SHA")
    git("cat-file", "-e", f"{stamp}^{{commit}}")
    # Union prevents an entry narrowing the contract inherited from queue base.
    paths = sorted(code_paths(base) | code_paths(head))
    drift = git("diff", "--name-only", stamp, head, "--", *paths)
    if drift:
        raise ValueError("stale ledger on combined tree: " + ", ".join(drift.splitlines()))
    return f"ledger {stamp} covers merge group {head} (base {base})"


def main():
    try:
        if os.environ.get("GITHUB_EVENT_NAME") != "merge_group":
            raise ValueError("expected a bound merge_group event")
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        print(validate(event))
    except (ValueError, KeyError, OSError, subprocess.SubprocessError) as error:
        print(f"REFUSED: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
