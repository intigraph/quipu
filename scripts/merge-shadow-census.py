#!/usr/bin/env python3
"""Bounded retrospective candidates from a retained Quipu snapshot, never a writer.

# arming: library; offline retained-copy analysis, no hook or schedule installed.
This is not merge3 execution, proof of LWW loss, or authenticated actor identity.
"""

import argparse
from collections import Counter
from datetime import datetime
import json
from pathlib import Path
import sqlite3
import time


class Unknown(Exception):
    """No complete result can be established within the declared bounds."""


# Both indexes are shipped by Quipu. Refuse rather than silently full-scan an
# old schema. Match an actual closure transaction, not merely nearby writes.
PAIR_TEMPLATE = """
SELECT old.g, old.e, old.a, old.tx, new.tx,
       prior.timestamp, current.timestamp, prior.actor, current.actor,
       old.valid_from, old.valid_to, new.valid_from, new.valid_to{auth_columns}
FROM facts AS old INDEXED BY idx_retracted_tx
JOIN facts AS new INDEXED BY idx_tx ON new.tx=old.retracted_tx
  AND new.g=old.g AND new.e=old.e AND new.a=old.a
JOIN transactions AS prior ON prior.id=old.tx
JOIN transactions AS current ON current.id=new.tx
{auth_joins}
WHERE old.retracted_tx>? AND old.retracted_tx<=?
  AND old.tx<new.tx AND old.op=1 AND new.op=1 AND old.v!=new.v
  AND old.valid_to IS NOT NULL
LIMIT ?
"""
PAIRS = PAIR_TEMPLATE.format(auth_columns=", NULL, NULL, NULL, NULL", auth_joins="")


def pair_query(connection):
    """Optional local evidence; never infer an authenticated principal from actor."""
    columns = connection.execute('PRAGMA main.table_info("transaction_auth")').fetchall()
    if not columns:
        return PAIRS, "not_recorded"
    names = {column[1] for column in columns}
    primary = [column[1] for column in columns if column[5]]
    if not {"tx", "principal", "auth_class"} <= names or primary != ["tx"]:
        raise Unknown("local-auth-schema-unproved")
    return PAIR_TEMPLATE.format(
        auth_columns=", pa.principal, pa.auth_class, ca.principal, ca.auth_class",
        auth_joins=("LEFT JOIN transaction_auth AS pa ON pa.tx=prior.id\n"
                    "LEFT JOIN transaction_auth AS ca ON ca.tx=current.id"),
    ), "local_transaction_rows"


def principal_evidence(old_principal, old_class, new_principal, new_class):
    """Describe retained local metadata, not signature or present-day authority."""
    if not all(isinstance(v, str) and v.strip()
               for v in (old_principal, old_class, new_principal, new_class)):
        return "unknown"
    return "same_recorded_principal" if old_principal == new_principal else "different_recorded_principals"


def gap_bucket(gap):
    """Fixed aggregate buckets; no transaction timestamp is exported."""
    lower = 0
    if gap == 0:
        return "0"
    for upper in (1, 10, 60, 300, 3600):
        if gap <= upper:
            return f"({lower},{upper}]"
        lower = upper
    raise Unknown("candidate-gap-out-of-bounds")


def timestamp(value):
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("timezone required")
    return parsed.timestamp()


