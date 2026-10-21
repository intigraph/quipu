#!/usr/bin/env python3
"""Compare the actual waiter main() at two revisions with identical API fixtures."""
import argparse
import contextlib
import io
import os
from pathlib import Path
import subprocess
import tempfile
import types
from unittest.mock import patch


def demonstrate(ref):
    sha = subprocess.check_output(["git", "rev-parse", ref], text=True).strip()
    source = subprocess.check_output(
        ["git", "show", f"{sha}:scripts/wait_release_correctness.py"], text=True,
    )
    module = types.ModuleType("fixture_waiter")
    exec(compile(source, "wait_release_correctness.py", "exec"), module.__dict__)
    print(f"\nRevision {sha}; actual waiter main(), mocked check API, one poll")
    results = {}
    for status, conclusion in (("queued", None), ("in_progress", None),
                               ("completed", "failure"), ("completed", "success")):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "outputs"
            summary = Path(directory) / "summary"
            env = {"GITHUB_REPOSITORY": "example/project", "CI_GATE_MAX_ATTEMPTS": "1",
                   "CI_GATE_INTERVAL_SECONDS": "0", "GITHUB_OUTPUT": str(output),
                   "GITHUB_STEP_SUMMARY": str(summary)}
            checks = [dict(id=42, name="Release correctness", status=status,
                           conclusion=conclusion, app={"slug": "github-actions"})]
            log = io.StringIO()
            with patch.dict(os.environ, env), patch.object(module.sys, "argv", ["waiter", "a" * 40]), \
                    patch.object(module, "fetch", return_value=checks), \
                    contextlib.redirect_stdout(log), contextlib.redirect_stderr(log):
                code = module.main()
            ready = output.read_text().strip() if output.exists() else "(not emitted)"
            print(f"  {status}/{conclusion}: exit={code}, {ready}")
            print("    " + log.getvalue().splitlines()[-1])
            results[(status, conclusion)] = code, ready
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("head")
    args = parser.parse_args()
    before = demonstrate(args.base)
    after = demonstrate(args.head)
    assert before[("queued", None)][0] == 1
    assert after[("queued", None)] == (0, "ready=false")
    assert after[("in_progress", None)] == (0, "ready=false")
    assert before[("completed", "failure")][0] == after[("completed", "failure")][0] == 1
    assert after[("completed", "success")] == (0, "ready=true")
    print("\nObserved: pending timeout defers; real failure still fails; only success is ready.")
