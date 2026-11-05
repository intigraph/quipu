#!/usr/bin/env python3
"""Static contract connecting CI correctness to Release."""

from pathlib import Path
import json
import os
import re
import subprocess
import tempfile
import yaml
from wait_release_correctness import DEFERRED_JOB


ci = Path(".github/workflows/ci.yml").read_text()
release = Path(".github/workflows/release.yml").read_text()

assert "  release-correctness:\n" in ci
assert "    name: Release correctness\n" in ci
assert "needs: [clippy, test, build, check, shacl-write-path, generated-docs]" in ci
aggregate_header = ci[ci.index("  release-correctness:\n") :].split("    runs-on:", 1)[0]
for excluded in ("source-size", "fmt", "lint-markdown", "build-full", "wasm", "shapes"):
    assert excluded not in aggregate_header, f"housekeeping job {excluded} must not gate release"

assert "  extended-correctness:\n" in ci
assert "    name: Extended correctness\n" in ci
extended_header = ci[ci.index("  extended-correctness:\n") :].split("    runs-on:", 1)[0]
for required in (
    "extended-features",
    "build-full",
    "wasm",
    "source-size",
    "load-test",
    "lint-markdown",
):
    assert required in extended_header, f"extended surface {required} must remain covered"

# Reject duplicate keys rather than letting a later value hide a safety gate.
class UniqueSafeLoader(yaml.SafeLoader):
    def construct_mapping(self, node, deep=False):
        self.flatten_mapping(node)
        keys = [self.construct_object(key, deep=deep) for key, _ in node.value]
        assert len(keys) == len(set(keys)), "duplicate workflow mapping keys"
        return super().construct_mapping(node, deep=deep)


jobs = yaml.load(ci, Loader=UniqueSafeLoader)["jobs"]
combined = jobs["source-size"]
assert "if" not in combined, "combined invariants must not be conditional"
assert combined.get("continue-on-error", False) is False, "combined failures must gate"
write_path = jobs["shacl-write-path"]
assert "if" not in write_path, "SHACL write-path conformance must not be conditional"
assert write_path.get("continue-on-error", False) is False, "write-path divergence must gate release"
generated = jobs["generated-docs"]
assert "if" not in generated, "generated-docs drift must not be conditional"
assert generated.get("continue-on-error", False) is False, "generated-docs drift must gate release"
assert any("quipu-docgen -- --check" in step.get("run", "") for step in generated["steps"])
aggregate = jobs["extended-correctness"]
assert "source-size" in aggregate["needs"], "combined invariants must gate the aggregate"
assert aggregate.get("continue-on-error", False) is False, "aggregate failures must gate"
for name, command in (
    ("Self-test the invariant checker", "python3 shapes/verify_shape_invariants.py --selftest"),
    ("Verify shape invariants (static)", "python3 shapes/verify_shape_invariants.py"),
):
    matches = [step for step in combined["steps"] if step.get("name") == name]
    assert len(matches) == 1, f"exactly one shape step required: {name}"
    step = matches[0]
    assert step.get("run", "").strip() == command, f"shape command missing: {name}"
    assert "if" not in step, f"shape step must be unconditional: {name}"
    assert step.get("continue-on-error", False) is False, f"shape failure must gate: {name}"

assert "  ci-correctness:\n" in release
assert "python3 scripts/test_wait_release_correctness.py" in release
assert "python3 scripts/test_release_ci_gate.py" in release
assert 'python3 scripts/wait_release_correctness.py "$GITHUB_SHA"' in release
release_plz = release[release.index("  release-plz:\n") :]
assert "    needs: ci-correctness\n" in release_plz.split("    steps:\n", 1)[0]
assert "  workflow_dispatch:\n" in release, "manual artifact recovery must remain available"