def census(database, after_tx, through_tx, max_pairs=5000, gap_seconds=300, seconds=5):
    """Aggregate physical claim pairs; no terms, values, entities or actors escape."""
    if (type(after_tx) is not int or type(through_tx) is not int
            or not 0 <= after_tx < through_tx
            or type(max_pairs) is not int or not 1 <= max_pairs <= 50000
            or type(gap_seconds) is not int or not 1 <= gap_seconds <= 3600
            or not 0 < seconds <= 10):
        raise Unknown("invalid-bounds")
    database = Path(database)
    if not database.is_absolute() or database.is_symlink() or not database.is_file():
        raise Unknown("retained-snapshot-required")
    deadline = time.monotonic() + seconds
    connection = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=0.05)
    connection.set_progress_handler(lambda: int(time.monotonic() >= deadline), 1000)
    try:
        connection.execute("PRAGMA query_only=ON")
        connection.execute("PRAGMA trusted_schema=OFF")
        connection.execute("PRAGMA temp_store=MEMORY")
        connection.execute("BEGIN")
        store = connection.execute("SELECT store_id FROM store_identity WHERE id=1").fetchone()
        epoch = connection.execute("SELECT epoch FROM log_epoch WHERE id=1").fetchone()
        head = connection.execute("SELECT MAX(id) FROM transactions").fetchone()[0]
        if not store or not epoch or head is None or through_tx > head:
            raise Unknown("snapshot-identity-or-head-unproved")
        query, auth_coverage = pair_query(connection)
        rows = connection.execute(query, (after_tx, through_tx, max_pairs + 1)).fetchall()
        if len(rows) > max_pairs:
            raise Unknown("pair-budget-exhausted")
        counts = Counter()
        groups = Counter()
        identity_counts = Counter()
        group_identity = Counter()
        gaps = Counter()
        slots = set()
        for (graph, entity, predicate, old_tx, new_tx, before, after, old_actor, new_actor,
             old_from, old_to, new_from, new_to,
             old_principal, old_class, new_principal, new_class) in rows:
            if time.monotonic() >= deadline:
                raise Unknown("analysis-deadline-exhausted")
            counts["physical_claim_pairs"] += 1
            try:
                nonempty = (timestamp(old_from) < timestamp(old_to)
                            and (new_to is None or timestamp(new_from) < timestamp(new_to)))
            except (ValueError, TypeError, AttributeError, OverflowError):
                counts["interval_unknown"] += 1
                continue
            if not nonempty:
                counts["empty_or_reversed_interval"] += 1
                continue
            if not old_actor or not new_actor:
                counts["actor_unknown"] += 1
                continue
            if old_actor == new_actor:
                counts["same_declared_actor"] += 1
                continue
            try:
                gap = timestamp(after) - timestamp(before)
            except (ValueError, TypeError, AttributeError, OverflowError):
                counts["time_unknown"] += 1
                continue
            if gap < 0:
                counts["time_unknown"] += 1
            elif gap > gap_seconds:
                counts["outside_time_window"] += 1
            else:
                counts["different_declared_actor_candidates"] += 1
                groups[(graph, predicate)] += 1
                slots.add((graph, entity, predicate, new_tx))
                evidence = principal_evidence(old_principal, old_class, new_principal, new_class)
                identity_counts[evidence] += 1
                group_identity[(graph, predicate, evidence)] += 1
                gaps[gap_bucket(gap)] += 1
        # Deduplicate physical claim cross-products only for this sizing view.
        # It remains a physical slot/transaction cohort, not logical LWW proof.
        slot_transactions = Counter((graph, entity, predicate)
                                    for graph, entity, predicate, _tx in slots)
        hot_slots = Counter(slot_transactions.values())
        group_slots = Counter((graph, predicate) for graph, _entity, predicate, _tx in slots)
        return {
            "schema": "quipu-retrospective-claim-census/v1",
            "status": "observed_bounded_claim_pairs",
            "store_id": store[0], "log_epoch": epoch[0], "snapshot_head": head,
            "after_tx_exclusive": after_tx, "through_tx_inclusive": through_tx,
            "gap_seconds": gap_seconds, "max_pairs": max_pairs,
            "counts": dict(sorted(counts.items())),
            "distinct_candidate_slot_transactions": len(slots),
            "candidate_identity_evidence": dict(sorted(identity_counts.items())),
            "candidate_gap_histogram_seconds": dict(sorted(gaps.items())),
            "distinct_candidate_physical_slots": len(slot_transactions),
            "physical_hot_slot_histogram": [
                {"candidate_replacement_transactions": frequency, "slots": count}
                for frequency, count in sorted(hot_slots.items())],
            "groups": [{"graph_id": graph, "predicate_id": predicate,
                        "physical_claim_pair_candidates": count,
                        "distinct_candidate_slot_transactions": group_slots[(graph, predicate)],
                        "candidate_identity_evidence": {
                            evidence: group_identity[(graph, predicate, evidence)]
                            for evidence in sorted(identity_counts)
                            if group_identity[(graph, predicate, evidence)]}}
                       for (graph, predicate), count in sorted(groups.items())],
            "coverage": {
                "read_base": "unknown", "three_way_classification": "not_run",
                "shape_cardinality": "unknown", "ontology_mapping": "unknown",
                "actor_identity": "caller_declared_not_authenticated",
                "local_auth_evidence": auth_coverage,
                "principal_to_agent_attribution": "not_established",
                "legacy_closures_without_retracted_tx": "not_observable",
                "logical_slot_deduplication": "not_claimed",
                "lost_update_or_lww_proof": "not_established",
            },
        }
    except sqlite3.Error as error:
        raise Unknown("snapshot-schema-or-read-budget-unknown") from error
    finally:
        connection.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--snapshot", type=Path, required=True)
    parser.add_argument("--after-tx", type=int, required=True)
    parser.add_argument("--through-tx", type=int, required=True)
    parser.add_argument("--max-pairs", type=int, default=5000)
    parser.add_argument("--gap-seconds", type=int, default=300)
    args = parser.parse_args()
    try:
        result = census(args.snapshot, args.after_tx, args.through_tx,
                        args.max_pairs, args.gap_seconds)
    except (Unknown, OSError, sqlite3.Error):
        # No private paths, SQLite diagnostics, actor names or partial counts.
        print(json.dumps({"schema": "quipu-retrospective-claim-census/v1", "status": "unknown"}))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
