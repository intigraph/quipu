"""ARM F: observed Dolt JSON/TEXT/triple-row merges and equal-answer SQL probes."""

import argparse
import collections
import datetime
import hashlib
import json
import random
import statistics
import time
from pathlib import Path

from pymysql.constants import FIELD_TYPE

from model import DECLARATIONS, facts, rows, scenario, score, valid
from runtime import server, sql

HERE = Path(__file__).resolve().parent
ARMS = {"json-native": "issues", "whole-cell-text": "issues_text", "typed-rows": "metadata_rows"}


def tables(connection):
    # Exact source migration, not a hand-written replacement for the JSON arm.
    sql(connection, (HERE / "beads-issues.sql").read_text())
    sql(
        connection,
        "CREATE TABLE issues_text (id VARCHAR(255) PRIMARY KEY, "
        "description TEXT NOT NULL, metadata LONGTEXT NOT NULL)",
    )
    sql(
        connection,
        "CREATE TABLE metadata_rows (bead_id VARCHAR(255), "
        "`key` VARCHAR(32), slot VARCHAR(255), value VARCHAR(255), "
        "datatype VARCHAR(32), set_in_version VARCHAR(32), "
        "PRIMARY KEY(bead_id, `key`, slot), INDEX key_value (`key`, value))",
    )


def load(connection, cases, side):
    for case in cases:
        metadata = json.dumps(case[side], sort_keys=True)
        if side == "base":
            sql(
                connection,
                "INSERT INTO issues "
                "(id,title,description,design,acceptance_criteria,notes,metadata,created_at,updated_at) "
                "VALUES (%s,'Synthetic Memory','unchanged body','','','',%s,"
                "'2026-01-01 00:00:00','2026-01-01 00:00:00')",
                (case["id"], metadata),
            )
            sql(
                connection,
                "INSERT INTO issues_text VALUES (%s,'unchanged body',%s)",
                (case["id"], metadata),
            )
        else:
            for table in ("issues", "issues_text"):
                extra = ",updated_at='2026-01-01 00:00:00'" if table == "issues" else ""
                sql(
                    connection,
                    f"UPDATE {table} SET metadata=%s{extra} WHERE id=%s",
                    (metadata, case["id"]),
                )
        write_rows(connection, case, side)


def write_rows(connection, case, side):
    if not all(valid(key, value) for key, value in case[side].items()):
        raise ValueError("declared metadata datatype/cardinality rejected before mutation")
    sql(connection, "DELETE FROM metadata_rows WHERE bead_id=%s", (case["id"],))
    with connection.cursor() as cursor:
        cursor.executemany("INSERT INTO metadata_rows VALUES (%s,%s,%s,%s,%s,%s)", rows(case, side))


def outputs(connection, arm):
    if arm == "typed-rows":
        result = collections.defaultdict(set)
        for ident, key, value in sql(connection, "SELECT bead_id,`key`,value FROM metadata_rows"):
            result[ident].add((key, value))
        return result
    return {
        ident: facts(json.loads(metadata))
        for ident, metadata in sql(connection, f"SELECT id,metadata FROM {ARMS[arm]}")
    }


def merge_probe(connection, seed, copies):
    cases = scenario(seed, copies)
    tables(connection)
    load(connection, cases, "base")
    sql(connection, "CALL DOLT_ADD('.')")
    sql(connection, "CALL DOLT_COMMIT('-m','base')")
    sql(connection, "CALL DOLT_BRANCH('base')")
    sql(connection, "CALL DOLT_CHECKOUT('-b','left')")
    load(connection, cases, "left")
    sql(connection, "CALL DOLT_COMMIT('-am','left')")
    sql(connection, "CALL DOLT_CHECKOUT('base')")
    sql(connection, "CALL DOLT_CHECKOUT('-b','right')")
    load(connection, cases, "right")
    sql(connection, "CALL DOLT_COMMIT('-am','right')")
    sql(connection, "CALL DOLT_CHECKOUT('left')")
    sql(connection, "SET @@dolt_allow_commit_conflicts=1")
    sql(connection, "SET @@dolt_force_transaction_commit=1")
    begin = time.perf_counter()
    merge_result = sql(connection, "CALL DOLT_MERGE('right')")
    elapsed = (time.perf_counter() - begin) * 1000
    conflict_tables = dict(sql(connection, "SELECT `table`,num_conflicts FROM dolt_conflicts"))
    arm_results = {}
    for arm, table in ARMS.items():
        conflict_keys = collections.defaultdict(set)
        raw = []
        if table in conflict_tables:
            fields = "COALESCE(our_bead_id,their_bead_id,base_bead_id), "
            fields += "COALESCE(our_key,their_key,base_key)" if arm == "typed-rows" else "''"
            if arm != "typed-rows":
                fields = "COALESCE(our_id,their_id,base_id),''"
            raw = sql(connection, f"SELECT {fields} FROM dolt_conflicts_{table} ORDER BY 1,2")
            for ident, key in raw:
                conflict_keys[ident].add(key)
        arm_results[arm] = score(cases, outputs(connection, arm), conflict_keys, arm)
        arm_results[arm]["native_conflict_rows"] = len(raw)
        assert len(raw) == arm_results[arm]["decision_units"], "fixture has one row per flagged key"
        arm_results[arm]["conflict_rows"] = raw
    digest = hashlib.sha256(json.dumps(cases, sort_keys=True).encode()).hexdigest()
    # Restore a common base before query experiments, not an arbitrarily resolved merge.
    sql(connection, "CALL DOLT_MERGE('--abort')")
    sql(connection, "CALL DOLT_CHECKOUT('base')")
    return {
        "seed": seed,
        "memories": len(cases),
        "input_sha256": digest,
        "cases": cases,
        "merge_result": merge_result,
        "merge_ms": elapsed,
        "arms": arm_results,
    }