release_jobs = yaml.load(release, Loader=UniqueSafeLoader)["jobs"]
waiter = release_jobs["ci-correctness"]
assert waiter.get("continue-on-error", False) is False
assert waiter["outputs"]["ready"] == "${{ steps.correctness.outputs.ready }}"
gate_steps = [step for step in waiter["steps"] if step.get("id") == "correctness"]
assert len(gate_steps) == 1
assert gate_steps[0]["run"] == 'python3 scripts/wait_release_correctness.py "$GITHUB_SHA"'
assert "if" not in gate_steps[0] and not gate_steps[0].get("continue-on-error", False)
assert release_jobs["release-plz"]["if"] == (
    "github.event_name == 'push' && needs.ci-correctness.outputs.ready == 'true'"
), "only explicit readiness permits publication"
deferred = release_jobs["release-deferred"]
assert deferred["name"] == DEFERRED_JOB
assert deferred["needs"] == "ci-correctness"
assert deferred["if"] == (
    "github.event_name == 'push' && needs.ci-correctness.outputs.ready == 'false'"
)
rearm = yaml.load(Path(".github/workflows/release-ci-ready.yml").read_text(), Loader=UniqueSafeLoader)
trigger = rearm.get("on", rearm.get(True))["workflow_run"]
assert trigger == {"workflows": ["CI", "Release"], "types": ["completed"], "branches": ["main"]}
assert rearm["concurrency"]["cancel-in-progress"] is False
job = rearm["jobs"]["rearm"]
assert " ".join(job["if"].split()) == (
    "github.event.workflow_run.event == 'push' && "
    "github.event.workflow_run.head_repository.full_name == github.repository"
)
checkout = next(step for step in job["steps"] if step.get("uses", "").startswith("actions/checkout@"))
assert checkout["with"] == {
    "ref": "${{ github.event.repository.default_branch }}", "persist-credentials": False,
}, "privileged workflow_run must execute trusted default-branch code"
assert job["steps"][-1]["run"] == 'python3 scripts/rearm_release_correctness.py "$HEAD_SHA"'
assert "python3 scripts/test_rearm_release_correctness.py" in ci
steps = release_jobs["release-plz"]["steps"]
policies = [step for step in steps if step.get("run", "").strip() == "python3 scripts/check-semver-policy.py"]
assert len(policies) == 1, "semver exception must be bounded at release time"
assert "if" not in policies[0] and not policies[0].get("continue-on-error", False)
first_action = next(i for i, step in enumerate(steps) if "release-plz-action@" in step.get("uses", ""))
assert steps.index(policies[0]) < first_action, "policy must run before release-plz"
assert "python3 scripts/test_semver_policy.py" in ci
assert "python3 scripts/check-semver-policy.py" in ci

# aegis-0zr: the release commit's own CI and Release runs are never cancelled by
# a later push. Both workflows must carry the SAME exemption suffix.
EXEMPT = ("${{ startsWith(github.event.head_commit.message, 'chore: release v') "
          "&& format('-{0}', github.sha) || '' }}")
ci_doc = yaml.load(ci, Loader=UniqueSafeLoader)
release_doc = yaml.load(release, Loader=UniqueSafeLoader)
for name, doc in (("ci.yml", ci_doc), ("release.yml", release_doc)):
    group = doc["concurrency"]["group"]
    assert group.endswith(EXEMPT), f"{name}: release commits need their own concurrency group"
    assert doc["concurrency"]["cancel-in-progress"] == "${{ github.event_name == 'pull_request' }}", \
        f"{name}: only superseded PR runs may cancel; main pushes must finish"

# aegis-0zr: the cut is verified BEFORE release-plz tags, on the guarded state.
cut = [s for s in steps if s.get("name") == "Refuse to cut a version whose section misses packaged commits"]
assert len(cut) == 1, "pre-cut changelog gate required"
assert cut[0]["if"] == "steps.guard.outputs.state == 'release'"
assert './scripts/verify-changelog.sh --version "$VERSION"' in cut[0]["run"]
assert not cut[0].get("continue-on-error", False)
guard = next(i for i, s in enumerate(steps) if s.get("id") == "guard")
assert guard < steps.index(cut[0]) < first_action, "gate must sit between guard and release-plz"

