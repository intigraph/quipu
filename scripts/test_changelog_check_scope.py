#!/usr/bin/env python3
"""Prove changelog-check.yml gates release-plz PRs only, and still fails a bad one.

aegis-06px. The workflow triggers on any PR touching CHANGELOG.md, but it was
introduced (becf5a0d) to gate release-plz's PRs. Since 2ef6dfa5 ordinary PRs are
told to add a CHANGELOG entry, so the trigger alone no longer means "release PR".
The Scope step restores the intent. This test reads the real workflow text:

1. the trigger and the job are unchanged in shape (no job-level `if:`, since a
   skipped check is not provable by the merge gate);
2. the IN_SCOPE expression is true only for a same-repository `release-plz*` head;
3. the Scope step's own bash writes in_scope true/false and refuses anything else;
4. every later step is gated on that output;
5. POSITIVE CONTROL: an in-scope release PR whose CHANGELOG misses a packaged
   commit still FAILS the verifier; an ordinary PR runs no verifier step at all.

# arming: ci Verify the CI to Release contract; also agent-invoked.
"""
from pathlib import Path
import os
import re
import shutil
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parents[1]
workflow = (root / ".github/workflows/changelog-check.yml").read_text()
failures = []


def check(name, condition, detail=""):
    print(("  PASS  " if condition else "  FAIL  ") + name + ("" if condition else f": {detail}"))
    if not condition:
        failures.append(name)


# 1. Trigger and job shape.
check("trigger is still pull_request on CHANGELOG.md",
      re.search(r"^on:\n  pull_request:\n    paths:\n      - CHANGELOG\.md\n", workflow, re.M) is not None)
job = workflow.split("\njobs:\n", 1)[1]
header = job.split("    steps:\n", 1)[0]
check("no job-level if (a skipped check is not provable)", "\n    if:" not in header, header)

# 2. Scope expression, evaluated for each PR shape.
match = re.search(r"IN_SCOPE: >-\n((?:            .*\n)+)", workflow)
check("Scope step declares IN_SCOPE", match is not None)
expression = " ".join(line.strip() for line in match.group(1).splitlines()) if match else ""
expression = expression.removeprefix("${{").removesuffix("}}").strip()


def evaluate(head_repo, head_ref, repository="intigraph/quipu"):
    values = {
        "github.event.pull_request.head.repo.full_name": repr(head_repo),
        "github.event.pull_request.head.ref": repr(head_ref),
        "github.repository": repr(repository),
    }
    text = expression
    for key, value in values.items():
        text = text.replace(key, value)
    text = re.sub(r"startsWith\(([^,]+),\s*'([^']*)'\)", r"\1.startswith('\2')", text)
    text = text.replace("&&", " and ").replace("||", " or ")
    text = re.sub(r"\btrue\b", "True", re.sub(r"\bfalse\b", "False", text))
    if re.search(r"github\.|\$\{\{", text):
        raise AssertionError(f"unexpected context in scope expression: {text}")
    return "true" if eval(text, {"__builtins__": {}}) else "false"  # noqa: S307 (fixed, local text)


cases = [
    ("same-repo release-plz PR", ("intigraph/quipu", "release-plz-2026-11-11T07-00-00Z"), "true"),
    ("same-repo ordinary PR touching CHANGELOG.md", ("intigraph/quipu", "ian/some-fix"), "false"),
    ("fork PR named release-plz-*", ("someone/quipu", "release-plz-evil"), "false"),
]
for name, args, want in cases:
    got = evaluate(*args) if expression else "?"
    check(f"scope {name} -> {want}", got == want, f"got {got}")

# 3. The Scope step's own bash.
run_match = re.search(r"        run: \|\n((?:          .*\n)+)", workflow)
check("Scope step has a run block", run_match is not None)
scope_script = "\n".join(line[10:] for line in run_match.group(1).splitlines()) if run_match else ""


def run_scope(value):
    with tempfile.NamedTemporaryFile("r+") as output:
        done = subprocess.run(["bash", "-c", scope_script], env={**os.environ, "IN_SCOPE": value,
                              "GITHUB_OUTPUT": output.name}, capture_output=True, text=True)
        return done.returncode, Path(output.name).read_text()


for value in ("true", "false"):
    rc, out = run_scope(value)
    check(f"Scope step writes in_scope={value}", rc == 0 and out == f"in_scope={value}\n", f"rc={rc} out={out!r}")
rc, _ = run_scope("maybe")
check("Scope step refuses an unexpected value", rc == 1, f"rc={rc}")

# 4. Every later step is gated on the scope output.
steps = job.split("    steps:\n", 1)[1].split("\n      - ")[1:]
gate = "if: steps.scope.outputs.in_scope == 'true'"
check("three steps follow Scope", len(steps) == 3, f"{len(steps)} steps")
for step in steps:
    check(f"gated: {step.splitlines()[0].strip()}", gate in step, step)

# 5. Positive control on a hermetic release fixture.
cliff = shutil.which("git-cliff")
if cliff is None:
    check("git-cliff available for the positive control", False, "install git-cliff")
else:
    with tempfile.TemporaryDirectory() as scratch:
        repo = Path(scratch)
        git = lambda *a: subprocess.run(["git", *a], cwd=repo, check=True, capture_output=True, text=True).stdout.strip()
        git("init", "-q", ".")
        git("config", "user.email", "t@example.com")
        git("config", "user.name", "t")
        (repo / "scripts").mkdir()
        (repo / "src").mkdir()
        shutil.copy(root / "cliff.toml", repo)
        for name in ("verify-changelog.sh", "fix-changelog.sh", "filter-packaged-commits.py",
                     "check-release-window.py", "check-release-mechanics.py",
                     "changelog-duplicates.py", "release-pr-freshness.sh",
                     "python-binding-sync-only.py"):
            shutil.copy(root / "scripts" / name, repo / "scripts")
        (repo / "Cargo.toml").write_text('[package]\nname="probe"\nversion="1.0.0"\nedition="2021"\nexclude=["docs/"]\n')
        for body, message in (("a", "feat: the released thing"), ("b", "feat: the second thing"), ("c", "fix: the third thing")):
            (repo / "src/lib.rs").write_text(body + "\n")
            git("add", "-A")
            git("commit", "-qm", message)
            if body == "a":
                git("tag", "v1.0.0")
        second = git("rev-parse", "--short=7", "HEAD~1")
        (repo / "CHANGELOG.md").write_text(
            "# Changelog\n\n## [1.1.0] - 2026-01-01\n\n"
            f"- Some change([{second}](https://example.invalid/commit/{second}))\n")

        def job_outcome(head_repo, head_ref):
            """Run the job as Actions would: Scope, then the gated verifier step."""
            if evaluate(head_repo, head_ref) != "true":
                return 0, "verifier step skipped by scope"
            done = subprocess.run(["./scripts/verify-changelog.sh"], cwd=repo, capture_output=True, text=True)
            return done.returncode, done.stdout + done.stderr

        rc, out = job_outcome("intigraph/quipu", "release-plz-2026-11-11")
        check("release-plz PR missing a packaged commit still FAILS", rc == 1 and "is missing commits" in out,
              f"rc={rc}: {out[-300:]}")
        rc, out = job_outcome("intigraph/quipu", "ian/ordinary-change")
        check("ordinary PR with the same CHANGELOG runs no verifier and passes",
              rc == 0 and "skipped by scope" in out, f"rc={rc}: {out}")

print(f"{'FAIL' if failures else 'OK'}: {len(failures)} failing check(s)")
sys.exit(1 if failures else 0)