def timing(connection, query, expected, repeats):
    first = sql(connection, query)
    assert sorted(first) == sorted(expected), (query, first[:3], expected[:3])
    # Dolt 2.1.8 EXPLAIN emits text 'NULL' in numeric estimate fields.
    previous = connection.decoders.copy()
    try:
        for code in (FIELD_TYPE.LONG, FIELD_TYPE.LONGLONG):
            connection.decoders[code] = lambda v: None if v == "NULL" else int(v)
        plan = sql(connection, "EXPLAIN " + query)
    finally:
        connection.decoders = previous
    samples = []
    for _ in range(repeats):
        begin = time.perf_counter()
        result = sql(connection, query)
        samples.append((time.perf_counter() - begin) * 1000)
        assert sorted(result) == sorted(expected)
    return {
        "sql": query,
        "answer_count": len(expected),
        "explain": plan,
        "samples_ms": samples,
        "median_ms": statistics.median(samples),
        "min_ms": min(samples),
        "max_ms": max(samples),
    }


def query_probe(connection, seed, size, repeats):
    for table in ARMS.values():
        sql(connection, "DELETE FROM " + table)
    sql(
        connection,
        "CREATE TABLE links (source VARCHAR(255), target VARCHAR(255), "
        "PRIMARY KEY(source,target), INDEX target_idx(target))",
    )
    rng = random.Random(seed)
    cases, active, tagged, traversed = [], [], [], []
    edges = []
    for n in range(size):
        ident = f"query-{n:06d}"
        is_active = rng.random() < 0.2
        tag = "selected" if rng.random() < 0.1 else "other"
        meta = {
            "status": "active" if is_active else "retired",
            "valid-until": "2026-12-01T00:00:00Z",
            "tags": [tag],
            "evidence": [f"source-{n}"],
            "applies-to": [f"target-{n % 10}"],
        }
        cases.append({"id": ident, "base": meta})
        edges.append((ident, f"target-{n % 10}"))
        if is_active:
            active.append((ident,))
        if tag == "selected":
            tagged.append((ident,))
        if is_active and n % 10 == 3:
            traversed.append((ident, f"target-{n % 10}"))
    load(connection, cases, "base")
    with connection.cursor() as cursor:
        cursor.executemany("INSERT INTO links VALUES (%s,%s)", edges)
    assert len(active) and len(tagged) and len(traversed)
    coverage = {
        "memories": sql(connection, "SELECT COUNT(*) FROM issues")[0][0],
        "facts": sql(connection, "SELECT COUNT(*) FROM metadata_rows")[0][0],
        "links": sql(connection, "SELECT COUNT(*) FROM links")[0][0],
    }
    assert coverage == {"memories": size, "facts": 5 * size, "links": size}
    status = "JSON_UNQUOTE(JSON_EXTRACT(i.metadata,'$.status'))='active'"
    queries = {
        "status": (
            f"SELECT i.id FROM issues i WHERE {status}",
            "SELECT bead_id FROM metadata_rows WHERE `key`='status' AND value='active'",
            active,
        ),
        "tag": (
            "SELECT id FROM issues WHERE JSON_CONTAINS(JSON_EXTRACT(metadata,'$.tags'), '\"selected\"')",
            "SELECT bead_id FROM metadata_rows WHERE `key`='tags' AND value='selected'",
            tagged,
        ),
        "traversal": (
            "SELECT i.id,l.target FROM issues i JOIN links l ON l.source=i.id "
            f"WHERE {status} AND l.target='target-3' AND "
            "JSON_CONTAINS(JSON_EXTRACT(i.metadata,'$.\"applies-to\"'),JSON_QUOTE(l.target))",
            "SELECT s.bead_id,l.target FROM metadata_rows s JOIN links l ON l.source=s.bead_id "
            "JOIN metadata_rows a ON a.bead_id=s.bead_id AND a.`key`='applies-to' AND a.value=l.target "
            "WHERE s.`key`='status' AND s.value='active' AND l.target='target-3'",
            traversed,
        ),
    }
    return {
        "coverage": coverage,
        "queries": {
            name: {
                "json-native": timing(connection, jq, expected, repeats),
                "typed-rows": timing(connection, rq, expected, repeats),
            }
            for name, (jq, rq, expected) in queries.items()
        },
    }


