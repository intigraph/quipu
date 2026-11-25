"""Run the temporal and as-of oracle cases through a native adapter.

Usage: run_native.py <oracle-dir> <merge_native_oracle binary> <out.json> [fork-promote|share]

Builds a native recipe for each temporal/as-of case from the case
INPUTS only (base/ours/theirs value sets, cardinality, and the named decision),
runs examples/merge_native_oracle, and scores with oracle.evaluate. Expected
current/history/transactions are never read when building a recipe.
"""
import json, subprocess, sys, copy
ORACLE_DIR, BIN = sys.argv[1], sys.argv[2]
PATH = sys.argv[4] if len(sys.argv) > 4 else "fork-promote"
sys.path.insert(0, ORACLE_DIR)
import oracle, temporal_cases as tc, asof_cases

T0, TO, TT, TR, TD = tc.T0, tc.TO, tc.TT, tc.TR, tc.TD

def step(at, before, after, start):
    rows = [{"value": v, "from": start, "to": None, "op": "retract"} for v in before if v not in after]
    rows += [{"value": v, "from": start, "to": None, "op": "assert"} for v in after if v not in before]
    return [{"at": at, "rows": rows}] if rows else []

def recipe(case):
    base, ours, theirs = case["base"], case["ours"], case["theirs"]
    dec = case["expected_report"].get("decision")  # the use case's named decision = an input
    r = {"schema": "quipu-native-input/v1", "case": case["case"], "path": "fork-promote",
         "max_count": 1 if case["card"] == "one" else None,
         "base": {"at": T0, "rows": [{"value": v, "from": T0, "to": None, "op": "assert"} for v in base]},
         "fork_at": "2026-01-01T01:00:00Z",
         "ours": step(TO, base, ours, TO), "theirs": step(TT, base, theirs, TT),
         "decision_at": TD, "resolution": None, "probes": []}
    if case["case"] == "C7":  # ours closes a at To and reclaims it at Tr (design C7)
        r["ours"] = [{"at": TO, "rows": [{"value": "a", "from": TO, "to": None, "op": "retract"}]},
                     {"at": TR, "rows": [{"value": "a", "from": TR, "to": None, "op": "assert"}]}]
    side = {"ours": ours, "theirs": theirs, "base": base, "both": sorted(set(ours) | set(theirs))}
    if dec in side:
        r["resolution"] = side[dec]
    elif dec == "value":
        r["resolution"] = ["d"]  # design's decided value for C2c / C4-value
    # continue / defer / None: no resolution
    if PATH == "share":
        r["path"] = "share"
        r["decision"] = dec
        r["resolution"] = ["d"] if dec == "value" else None
    if not r["base"]["rows"]:
        r["base"] = None
    return r

def judge(case, rec):
    if rec["base"] is None:
        return "UNREACHABLE", "adapter cannot write an empty base step (C3d has base=[])", None
    p = subprocess.run([BIN], input=json.dumps(rec).encode(), capture_output=True, timeout=120)
    if p.returncode != 0:
        err = p.stderr.decode()[-300:].strip()
        if "exactly one native conflict" in err:
            return "UNREACHABLE", "native records no conflict for this slot, so the case's decision cannot be submitted through fork_promote_resolved: " + err, None
        return "FAIL", "adapter error: " + err, None
    cap = json.loads(p.stdout)
    out = cap["native"]["outcome"]
    note = f"native {out}"
    if out == "error":
        note += f" ({cap['native'].get('error')})"
    res = oracle.evaluate(case, cap["capture"])
    rm = res.get("report_mismatches", {})
    facts_ok = not any(res.get(k) for k in ("missing_current", "extra_current", "missing_history", "extra_history")) \
        and res.get("transactions_match", True) is not False and res.get("as_of_matches", True) is not False
    absent = sorted(k for k, v in rm.items() if not v.get("present", True))
    wrong = {k: v for k, v in rm.items() if v.get("present", True)}
    if case["expected_report"].get("status") == "refused" and out not in ("refused", "error"):
        return "FAIL", f"expected refusal; {note}", cap
    if not facts_ok or wrong:
        detail = {k: res.get(k) for k in ("missing_current", "extra_current", "missing_history", "extra_history") if res.get(k)}
        return "FAIL", f"{note}; {json.dumps(detail)[:400]} wrong_report={json.dumps(wrong)[:300]}", cap
    if absent:
        return "PASS(facts)/report-UNREACHABLE", f"{note}; current+history+tx match; adapter does not project report {absent}", cap
    return "PASS", note, cap

rows = []
for case in tc.CASES:
    v, why, cap = judge(case, recipe(case))
    rows.append({"case": case["case"], "verdict": v, "why": why, "capture": cap})
# as-of case uses the committed input recipe (probes at receive/decide pins)
asof_in = json.load(open(ORACLE_DIR + "/native-c4-input.json"))
if PATH == "share":
    asof_in["path"] = "share"; asof_in["decision"] = "theirs"; asof_in["resolution"] = None
p = subprocess.run([BIN], input=json.dumps(asof_in).encode(), capture_output=True, timeout=120)
if p.returncode == 0:
    cap = json.loads(p.stdout); res = oracle.evaluate(asof_cases.CASE, cap["capture"])
    rows.append({"case": "C4-gate-as-of", "verdict": "PASS" if res["correct"] else "FAIL",
                 "why": f"native {cap['native']['outcome']}; " + ("" if res["correct"] else json.dumps(res)[:400]), "capture": cap})
else:
    rows.append({"case": "C4-gate-as-of", "verdict": "FAIL", "why": p.stderr.decode()[-300:], "capture": None})
json.dump(rows, open(sys.argv[3], "w"), indent=1)
for r in rows:
    print(f'{r["case"]:18} {r["verdict"]:30} {r["why"]}')
