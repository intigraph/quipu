"""Synthetic retained-history controls; never opens a live store."""
import importlib.util
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("census", Path(__file__).with_name("merge-shadow-census.py"))
CENSUS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CENSUS)


class CensusTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / "retained.sqlite"
        with closing(sqlite3.connect(self.path)) as db, db:
            db.executescript("""
CREATE TABLE transactions(id INTEGER PRIMARY KEY,timestamp TEXT,actor TEXT);
CREATE TABLE facts(e INTEGER,a INTEGER,v BLOB,g INTEGER,tx INTEGER,
 valid_from TEXT,valid_to TEXT,op INTEGER,retracted_tx INTEGER);
CREATE INDEX idx_tx ON facts(tx);
CREATE INDEX idx_retracted_tx ON facts(retracted_tx) WHERE retracted_tx IS NOT NULL;
CREATE TABLE store_identity(id INTEGER,store_id TEXT);
CREATE TABLE log_epoch(id INTEGER,epoch TEXT);
INSERT INTO store_identity VALUES(1,'fixture-store');
INSERT INTO log_epoch VALUES(1,'fixture-epoch');
INSERT INTO transactions VALUES(1,'2026-01-01T00:00:00Z','PRIVATE_ACTOR_A');
INSERT INTO transactions VALUES(2,'2026-01-01T00:01:00Z','PRIVATE_ACTOR_B');
INSERT INTO facts VALUES(1,2,X'61',0,1,'2026-01-01T00:00:00Z','2026-01-01T00:01:00Z',1,2);
INSERT INTO facts VALUES(1,2,X'62',0,2,'2026-01-01T00:01:00Z',NULL,1,NULL);
""")

    def tearDown(self):
        self.temp.cleanup()

    def alter(self, sql):
        with closing(sqlite3.connect(self.path)) as db, db:
            db.executescript(sql)

    def result(self, **kwargs):
        return CENSUS.census(self.path, 1, 2, **kwargs)

    def test_actual_closure_pair_positive_and_readonly_privacy(self):
        before = self.path.read_bytes()
        result = self.result()
        self.assertEqual(result['counts']['different_declared_actor_candidates'], 1)
        self.assertEqual(result['distinct_candidate_slot_transactions'], 1)
        self.assertEqual(before, self.path.read_bytes())
        self.assertNotIn('PRIVATE_ACTOR', json.dumps(result))
        self.assertEqual(result['coverage']['shape_cardinality'], 'unknown')
        self.assertEqual(result['coverage']['three_way_classification'], 'not_run')
        self.assertEqual(result['coverage']['local_auth_evidence'], 'not_recorded')
        self.assertEqual(result['candidate_identity_evidence'], {'unknown': 1})
        self.assertEqual(CENSUS.census(self.path, 0, 2), result | {'after_tx_exclusive': 0})

    def auth_table(self):
        self.alter('CREATE TABLE transaction_auth(tx INTEGER PRIMARY KEY,principal TEXT,auth_class TEXT)')

    def test_different_declared_actors_can_share_one_recorded_principal(self):
        self.auth_table()
        self.alter("INSERT INTO transaction_auth VALUES(1,'PRIVATE_SHARED','named_bearer');"
                   "INSERT INTO transaction_auth VALUES(2,'PRIVATE_SHARED','named_bearer')")
        before = self.path.read_bytes()
        result = self.result()
        self.assertEqual(result['counts']['different_declared_actor_candidates'], 1)
        self.assertEqual(result['candidate_identity_evidence'], {'same_recorded_principal': 1})
        self.assertEqual(result['groups'][0]['candidate_identity_evidence'],
                         {'same_recorded_principal': 1})
        self.assertEqual(result['coverage']['principal_to_agent_attribution'], 'not_established')
        self.assertNotIn('PRIVATE_SHARED', json.dumps(result))
        self.assertEqual(before, self.path.read_bytes())

    def test_local_evidence_distinct_principals_and_missing_evidence_stay_separate(self):
        self.auth_table()
        self.alter("INSERT INTO transaction_auth VALUES(1,'PRIVATE_AUTH_A','named_bearer');"
                   "INSERT INTO transaction_auth VALUES(2,'PRIVATE_AUTH_B','attested_session')")
        result = self.result()
        self.assertEqual(result['candidate_identity_evidence'], {'different_recorded_principals': 1})
        self.assertEqual(result['coverage']['local_auth_evidence'], 'local_transaction_rows')
        self.assertNotIn('PRIVATE_AUTH', json.dumps(result))
        for change in ("DELETE FROM transaction_auth WHERE tx=2",
                       "UPDATE transaction_auth SET principal=' ' WHERE tx=1",
                       "UPDATE transaction_auth SET auth_class=NULL WHERE tx=1"):
            with self.subTest(change=change):
                self.alter("DELETE FROM transaction_auth;"
                           "INSERT INTO transaction_auth VALUES(1,'PRIVATE_AUTH_A','named_bearer');"
                           "INSERT INTO transaction_auth VALUES(2,'PRIVATE_AUTH_B','attested_session')")
                self.alter(change)
                self.assertEqual(self.result()['candidate_identity_evidence'], {'unknown': 1})

    def test_present_but_ambiguous_local_auth_schema_refuses_instead_of_multiplying_pairs(self):
        self.alter('CREATE TABLE transaction_auth(tx INTEGER,principal TEXT,auth_class TEXT);'
                   "INSERT INTO transaction_auth VALUES(1,'A','named_bearer');"
                   "INSERT INTO transaction_auth VALUES(1,'B','named_bearer')")
        with self.assertRaises(CENSUS.Unknown):
            self.result()

    def test_multivalue_claim_cross_products_do_not_inflate_hot_slot_transactions(self):
        self.alter("INSERT INTO transactions VALUES(3,'2026-01-01T00:02:00Z','PRIVATE_ACTOR_C');"
                   "UPDATE facts SET valid_to='2026-01-01T00:02:00Z',retracted_tx=3 WHERE tx=2;"
                   "INSERT INTO facts SELECT e,a,X'63',g,tx,valid_from,valid_to,op,retracted_tx "
                   "FROM facts WHERE tx=2;"
                   "INSERT INTO facts VALUES(1,2,X'64',0,3,'2026-01-01T00:02:00Z',NULL,1,NULL)")
        result = CENSUS.census(self.path, 1, 3)
        self.assertEqual(result['counts']['physical_claim_pairs'], 4)
        self.assertEqual(result['distinct_candidate_slot_transactions'], 2)
        self.assertEqual(result['distinct_candidate_physical_slots'], 1)
        self.assertEqual(result['physical_hot_slot_histogram'],
                         [{'candidate_replacement_transactions': 2, 'slots': 1}])
        self.assertEqual(result['candidate_gap_histogram_seconds'], {'(10,60]': 4})
        self.assertEqual(result['groups'][0]['distinct_candidate_slot_transactions'], 2)
        self.assertNotIn('PRIVATE_ACTOR', json.dumps(result))

    def test_adjacent_different_actor_writes_without_closure_are_not_overwrites(self):
        self.alter('UPDATE facts SET retracted_tx=NULL WHERE tx=1')
        with closing(sqlite3.connect(self.path)) as db, db:
            naive = db.execute('SELECT COUNT(*) FROM facts old JOIN facts new '
                               'ON old.e=new.e AND old.a=new.a AND old.tx<new.tx '
                               'AND old.v!=new.v').fetchone()[0]
        self.assertEqual(naive, 1, 'a proximity-only detector would incorrectly count this')
        self.assertEqual(self.result()['counts'], {})

    def test_query_uses_retained_retraction_and_transaction_indexes(self):
        with closing(sqlite3.connect(self.path)) as db, db:
            plan = db.execute('EXPLAIN QUERY PLAN ' + CENSUS.PAIRS, (1, 2, 5001)).fetchall()
        rendered = repr(plan)
        self.assertIn('idx_retracted_tx', rendered)
        self.assertIn('idx_tx', rendered)

    def test_same_value_foreign_graph_and_foreign_slot_are_not_pairs(self):
        for change in ["v=X'61'", 'g=7', 'e=7', 'a=7']:
            with self.subTest(change=change):
                self.alter("UPDATE facts SET v=X'62',g=0,e=1,a=2 WHERE tx=2")
                self.alter('UPDATE facts SET ' + change + ' WHERE tx=2')
                self.assertEqual(self.result()['groups'], [])

    def test_receipt_empty_intervals_and_retract_rows_are_excluded(self):
        for change in ["valid_to=valid_from", 'op=0']:
            with self.subTest(change=change):
                self.alter('UPDATE facts SET valid_to=NULL,op=1 WHERE tx=2')
                self.alter('UPDATE facts SET ' + change + ' WHERE tx=2')
                self.assertEqual(self.result()['groups'], [])

    def test_equal_instants_with_different_offsets_are_receipts_not_live_rows(self):
        self.alter("UPDATE facts SET valid_to='2026-01-01T01:01:00+01:00' WHERE tx=2")
        result = self.result()
        self.assertEqual(result['counts']['empty_or_reversed_interval'], 1)
        self.assertEqual(result['groups'], [])

    def test_actor_unknown_same_actor_and_timestamp_unknown_do_not_invent_candidates(self):
        for sql, expected in [
            ("UPDATE transactions SET actor=NULL WHERE id=2", 'actor_unknown'),
            ("UPDATE transactions SET actor='PRIVATE_ACTOR_A' WHERE id=2", 'same_declared_actor'),
            ("UPDATE transactions SET actor='B',timestamp='not-time' WHERE id=2", 'time_unknown'),
            ("UPDATE transactions SET timestamp='2026-01-01T00:10:00Z' WHERE id=2", 'outside_time_window'),
        ]:
            self.alter(sql)
            self.assertEqual(self.result()['counts'][expected], 1)
            self.assertEqual(self.result()['groups'], [])

    def test_missing_schema_and_budget_refuse_not_partial_zero(self):
        self.alter("INSERT INTO facts SELECT e,a,X'63',g,tx,valid_from,valid_to,op,retracted_tx FROM facts WHERE tx=2")
        with self.assertRaises(CENSUS.Unknown):
            self.result(max_pairs=1)
        self.alter('DROP INDEX idx_retracted_tx')
        with self.assertRaises(CENSUS.Unknown):
            self.result()

    def test_unproved_head_identity_bad_bounds_and_missing_snapshot_refuse(self):
        for args in [(self.path, 1, 3), (self.path, 2, 2), (self.path, 1, 2, 50001),
                     (self.path.parent / 'missing', 1, 2)]:
            with self.subTest(args=args), self.assertRaises(CENSUS.Unknown):
                CENSUS.census(*args)
        self.alter('DELETE FROM log_epoch')
        with self.assertRaises(CENSUS.Unknown):
            self.result()

    def test_actual_cli_positive_and_budget_refusal_do_not_leak_or_report_partial_counts(self):
        command = [sys.executable, str(Path(__file__).with_name('merge-shadow-census.py')),
                   '--snapshot', str(self.path), '--after-tx', '1', '--through-tx', '2']
        good = subprocess.run(command, capture_output=True, text=True, timeout=10, check=True)
        self.assertEqual(json.loads(good.stdout)['counts']['different_declared_actor_candidates'], 1)
        self.alter("INSERT INTO facts SELECT e,a,X'63',g,tx,valid_from,valid_to,op,retracted_tx FROM facts WHERE tx=2")
        bad = subprocess.run(command + ['--max-pairs', '1'], capture_output=True, text=True, timeout=10)
        self.assertEqual(bad.returncode, 1)
        self.assertEqual(json.loads(bad.stdout), {'schema': 'quipu-retrospective-claim-census/v1',
                                                'status': 'unknown'})
        self.assertEqual(bad.stderr, '')
        self.assertNotIn('PRIVATE_ACTOR', good.stdout + bad.stdout)


if __name__ == '__main__':
    unittest.main()
