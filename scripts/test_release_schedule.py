#!/usr/bin/env python3
"""Check that daily proposal generation cannot publish or follow every push.

# arming: ci; the existing release-contract step runs these controls.
"""

import copy
from pathlib import Path
import yaml


def check(publication, proposal):
    assert set(proposal["on"]) == {"schedule", "workflow_dispatch"}, "proposals must not chase pushes"
    assert not proposal["on"]["workflow_dispatch"], "a manual proposal takes no inputs"
    assert proposal["on"]["schedule"] == [{"cron": "0 7 * * *"}]
    assert proposal["concurrency"]["cancel-in-progress"] is False
    jobs = proposal["jobs"]
    waiter = jobs["ci-correctness"]
    assert waiter["if"] == (
        "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'"
    )
    assert waiter["outputs"]["ready"] == "${{ steps.correctness.outputs.ready }}"
    gate = next(s for s in waiter["steps"] if s.get("id") == "correctness")
    assert gate["run"] == 'python3 scripts/wait_release_correctness.py "$GITHUB_SHA"'
    assert "if" not in gate and not gate.get("continue-on-error", False)
    job = jobs["release-pr"]
    assert job["needs"] == "ci-correctness"
    assert job["if"] == (
        "(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch')"
        " && needs.ci-correctness.outputs.ready == 'true'"
    )
    assert set(jobs) == {"ci-correctness", "release-pr"}
    actions = [s for s in job["steps"] if "release-plz-action@" in s.get("uses", "")]
    assert len(actions) == 1 and actions[0]["with"]["command"] == "release-pr"
    assert actions[0]["env"]["GITHUB_TOKEN"] == "${{ secrets.RELEASE_PLZ_TOKEN }}"
    assert "CARGO_REGISTRY_TOKEN" not in str(proposal), "proposal lane cannot publish"
    snapshot = next(s for s in job["steps"] if s.get("name") == "Require the measured main snapshot")
    assert 'test "$CURRENT" = "$GITHUB_SHA"' in snapshot["run"]
    assert job["steps"].index(snapshot) < job["steps"].index(actions[0])
    publisher = publication["jobs"]["release-plz"]
    publish_actions = [s for s in publisher["steps"] if "release-plz-action@" in s.get("uses", "")]
    assert len(publish_actions) == 1
    assert publish_actions[0]["with"]["command"] == "release"
    assert publish_actions[0]["id"] == "release"
    assert publish_actions[0]["env"]["GITHUB_TOKEN"] == "${{ secrets.GITHUB_TOKEN }}"
    assert "RELEASE_PLZ_TOKEN" not in str(publisher)


def main():
    # SafeLoader treats the event key `on` as a YAML 1.1 boolean.
    # Preserve semantic booleans while restoring that GitHub key.
    def load(path):
        result = yaml.safe_load(Path(path).read_text())
        result["on"] = result.pop(True, result.get("on"))
        return result

    publication = load(".github/workflows/release.yml")
    proposal = load(".github/workflows/release-pr-daily.yml")
    check(publication, proposal)
    for sabotage in ("push", "pull_request", "unready", "publish", "credential", "snapshot"):
        bad = copy.deepcopy(proposal)
        job = bad["jobs"]["release-pr"]
        action = next(s for s in job["steps"] if "release-plz-action@" in s.get("uses", ""))
        if sabotage == "push":
            bad["on"]["push"] = {"branches": ["main"]}
        elif sabotage == "pull_request":
            bad["on"]["pull_request"] = {}
        elif sabotage == "unready":
            job["if"] = "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'"
        elif sabotage == "publish":
            action["with"]["command"] = "release"
        elif sabotage == "credential":
            action["env"]["GITHUB_TOKEN"] = "${{ secrets.GITHUB_TOKEN }}"
        else:
            job["steps"] = [s for s in job["steps"] if s.get("name") != "Require the measured main snapshot"]
        try:
            check(publication, bad)
        except (AssertionError, StopIteration):
            print("PASS: refused", sabotage)
        else:
            raise AssertionError("unsafe scheduling mutation survived: " + sabotage)
    print("release scheduling: current positive and six unsafe-event controls pass")


if __name__ == "__main__":
    main()
