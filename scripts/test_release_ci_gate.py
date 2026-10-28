#!/usr/bin/env python3
"""Static contract connecting CI correctness to Release."""

from pathlib import Path
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

print("release CI gate contract: ok")