def type_probe(connection):
    invalid = [
        ("status", 42),
        ("valid-until", "yesterday"),
        ("tags", "tag"),
        ("evidence", [False]),
        ("applies-to", [42]),
    ]
    before = sql(connection, "SELECT * FROM metadata_rows ORDER BY bead_id,`key`,slot")
    ident, original = sql(connection, "SELECT id,metadata FROM issues ORDER BY id LIMIT 1")[0]
    accepted = 0
    for key, value in invalid:
        candidate = {"id": "invalid-control", "base": {}, "left": {key: value}}
        try:
            write_rows(connection, candidate, "left")
        except ValueError:
            pass
        else:
            raise AssertionError("invalid write was not refused")
        sql(
            connection,
            "UPDATE issues SET metadata=%s WHERE id=%s",
            (json.dumps({key: value}), ident),
        )
        observed = sql(connection, "SELECT metadata FROM issues WHERE id=%s", (ident,))[0][0]
        assert json.loads(observed) == {key: value}
        accepted += 1
    sql(connection, "UPDATE issues SET metadata=%s WHERE id=%s", (original, ident))
    assert before == sql(connection, "SELECT * FROM metadata_rows ORDER BY bead_id,`key`,slot")
    return {
        "invalid_candidates": len(invalid),
        "prototype_writer_refused": len(invalid),
        "native_json_accepts": accepted,
        "scope": "Python declaration guard, not Dolt native SHACL enforcement; configurable JSON guard also possible",
    }


def controls(result):
    arms = result["arms"]
    for name, arm in arms.items():
        assert arm["tp_slots"] > 0 and arm["fn_slots"] == 0, name
        assert arm["by_case"]["unchanged"]["flagged_memories"] == 0, name
    assert arms["json-native"]["by_case"]["different-keys"]["flagged_memories"] == 0
    assert arms["whole-cell-text"]["by_case"]["different-keys"]["flagged_memories"] > 0
    assert arms["typed-rows"]["by_case"]["multi-add"]["flagged_memories"] == 0
    assert scenario(result["seed"]) == scenario(result["seed"])
    assert scenario(result["seed"]) != scenario(result["seed"] + 1)
    for case in result["cases"]:
        recovered = {
            key
            for key, (bound, _) in DECLARATIONS.items()
            if bound == 1
            and case["left"].get(key) != case["base"].get(key)
            and case["right"].get(key) != case["base"].get(key)
            and case["left"].get(key) != case["right"].get(key)
        }
        assert recovered == set(case["conflicts"])
    tiny = scenario(1, 1)
    expected = {c["id"]: facts(c["expected"]) for c in tiny}
    missed = score(tiny, expected, {}, "typed-rows")
    assert missed["fn_slots"] == 3 and missed["triples_lost"] == 0
    false = score(tiny, expected, {"memory-0-unchanged": {"tags"}}, "typed-rows")
    assert false["fp_slots"] == 1
    damaged = {key: set(value) for key, value in expected.items()}
    damaged["memory-0-unchanged"].remove(("status", "active"))
    damaged["memory-0-unchanged"].add(("status", "fabricated"))
    counted = score(tiny, damaged, {}, "typed-rows")
    assert counted["triples_lost"] == 1 and counted["triples_spurious"] == 1
    for key, value in [
        ("status", "active"),
        ("valid-until", "2026-12-01T00:00:00Z"),
        ("tags", ["a", "b"]),
    ]:
        assert valid(key, value)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--seeds", default="1,7,42,99,2026")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--port", type=int, default=13318)
    parser.add_argument(
        "--scratch", required=True, help="fresh local fixture root (never production)"
    )
    parser.add_argument("--size", type=int, default=2000)
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--output", type=Path, default=HERE / "out")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now(datetime.UTC).strftime("%Y%m%dT%H%M%S%fZ")
    root = Path(args.scratch) / stamp
    seeds = [42] if args.selftest else [int(s) for s in args.seeds.split(",")]
    results = []
    for seed in seeds:
        with server(root, seed, args.port) as (connection, identity):
            result = merge_probe(connection, seed, 2 if args.selftest else 20)
            result["source_sha256"] = {
                name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
                for name in ("run.py", "model.py", "runtime.py", "beads-issues.sql", "uv.lock")
            }
            controls(result)
            result["typing"] = type_probe(connection)
            result["query"] = query_probe(
                connection,
                seed,
                200 if args.selftest else args.size,
                2 if args.selftest else args.repeats,
            )
            result["runtime"] = identity
        assert identity["stopped"]
        (args.output / f"seed-{seed}.json").write_text(json.dumps(result, indent=2) + "\n")
        print(
            json.dumps(
                {
                    "seed": seed,
                    "arms": {
                        a: {k: v for k, v in m.items() if k not in ("by_case", "conflict_rows")}
                        for a, m in result["arms"].items()
                    },
                    "fixture_stopped": identity["stopped"],
                }
            ),
            flush=True,
        )
        results.append(result)
    (args.output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    print(
        "PASS: merge controls, typed declaration controls, equal SQL answers/coverage, fixture cleanup"
    )


if __name__ == "__main__":
    main()