# aegis-0zr: the open release PR is re-validated whenever main or its own head
# moves. Its own workflow, because release.yml never runs for a regenerated head.
assert "release-pr-freshness" not in release_jobs
fresh_doc = yaml.load(Path(".github/workflows/release-pr-freshness.yml").read_text(),
                      Loader=UniqueSafeLoader)
fresh_on = fresh_doc.get("on", fresh_doc.get(True))
assert fresh_on["push"]["branches"] == ["main"]
assert set(fresh_on["pull_request"]["types"]) >= {"opened", "synchronize", "reopened"}
assert fresh_doc["permissions"] == {}
# A pending main-push verdict must never be replaced by a pull_request run, or the
# release PR keeps a stale success (#910 review). Keyed by event and head ref, no
# cancel-in-progress, and no job-level group that would re-share one queue.
fresh_group = fresh_doc["concurrency"]["group"]
assert "${{ github.event_name }}" in fresh_group, "freshness group must be keyed by event"
assert "github.event.pull_request.head.ref || github.ref" in fresh_group, \
    "freshness group must be keyed by head ref"
assert fresh_doc["concurrency"].get("cancel-in-progress") is False
fresh = fresh_doc["jobs"]["release-pr-freshness"]
assert "concurrency" not in fresh, "a job-level group would re-share one queue"
# No job-level `if:`: a job skipped on every ordinary PR leaves a skipped check the
# merge gate cannot prove (#910 review). The scope test is the first step and gates
# every later step, so out of scope the job is green having done nothing.
assert "if" not in fresh, "freshness must not skip as a job; scope it in its first step"
scope = fresh["steps"][0]
assert scope.get("id") == "scope" and "if" not in scope
scope_expr = " ".join(scope["env"]["IN_SCOPE"].split())
for clause in ("github.event_name == 'push'",
               "github.event.pull_request.head.repo.full_name == github.repository",
               "startsWith(github.event.pull_request.head.ref, 'release-plz')"):
    assert clause in scope_expr, f"scope must test {clause}"
for step in fresh["steps"][1:]:
    assert step.get("if") == "steps.scope.outputs.in_scope == 'true'", \
        f"every step after the scope test must be scoped: {step.get('name') or step.get('uses')}"
with tempfile.TemporaryDirectory() as directory:
    for value, code, out in (("true", 0, "in_scope=true"), ("false", 0, "in_scope=false"),
                             ("", 1, None), ("maybe", 1, None)):
        output = Path(directory) / f"out-{value or 'empty'}"
        output.write_text("")
        run = subprocess.run(["bash", "-c", scope["run"]], capture_output=True, text=True,
                             env={"PATH": os.environ["PATH"], "IN_SCOPE": value,
                                  "GITHUB_OUTPUT": str(output)})
        assert run.returncode == code, (value, run.returncode, run.stdout, run.stderr)
        if out:
            assert output.read_text().strip() == out, (value, output.read_text())
assert fresh["name"] != "Release PR freshness", "the job check must not shadow the status"
assert "needs" not in fresh, "freshness must not wait on CI"
checkout = next(s for s in fresh["steps"] if s.get("uses", "").startswith("actions/checkout"))
assert "'main'" in checkout["with"]["ref"], "a pull_request run checks out main, not the PR"
assert fresh["permissions"]["statuses"] == "write"
fresh_run = "\n".join(s.get("run", "") for s in fresh["steps"])
assert "scripts/release-pr-freshness.sh" in fresh_run
assert 'context="Release PR freshness"' in fresh_run

# Execute the actual workflow body: unreadable discovery is not known absence,
# and unreadable current refs must never be treated as an ordinary moved ref.
fresh_script = next(s["run"] for s in fresh["steps"]
                    if s.get("name") == "Check the open release PR against current main")
