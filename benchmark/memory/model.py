"""Synthetic edit scripts and explicit expected outcomes, separate from SQL merge."""

import copy
import datetime
import random

DECLARATIONS = {
    "status": (1, "string"),
    "valid-until": (1, "dateTime"),
    "evidence": (None, "string"),
    "applies-to": (None, "string"),
    "tags": (None, "string"),
}


def valid(key, value):
    cardinality, datatype = DECLARATIONS[key]
    values = [value] if cardinality == 1 else value
    if not isinstance(values, list) or any(not isinstance(v, str) for v in values):
        return False
    if cardinality != 1 and len(values) != len(set(values)):
        return False
    if datatype == "dateTime":
        try:
            parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
            return parsed.utcoffset() is not None
        except (ValueError, AttributeError):
            return False
    return True


def facts(metadata):
    return {
        (key, str(v))
        for key, value in metadata.items()
        for v in ([value] if DECLARATIONS[key][0] == 1 else value)
    }


def scenario(seed, copies=20):
    """Each case declares its own conflict keys and expected accepted metadata."""
    rng = random.Random(seed)
    cases = []
    for n in range(copies):
        suffix = str(rng.randrange(10**9))
        base = {
            "status": "active",
            "valid-until": "2026-12-01T00:00:00Z",
            "evidence": ["base"],
            "applies-to": ["memory-target"],
            "tags": ["base"],
        }
        specs = [
            (
                "different-keys",
                {"status": "retired"},
                {"valid-until": "2027-01-01T00:00:00Z"},
                set(),
                {"status": "retired", "valid-until": "2027-01-01T00:00:00Z"},
            ),
            ("single-divergent", {"status": "retired"}, {"status": "paused"}, {"status"}, {}),
            (
                "same-single-value",
                {"status": "retired"},
                {"status": "retired"},
                set(),
                {"status": "retired"},
            ),
            (
                "multi-add",
                {"tags": ["base", "left-" + suffix]},
                {"tags": ["base", "right-" + suffix]},
                set(),
                {"tags": ["base", "left-" + suffix, "right-" + suffix]},
            ),
            (
                "same-multi-add",
                {"evidence": ["base", suffix]},
                {"evidence": ["base", suffix]},
                set(),
                {"evidence": ["base", suffix]},
            ),
            (
                "multi-remove-add",
                {"tags": []},
                {"tags": ["base", suffix]},
                set(),
                {"tags": [suffix]},
            ),
            ("single-delete-edit", {"status": None}, {"status": "paused"}, {"status"}, {}),
            (
                "different-multi-keys",
                {"evidence": ["base", suffix]},
                {"applies-to": ["memory-target", suffix]},
                set(),
                {"evidence": ["base", suffix], "applies-to": ["memory-target", suffix]},
            ),
            (
                "mixed",
                {"status": "retired", "tags": ["base", "left-" + suffix]},
                {"status": "paused", "tags": ["base", "right-" + suffix]},
                {"status"},
                {"tags": ["base", "left-" + suffix, "right-" + suffix]},
            ),
            ("unchanged", {}, {}, set(), {}),
        ]
        for kind, left_edits, right_edits, conflicts, accepted in specs:
            versions = []
            for edits in ({}, left_edits, right_edits, accepted):
                meta = copy.deepcopy(base)
                for key, value in edits.items():
                    if value is None:
                        meta.pop(key, None)
                    else:
                        meta[key] = value
                versions.append(meta)
            changed = {
                k for k in base if versions[1].get(k) != base[k] or versions[2].get(k) != base[k]
            }
            cases.append(
                {
                    "id": f"memory-{n}-{kind}",
                    "case": kind,
                    "base": versions[0],
                    "left": versions[1],
                    "right": versions[2],
                    "expected": versions[3],
                    "conflicts": sorted(conflicts),
                    "changed": sorted(changed),
                }
            )
    rng.shuffle(cases)
    return cases


def rows(case, side):
    base_facts = facts(case["base"])
    return [
        (
            case["id"],
            key,
            "" if DECLARATIONS[key][0] == 1 else value,
            value,
            DECLARATIONS[key][1],
            "base" if (key, value) in base_facts else side,
        )
        for key, value in sorted(facts(case[side]))
    ]


def score(cases, outputs, conflicts, arm):
    """Conflicted units are held at base for fair automatic-output accounting."""
    truth = {(c["id"], k) for c in cases for k in c["conflicts"]}
    raised = set()
    lost = spurious = false_units = 0
    per_case = {}
    for case in cases:
        ident = case["id"]
        flagged = conflicts.get(ident, set())
        if arm == "typed-rows":
            false_units += len(flagged - set(case["conflicts"]))
        elif flagged:
            false_units += not bool(set(case["conflicts"]))
        if arm != "typed-rows" and flagged:
            flagged = set(case["changed"])
        raised.update((ident, k) for k in flagged)
        actual = outputs[ident]
        for key in flagged:
            actual = {f for f in actual if f[0] != key} | {
                f for f in facts(case["base"]) if f[0] == key
            }
        expected = facts(case["expected"])
        lost += len(expected - actual)
        spurious += len(actual - expected)
        counts = per_case.setdefault(case["case"], {"memories": 0, "flagged_memories": 0})
        counts["memories"] += 1
        counts["flagged_memories"] += bool(flagged)
    return {
        "true_conflict_slots": len(truth),
        "tp_slots": len(raised & truth),
        "fp_slots": len(raised - truth),
        "fn_slots": len(truth - raised),
        "decision_units": sum(
            len(v) if arm == "typed-rows" else bool(v) for v in conflicts.values()
        ),
        "false_decision_units": false_units,
        "triples_lost": lost,
        "triples_spurious": spurious,
        "by_case": per_case,
    }
