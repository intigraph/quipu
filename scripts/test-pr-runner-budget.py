#!/usr/bin/env python3
"""Reject scheduling escapes and cancellation of pending CI evidence."""
import copy
import importlib.util
from pathlib import Path
import unittest
import tempfile

spec = importlib.util.spec_from_file_location("budget", Path(__file__).with_name("check-pr-runner-budget.py"))
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)


class RunnerBudget(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.actual = budget.load(Path(__file__).resolve().parents[1])

    def test_actual_population_is_bounded(self):
        errors, count = budget.audit(self.actual)
        self.assertEqual(errors, [])
        self.assertGreater(count, 30)

    def test_every_real_runner_job_rejects_budget_escape(self):
        tested = 0
        for name, workflow in self.actual.items():
            if "pull_request" not in workflow.get("on", {}):
                continue
            for job in workflow["jobs"]:
                doc = copy.deepcopy(self.actual)
                doc[name]["jobs"][job].pop("concurrency", None)
                with self.subTest(workflow=name, job=job):
                    self.assertTrue(budget.audit(doc)[0])
                tested += 1
        self.assertGreater(tested, 30)

    def test_pending_and_active_jobs_must_not_be_canceled_by_budget(self):
        for mutation in ["single", "missing", "cancel"]:
            doc = copy.deepcopy(self.actual)
            policy = doc["ci.yml"]["jobs"]["build"]["concurrency"]
            if mutation == "single":
                policy["queue"] = "single"
            elif mutation == "missing":
                policy.pop("queue")
            else:
                policy["cancel-in-progress"] = "true"
            with self.subTest(mutation=mutation):
                self.assertTrue(budget.audit(doc)[0])

    def test_other_events_and_matrix_arms_must_not_share_a_slot(self):
        for mutation in ["job", "matrix", "run", "attempt", "event", "unbounded"]:
            doc = copy.deepcopy(self.actual)
            policy = doc["ci.yml"]["jobs"]["test"]["concurrency"]
            group = policy["group"]
            changes = {"job": ("quipu-test-", "quipu-build-"),
                       "matrix": ("strategy.job-index", "'same'"),
                       "run": ("github.run_id", "'same'"),
                       "attempt": ("github.run_attempt", "'same'"),
                       "event": ("github.event_name == 'pull_request'", "true"),
                       "unbounded": ("quipu-pr-runner-", "per-pr-")}
            a, b = changes[mutation]
            policy["group"] = group.replace(a, b)
            with self.subTest(mutation=mutation):
                self.assertTrue(budget.audit(doc)[0])

    def test_merge_group_and_same_pr_supersession_remain(self):
        for mutation in ["trigger", "cancel", "ref"]:
            doc = copy.deepcopy(self.actual)
            ci = doc["ci.yml"]
            if mutation == "trigger":
                ci["on"].pop("merge_group")
            elif mutation == "cancel":
                ci["concurrency"]["cancel-in-progress"] = "true"
            else:
                ci["concurrency"]["group"] = "global"
            with self.subTest(mutation=mutation):
                self.assertTrue(budget.audit(doc)[0])

    def test_yaml_extension_cannot_hide_an_unbounded_workflow(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            folder = root / ".github/workflows"
            folder.mkdir(parents=True)
            (folder / "ci.yml").write_text(budget.yaml.safe_dump(self.actual["ci.yml"]))
            (folder / "escape.yaml").write_text(
                "on: {pull_request: {}}\njobs: {unbounded: {runs-on: ubuntu-latest}}\n")
            docs = budget.load(root)
            self.assertIn("escape.yaml", docs)
            self.assertTrue(budget.audit(docs)[0])

    def test_duplicate_key_cannot_hide_policy(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            budget.yaml.load("on: {}\non: {pull_request: {}}\n", Loader=budget.UniqueLoader)


if __name__ == "__main__":
    unittest.main()