with tempfile.TemporaryDirectory() as directory:
    fixture = Path(directory)
    (fixture / "scripts").mkdir()
    freshness = fixture / "scripts/release-pr-freshness.sh"
    freshness.write_text('#!/bin/sh\nexit "${CHECK_EXIT:-0}"\n')
    freshness.chmod(0o700)
    gh = fixture / "gh"
    gh.write_text('''#!/bin/bash
set -eu
case "$1 $2" in
  'pr list')
    case "$CASE" in pr-*) echo 'a pull_request run must not discover' >&2; exit 99 ;; esac
    [ "$CASE" != discovery-error ] || { echo 'synthetic discovery failure' >&2; exit 1; }
    [ "$CASE" != multiple ] || { echo MULTIPLE; exit 0; }
    [ "$CASE" != empty ] || { printf '  \\n'; exit 0; }
    [ "$CASE" != discovery-malformed ] || { echo release-plz-test; exit 0; }
    printf 'release-plz-test %s 7\\n' "$OID" ;;
  'pr view')
    [ "$CASE" != head-error ] || exit 1
    [ "$CASE" != head-empty ] || exit 0
    if [ "$CASE" = head-moved ]; then echo "$OTHER"; else echo "$OID"; fi ;;
  'api repos/example/repo/commits/main')
    [ "$CASE" != main-error ] || exit 1
    [ "$CASE" != main-empty ] || exit 0
    if [ "$CASE" = main-moved ]; then echo "$OTHER"; else echo "$GITHUB_SHA"; fi ;;
  'api repos/example/repo/statuses/'*)
    printf '%s\\n' "$*" >> "$STATUS_LOG"
    [ "$CASE" != status-error ] ;;
  'run list')
    [ "$CASE" != busy-error ] || exit 1
    [ "$CASE" != busy-empty ] || exit 0
    [ "$CASE" != busy-text ] || { echo many; exit 0; }
    case "$CASE" in stale-idle|dispatch-error) echo 0 ;; *) echo 1 ;; esac ;;
  'workflow run')
    case "$CASE" in stale-idle|dispatch-error) ;; *) echo 'unexpected dispatch' >&2; exit 99 ;; esac
    [ "$CASE" != dispatch-error ] || { echo 'synthetic dispatch failure' >&2; exit 1; }
    echo dispatched >> "$STATUS_LOG.dispatch" ;;
  *) echo "unexpected gh call: $*" >&2; exit 99 ;;
esac
''')
    gh.chmod(0o700)
    git = fixture / "git"
    git.write_text('''#!/bin/bash
set -eu
case "$1" in
  fetch) [ "$CASE" != fetch-error ] ;;
  rev-parse)
    if [ "$2" = HEAD ]; then echo "$GITHUB_SHA"
    elif [ "$CASE" = fetch-mismatch ]; then echo "$OTHER"; else echo "$OID"; fi ;;
  *) exit 99 ;;
esac
''')
    git.chmod(0o700)
    for case in ("empty", "discovery-error", "discovery-malformed", "fetch-error", "fetch-mismatch",
                 "main-error", "head-error", "main-empty", "head-empty",
                 "main-moved", "head-moved", "success", "check-error", "status-error",
                 "stale-busy", "busy-error", "busy-empty", "busy-text", "stale-idle",
                 "dispatch-error", "multiple", "pr-success", "pr-stale-busy"):
        status = fixture / "status.log"
        status.unlink(missing_ok=True)
        env = {**os.environ, "PATH": str(fixture) + os.pathsep + os.environ["PATH"],
               "CASE": case, "OID": "a" * 40, "OTHER": "b" * 40,
               "GITHUB_SHA": "c" * 40, "GITHUB_REPOSITORY": "example/repo",
               "GITHUB_SERVER_URL": "https://example.invalid", "GITHUB_RUN_ID": "1",
               "GITHUB_STEP_SUMMARY": str(fixture / "summary"),
               "STATUS_LOG": str(status),
               "GITHUB_EVENT_NAME": "pull_request" if case.startswith("pr-") else "push",
               "PR_HEAD_REF": "release-plz-test", "PR_HEAD_SHA": "a" * 40, "PR_NUMBER": "7",
               "CHECK_EXIT": (
                   "2" if case == "check-error" else "1" if case in (
                       "stale-busy", "busy-error", "busy-empty", "busy-text", "stale-idle",
                       "dispatch-error", "pr-stale-busy") else "0")}
        Path(str(status) + ".dispatch").unlink(missing_ok=True)
        result = subprocess.run(["bash", "-c", fresh_script], cwd=fixture,
                                env=env, capture_output=True, text=True)
        expected_error = case.endswith("error") or case in (
            "discovery-malformed", "fetch-mismatch", "main-empty", "head-empty",
            "busy-empty", "busy-text", "multiple")
        assert (result.returncode != 0) == expected_error, (case, result)
        assert ("No open release PR" in result.stdout) == (case == "empty"), (case, result)
        assert status.exists() == (case in (
            "success", "check-error", "status-error", "stale-busy", "busy-error",
            "busy-empty", "busy-text", "stale-idle", "dispatch-error", "pr-success",
            "pr-stale-busy")), (case, result)
        assert Path(str(status) + ".dispatch").exists() == (case == "stale-idle"), (case, result)
        assert "unexpected dispatch" not in result.stderr, (case, result)
        if status.exists():
            record = status.read_text()
            assert "statuses/" + "a" * 40 in record
            expected_state = ("error" if case == "check-error" else
                              "failure" if case in ("stale-busy", "busy-error", "busy-empty",
                                                    "busy-text", "stale-idle", "dispatch-error",
                                                    "pr-stale-busy")
                              else "success")
            assert "state=" + expected_state in record
        print(f"release PR discovery control: {case}: ok")

