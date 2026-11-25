"""Independent Arm A judge (aegis-karb.4 deliverable 4).

Reads only a `mergebench --dump` file and `intent.json`. Ground truth is
derived from (base, ours, theirs) and the DECLARED INTENDED cardinality per
predicate. It never reads examples/mergebench/shapes.rs or the generator's
conflict labels, with one exception: alias-mint slots, which are invisible at
the triple level, are taken from the generator's alias log (class
"alias-mint"), and no shape is consulted to produce them.

Usage: judge.py <intent.json> <dump-seed*.json>... > report.json
"""
import json, sys
from collections import defaultdict

def parse(lines):
    slots = defaultdict(set)
    for line in lines:
        line = line.strip()
        if not line:
            continue
        s, rest = line[1:].split("> <", 1)
        p, o = rest.split("> ", 1)
        o = o.rsplit(" .", 1)[0]
        slots[(s, p)].add(o)
    return slots

def three_way(b, o, t):
    # keep base values neither side removed, plus each side's additions
    return (b & o & t) | (o - b) | (t - b)

def truth(dump, intent, ns):
    b, o, t = parse(dump["base"]), parse(dump["ours"]), parse(dump["theirs"])
    out = {}
    for slot in set(b) | set(o) | set(t):
        name = slot[1][len(ns):] if slot[1].startswith(ns) else None
        n = intent.get(name, {}).get("intended") if name else None
        if n is None:
            continue
        bv, ov, tv = b.get(slot, set()), o.get(slot, set()), t.get(slot, set())
        if ov == tv:
            continue
        if len(three_way(bv, ov, tv)) > n:
            out[slot] = "functional-divergence" if n == 1 else "cardinality-overflow"
        elif n == 1 and bv and ((not ov and tv != bv) or (not tv and ov != bv)):
            out[slot] = "delete-modify"
    for row in dump["generator_truth"]:
        if row["class"] == "alias-mint":
            out[(row["s"], row["p"])] = "alias-mint"
    return out

def intended_violations(merged_lines, intent, ns):
    bad = 0
    for (s, p), vals in parse(merged_lines).items():
        name = p[len(ns):] if p.startswith(ns) else None
        n = intent.get(name, {}).get("intended") if name else None
        if n is not None and len(vals) > n:
            bad += 1
    return bad

def main():
    spec = json.load(open(sys.argv[1]))
    ns, intent = spec["namespace"], spec["predicates"]
    report = []
    for path in sys.argv[2:]:
        d = json.load(open(path))
        tr = truth(d, intent, ns)
        gen = {(r["s"], r["p"]): r["class"] for r in d["generator_truth"]}
        row = {"seed": d["params"]["seed"], "operator_shapes": d["operator_shapes"] or "declared",
               "truth": len(tr), "truth_equals_generator": tr == gen, "arms": {}}
        for arm, a in d["arms"].items():
            if not a["available"]:
                row["arms"][arm] = None
                continue
            got = {tuple(x) for x in a["conflicts"]}
            tp, fp, fn = len(got & tr.keys()), len(got - tr.keys()), len(tr.keys() - got)
            row["arms"][arm] = {"tp": tp, "fp": fp, "fn": fn,
                "precision": round(tp / (tp + fp), 3) if tp + fp else None,
                "recall": round(tp / (tp + fn), 3) if tp + fn else None,
                "intended_cardinality_violations": intended_violations(a["merged"], intent, ns)}
        report.append(row)
    json.dump(report, sys.stdout, indent=1)

if __name__ == "__main__":
    main()
