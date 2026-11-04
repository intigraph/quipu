#!/usr/bin/env python3
"""Offline durable-outcome accounting demonstration; no service traffic."""

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
BASE = "fd93c2b0b3c2eb77e2375935bedf7ac49c3106e0"
SCRIPT = "scripts/search-outcome-replay.py"


def main():
    print("Target: source checkout with synthetic private journals; no installed-service claim.")
    control = subprocess.run(
        ["git", "cat-file", "-e", f"{BASE}^{{commit}}"], cwd=ROOT,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
    )
    assert control.returncode == 0, "baseline commit unavailable; fetch it before comparing"
    before = subprocess.run(
        ["git", "cat-file", "-e", f"{BASE}:{SCRIPT}"], cwd=ROOT,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
    )
    assert before.returncode == 128, "baseline unexpectedly contains the reader"
    print(f"BEFORE {BASE}: durable outcome replay command is absent.")
    spec = importlib.util.spec_from_file_location("outcome_replay", ROOT / SCRIPT)
    reader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reader)
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        journal = root / "journal"
        journal.mkdir(mode=0o700)
        source = journal / f"search-{1:032x}.jsonl"
        events = [
            {"schema": "quipu-search-outcome/v1", "logical_call_id": f"{number:032x}",
             "surface": "cli_local", "operation": "query", "summary": {
                 "outcome": outcome, "zero_kind": "empty_rows" if outcome == "empty" else None,
                 "candidate_count": None, "admitted_count": None,
                 "result_count": 0 if outcome == "empty" else None}}
            for number, outcome in [(1, "empty"), (2, "error")]
        ]
        source.write_text("".join(json.dumps(event) + "\n" for event in events + events[:1]))
        source.chmod(0o600)
        state = root / "state.sqlite"
        print("INPUT: one successful empty query, one failed query, one identical duplicate.")
        with reader.connect(state) as db:
            health = reader.replay(journal, db)
            print("AFTER: " + json.dumps(health, sort_keys=True))
            print(reader.metrics(db, health).split("# TYPE quipu_search_journal_records_processed")[0], end="")
            assert db.execute("SELECT count(*) FROM events").fetchone()[0] == 2
            assert db.execute("SELECT sum(value) FROM zeros").fetchone()[0] == 1
        print("RESTART: reopen consumer state and replay the same source.")
        with reader.connect(state) as db:
            health = reader.replay(journal, db)
            assert health["records_processed"] == 0
            assert db.execute("SELECT count(*) FROM events").fetchone()[0] == 2
            assert db.execute("SELECT sum(value) FROM zeros").fetchone()[0] == 1
            print(f"records_processed={health['records_processed']}; unique_outcomes=2; errors={health['errors']}")
    print("PASS: failed query remains error; duplicate/restart do not increase unique outcomes.")


if __name__ == "__main__":
    os.umask(0o077)
    main()