# The discovery filter itself, run by jq: fork PRs and other bases never count,
# two release PRs are ambiguous, and nothing is capped at the first page.
filter_text = re.search(r"--jq '(\[\.\[\] \| select\(\(\.isCrossRepository.*?end)'", fresh_script, re.S)
assert filter_text, "discovery filter not found"
assert "--limit 1000" in fresh_script and "--base main" in fresh_script
def discover(prs):
    return subprocess.run(["jq", "-r", filter_text.group(1)], input=json.dumps(prs),
                          capture_output=True, text=True, check=True).stdout.strip()
ours = {"headRefName": "release-plz-x", "headRefOid": "a" * 40, "number": 9, "isCrossRepository": False}
fork = {**ours, "headRefOid": "f" * 40, "number": 10, "isCrossRepository": True}
other = [{"headRefName": f"feature-{n}", "headRefOid": "e" * 40, "number": 100 + n,
          "isCrossRepository": False} for n in range(31)]
assert discover([fork, ours]) == f"release-plz-x {'a' * 40} 9"
assert discover([fork]) == ""
assert discover([ours, {**ours, "number": 11}]) == "MULTIPLE"
assert discover(other + [ours]) == f"release-plz-x {'a' * 40} 9"
print("release PR discovery filter: ok")

# aegis-0zr: the trusted share is its own job; crates.io never waits on it.
share = release_jobs["repository-share"]
# ubuntu-22.04's python3 is 3.10; the share's tomllib steps need 3.11+.
py = next(s for s in share["steps"] if s.get("uses", "").startswith("actions/setup-python"))
assert tuple(int(x) for x in str(py["with"]["python-version"]).split(".")) >= (3, 11)
assert share["steps"].index(py) < next(i for i, s in enumerate(share["steps"])
                                       if "tomllib" in s.get("run", ""))
assert set(share["needs"]) == {"release-plz", "artifacts"}
assert "TRUSTED_REPOSITORY_SHARE_ENABLED == 'true'" in share["if"]
assert any(s.get("name") == "Require the trusted repository share" for s in share["steps"])
for step in release_jobs["artifacts"]["steps"]:
    assert step.get("name") != "Require the trusted repository share", \
        "a missing share must not fail Binaries (it skipped crates.io for 0.16.4/0.16.5)"
assert "repository-share" not in release_jobs["crates"]["needs"]
assert "repository-share" not in release_jobs["mcp-registry"]["needs"]
missing_crate = release_jobs["crates-not-published"]
assert "needs.crates.result != 'success'" in missing_crate["if"]
assert "releases_created == 'true'" in missing_crate["if"]

print("release CI gate contract: ok")
