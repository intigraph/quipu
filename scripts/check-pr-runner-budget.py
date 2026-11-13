#!/usr/bin/env python3
"""Bound ordinary PR runner jobs while merge-group jobs bypass that budget.

# arming: ci Pre-commit checks; agent-invoked through just ci-pr-budget check.
"""
import argparse
from pathlib import Path
import sys
import yaml

PR_CANCEL_GUARD = "(github.event_name != 'pull_request' || !cancelled())"


class UniqueLoader(yaml.BaseLoader):
    def construct_mapping(self, node, deep=False):
        keys = [self.construct_object(k, deep=deep) for k, _ in node.value]
        if len(keys) != len(set(keys)):
            raise ValueError("duplicate workflow mapping key")
        return super().construct_mapping(node, deep=deep)


def budget_slot(group, job):
    """Accept the closed scheduling contract, including a distinct non-PR key."""
    prefix = "${{ github.event_name == 'pull_request' && 'quipu-pr-runner-"
    suffix = ("' || format('quipu-" + job + "-{0}-{1}-{2}-{3}', github.workflow, "
              "github.run_id, github.run_attempt, strategy.job-index) }}")
    if not isinstance(group, str) or not group.startswith(prefix) or not group.endswith(suffix):
        return None
    value = group[len(prefix):-len(suffix)]
    return int(value) if value in {"1", "2", "3", "4"} else None


def audit(documents):
    errors, slots, count = [], set(), 0
    for name, workflow in documents.items():
        events = workflow.get("on", {})
        if "pull_request" not in events:
            continue
        for job, config in workflow.get("jobs", {}).items():
            label = f"{name}:{job}"
            if "runs-on" not in config:
                errors.append(label + ": reusable/unknown runner placement needs an explicit budget audit")
                continue
            count += 1
            policy = config.get("concurrency", {})
            slot = budget_slot(policy.get("group"), job)
            if slot is None:
                errors.append(label + ": PR job must use one of four shared slots and a unique non-PR key")
            else:
                slots.add(slot)
            if policy.get("queue") != "max":
                errors.append(label + ": queue:max must retain pending jobs instead of replacing them")
            if str(policy.get("cancel-in-progress")).lower() != "false":
                errors.append(label + ": a budget slot must never cancel another PR job")
            condition = config.get("if", "")
            if "always()" in condition and PR_CANCEL_GUARD not in condition:
                errors.append(label + ": always jobs must release superseded PR runs")
    if not count or slots != {1, 2, 3, 4}:
        errors.append("complete PR runner population/four budget slots not established")
    ci = documents.get("ci.yml", {})
    policy = ci.get("concurrency", {})
    if ("${{ github.ref }}" not in policy.get("group", "")
            or policy.get("cancel-in-progress") != "${{ github.event_name == 'pull_request' }}"):
        errors.append("CI must supersede only older runs of the same PR")
    if "merge_group" not in ci.get("on", {}):
        errors.append("merge-group CI trigger is required")
    return errors, count


def load(root):
    return {p.name: yaml.load(p.read_text(), Loader=UniqueLoader)
            for p in sorted((root / ".github/workflows").glob("*.y*ml"))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        errors, count = audit(load(args.root))
    except (OSError, ValueError, TypeError, yaml.YAMLError) as error:
        print(f"PR budget UNKNOWN: {error}", file=sys.stderr)
        return 2
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"PASS {count} PR runner jobs share four FIFO slots; other events have distinct keys")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
