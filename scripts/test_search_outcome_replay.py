#!/usr/bin/env python3
"""Exercise restart, replay and corrupt-source controls without live services."""

import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("replay", Path(__file__).with_name("search-outcome-replay.py"))
REPLAY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REPLAY)


def event(identity, outcome="empty"):
    return (json.dumps({"schema": "quipu-search-outcome/v1", "logical_call_id": f"{identity:032x}",
                       "surface": "cli_local", "operation": "search", "summary": {"outcome": outcome}}) + "\n").encode()


class ReplayTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.journal = self.root / "journal"
        self.journal.mkdir(mode=0o700)
        self.state = self.root / "state.sqlite"
        self.db = REPLAY.connect(self.state)
        self.source = self.journal / f"search-{1:032x}.jsonl"

    def tearDown(self):
        self.db.close()
        self.temp.cleanup()

    def write(self, data, path=None):
        path = path or self.source
        path.write_bytes(data)
        path.chmod(0o600)

    def count(self):
        return self.db.execute("SELECT count(*) FROM events").fetchone()[0]

    def test_restart_budget_and_duplicate_records_are_idempotent(self):
        self.write(event(1) + event(2) + event(1))
        first = REPLAY.replay(self.journal, self.db, max_records=1)
        self.assertGreater(first["backlog_bytes"], 0)
        self.assertEqual(self.count(), 1)
        self.db.close()
        self.db = REPLAY.connect(self.state)
        REPLAY.replay(self.journal, self.db)
        self.assertEqual(self.count(), 2)
        health = REPLAY.replay(self.journal, self.db)
        self.assertEqual(health["records_processed"], 0)
        output = REPLAY.metrics(self.db, health)
        self.assertIn('outcome="empty"} 2', output)
        self.assertNotIn("quipu_search_requests_total", output)

    def test_partial_tail_waits_without_advancing_or_fabricating_zero(self):
        second = event(2, "error")
        self.write(event(1) + second[:-4])
        health = REPLAY.replay(self.journal, self.db)
        self.assertEqual(health["partial_tails"], 1)
        self.assertEqual(self.count(), 1)
        with self.source.open("ab") as stream:
            stream.write(second[-4:])
        REPLAY.replay(self.journal, self.db)
        self.assertEqual(self.count(), 2)
        self.assertIn('outcome="error"} 1', REPLAY.metrics(self.db, health))

    def test_conflicting_identity_blocks_cursor_and_preserves_source(self):
        self.write(event(1) + event(1, "hits") + event(2))
        health = REPLAY.replay(self.journal, self.db)
        self.assertEqual(health["errors"], 1)
        self.assertEqual(self.count(), 1)
        self.assertEqual(self.db.execute("SELECT offset FROM files").fetchone()[0], len(event(1)))
        self.assertEqual(self.source.read_bytes(), event(1) + event(1, "hits") + event(2))

    def test_schema_unknown_truncation_and_missing_source_are_visible(self):
        self.write(event(1))
        REPLAY.replay(self.journal, self.db)
        self.write(b"")
        self.assertEqual(REPLAY.replay(self.journal, self.db)["source_gaps"], 1)
        self.source.unlink()
        self.assertEqual(REPLAY.replay(self.journal, self.db)["source_gaps"], 1)
        self.write(b'{"schema":"future"}\n', self.journal / f"search-{2:032x}.jsonl")
        self.assertGreater(REPLAY.replay(self.journal, self.db)["errors"], 0)
        self.assertEqual(self.count(), 1)

    def test_file_budget_round_robin_does_not_starve_later_files(self):
        for identity in range(1, 5):
            self.write(event(identity), self.journal / f"search-{identity:032x}.jsonl")
        for _ in range(4):
            REPLAY.replay(self.journal, self.db, max_files=1)
        self.assertEqual(self.count(), 4)

    def test_metrics_read_materialized_counts_and_unknown_state_is_refused(self):
        self.write(event(1) + event(1))
        health = REPLAY.replay(self.journal, self.db)
        self.assertEqual(self.db.execute("SELECT value FROM counts").fetchone()[0], 1)
        self.assertIn('outcome="empty"} 1', REPLAY.metrics(self.db, health))
        self.db.execute("PRAGMA user_version=99")
        self.db.close()
        with self.assertRaises(ValueError):
            REPLAY.connect(self.state)
        self.db = sqlite3.connect(self.state)

    def test_zero_kinds_require_observed_counts_and_reasons_are_bounded(self):
        summaries = [
            {"outcome":"empty", "zero_kind":"no_candidates", "candidate_count":0},
            {"outcome":"no_context", "zero_kind":"all_rejected", "candidate_count":2, "admitted_count":0},
            {"outcome":"empty", "zero_kind":"empty_rows", "result_count":0},
            {"outcome":"error", "zero_kind":"empty_rows", "result_count":0},
            {"outcome":"unknown", "zero_kind":"no_candidates", "candidate_count":0},
            {"outcome":"empty", "zero_kind":"empty_rows"},
            {"outcome":"empty", "zero_kind":"no_candidates", "candidate_count":False},
        ]
        records = []
        for identity, summary in enumerate(summaries, 1):
            record = json.loads(event(identity))
            record["summary"] = summary
            record["details"] = {"context_reason":"private-unbounded-reason-sentinel"}
            records.append(json.dumps(record) + "\n")
        self.write("".join(records).encode())
        health = REPLAY.replay(self.journal, self.db)
        self.assertEqual(self.db.execute("SELECT sum(value) FROM zeros").fetchone()[0], 3)
        output = REPLAY.metrics(self.db, health)
        for kind in ["no_candidates", "all_rejected", "empty_rows"]:
            self.assertIn(f'kind="{kind}"}} 1', output)
        self.assertIn('reason="other"', output)
        self.assertNotIn("private-unbounded-reason-sentinel", output)
        REPLAY.replay(self.journal, self.db)
        self.assertEqual(self.db.execute("SELECT sum(value) FROM zeros").fetchone()[0], 3)

    def test_insecure_source_and_cross_directory_state_are_refused(self):
        self.write(event(1))
        self.source.chmod(0o644)
        with self.assertRaises(ValueError):
            REPLAY.replay(self.journal, self.db)
        other = self.root / "other"
        other.mkdir(mode=0o700)
        with self.assertRaises(ValueError):
            REPLAY.replay(other, self.db)
        self.source.unlink()
        self.source.symlink_to(self.state)
        with self.assertRaises(OSError):
            REPLAY.replay(self.journal, self.db)

    def test_cli_preserves_health_output_and_nonzero_exit_on_corruption(self):
        self.write(event(1))
        command = [sys.executable, "-B", str(Path(__file__).with_name("search-outcome-replay.py")),
                   "--journal-dir", str(self.journal), "--state", str(self.root / "cli.sqlite")]
        first = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertIn('outcome="empty"} 1', first.stdout)
        with self.source.open("ab") as stream:
            stream.write(b'{"schema":"future"}\n')
        failed = subprocess.run(command, capture_output=True, text=True, check=False)
        self.assertEqual(failed.returncode, 2)
        self.assertIn("quipu_search_journal_errors 1", failed.stdout)
        self.assertIn('outcome="empty"} 1', failed.stdout)
        self.assertNotIn(str(self.root), failed.stdout + failed.stderr)


if __name__ == "__main__":
    os.umask(0o077)
    unittest.main()
