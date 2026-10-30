#!/usr/bin/env python3
"""Static contract connecting CI correctness to Release."""

from pathlib import Path
import os
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
    assert doc["concurrency"]["cancel-in-progress"] is True

# aegis-0zr: the cut is verified BEFORE release-plz tags, on the guarded state.
cut = [s for s in steps if s.get("name") == "Refuse to cut a version whose section misses packaged commits"]
assert len(cut) == 1, "pre-cut changelog gate required"
assert cut[0]["if"] == "steps.guard.outputs.state == 'release'"
assert './scripts/verify-changelog.sh --version "$VERSION"' in cut[0]["run"]
assert not cut[0].get("continue-on-error", False)
guard = next(i for i, s in enumerate(steps) if s.get("id") == "guard")
assert guard < steps.index(cut[0]) < first_action, "gate must sit between guard and release-plz"

# aegis-0zr: the open release PR is re-validated against main on every push.
fresh = release_jobs["release-pr-freshness"]
assert fresh["if"] == "github.event_name == 'push'"
assert "needs" not in fresh, "freshness must not wait on CI"
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
    [ "$CASE" != discovery-error ] || { echo 'synthetic discovery failure' >&2; exit 1; }
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
    echo 1 ;;
  'workflow run') echo 'unexpected dispatch' >&2; exit 99 ;;
  *) echo "unexpected gh call: $*" >&2; exit 99 ;;
esac
''')
    gh.chmod(0o700)
    git = fixture / "git"
    git.write_text('''#!/bin/bash
set -eu
case "$1" in
  fetch) [ "$CASE" != fetch-error ] ;;
  rev-parse) if [ "$CASE" = fetch-mismatch ]; then echo "$OTHER"; else echo "$OID"; fi ;;
  *) exit 99 ;;
esac
''')
    git.chmod(0o700)
    for case in ("empty", "discovery-error", "discovery-malformed", "fetch-error", "fetch-mismatch",
                 "main-error", "head-error", "main-empty", "head-empty",
                 "main-moved", "head-moved", "success", "check-error", "status-error",
                 "stale-busy", "busy-error"):
        status = fixture / "status.log"
        status.unlink(missing_ok=True)
        env = {**os.environ, "PATH": str(fixture) + os.pathsep + os.environ["PATH"],
               "CASE": case, "OID": "a" * 40, "OTHER": "b" * 40,
               "GITHUB_SHA": "c" * 40, "GITHUB_REPOSITORY": "example/repo",
               "GITHUB_SERVER_URL": "https://example.invalid", "GITHUB_RUN_ID": "1",
               "GITHUB_STEP_SUMMARY": str(fixture / "summary"),
               "STATUS_LOG": str(status), "CHECK_EXIT": (
                   "2" if case == "check-error" else "1" if case in ("stale-busy", "busy-error") else "0")}
        result = subprocess.run(["bash", "-c", fresh_script], cwd=fixture,
                                env=env, capture_output=True, text=True)
        expected_error = case.endswith("error") or case in (
            "discovery-malformed", "fetch-mismatch", "main-empty", "head-empty")
        assert (result.returncode != 0) == expected_error, (case, result)
        assert ("No open release PR" in result.stdout) == (case == "empty"), (case, result)
        assert status.exists() == (case in (
            "success", "check-error", "status-error", "stale-busy", "busy-error")), (case, result)
        assert "unexpected dispatch" not in result.stderr, (case, result)
        if status.exists():
            record = status.read_text()
            assert "statuses/" + "a" * 40 in record
            expected_state = ("error" if case == "check-error" else
                              "failure" if case in ("stale-busy", "busy-error") else "success")
            assert "state=" + expected_state in record
        print(f"release PR discovery control: {case}: ok")

# aegis-0zr: the trusted share is its own job; crates.io never waits on it.
share = release_jobs["repository-share"]
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
