#!/usr/bin/env python3
"""Reject scheduling escapes and cancellation of pending CI evidence."""
import ast
import copy
import importlib.util
import itertools
import json
from pathlib import Path
import re
import unittest
import tempfile

spec = importlib.util.spec_from_file_location("budget", Path(__file__).with_name("check-pr-runner-budget.py"))
budget = importlib.util.module_from_spec(spec)
spec.loader.exec_module(budget)


def condition_value(expression, event, draft, cancelled, success):
    """Evaluate the actual aggregate predicate, with a closed boolean AST."""
    expression = expression.strip()
    if expression.startswith("${{"):
        expression = expression[3:-2].strip()
    expression = re.sub(r"contains\(fromJSON\('([^']+)'\), github.event_name\)",
                        lambda match: repr(event in json.loads(match[1])), expression)
    for source, value in [("github.event.pull_request.draft", draft),
                          ("github.event_name", event), ("always()", True),
                          ("cancelled()", cancelled), ("success()", success)]:
        expression = expression.replace(source, repr(value))
    expression = re.sub(r"!(?!=)", " not ", expression)
    expression = expression.replace("&&", " and ").replace("||", " or ")

    def visit(node):
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.BoolOp):
            return (all if isinstance(node.op, ast.And) else any)(visit(x) for x in node.values)
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
            return not visit(node.operand)
        if isinstance(node, ast.Compare) and len(node.ops) == 1:
            if isinstance(node.ops[0], ast.Eq):
                return visit(node.left) == visit(node.comparators[0])
            if isinstance(node.ops[0], ast.NotEq):
                return visit(node.left) != visit(node.comparators[0])
        raise ValueError("unsupported aggregate predicate node")

    return bool(visit(ast.parse(expression.strip(), mode="eval").body))


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
            if "pull_request" not in workflow.get("on", {}) and name != "conformance.yml":
                continue
            for job, config in workflow["jobs"].items():
                if "runs-on" not in config:
                    continue
                doc = copy.deepcopy(self.actual)
                doc[name]["jobs"][job].pop("concurrency", None)
                with self.subTest(workflow=name, job=job):
                    self.assertTrue(budget.audit(doc)[0])
                tested += 1
        self.assertGreater(tested, 30)

    def test_reusable_conformance_cannot_escape_or_lose_required_dependency(self):
        self.assertEqual(self.actual["conformance.yml"]["jobs"]["re-run-conformance"]["needs"], "expected-controls")
        self.assertIn("conformance", self.actual["ci.yml"]["jobs"]["release-correctness"]["needs"])
        for job in ["expected-controls", "re-run-conformance"]:
            doc = copy.deepcopy(self.actual)
            doc["conformance.yml"]["jobs"][job].pop("concurrency")
            self.assertTrue(budget.audit(doc)[0])
        doc = copy.deepcopy(self.actual)
        doc["ci.yml"]["jobs"]["conformance"]["uses"] = "./.github/workflows/missing.yml"
        self.assertTrue(budget.audit(doc)[0])

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

    def test_always_jobs_cannot_survive_pr_supersession(self):
        tested = 0
        for name, workflow in self.actual.items():
            if "pull_request" not in workflow.get("on", {}) and name != "conformance.yml":
                continue
            for job, config in workflow["jobs"].items():
                if "always()" not in config.get("if", ""):
                    continue
                doc = copy.deepcopy(self.actual)
                changed = doc[name]["jobs"][job]
                changed["if"] = changed["if"].replace(budget.PR_CANCEL_GUARD, "true")
                with self.subTest(workflow=name, job=job):
                    self.assertTrue(budget.audit(doc)[0])
                tested += 1
        self.assertEqual(tested, 3)

    def test_aggregate_truth_table_retains_failure_and_main_verdicts(self):
        for job in ["release-correctness", "extended-correctness"]:
            expression = self.actual["ci.yml"]["jobs"][job]["if"]
            self.assertIn("always()", expression)  # No implicit success-only filter.
            for event, draft, cancelled, success in itertools.product(
                    ["pull_request", "push", "merge_group", "schedule", "workflow_dispatch", "release"],
                    [False, True], [False, True], [False, True]):
                expected = not (event == "pull_request" and (draft or cancelled))
                if job == "extended-correctness" and event in {"push", "merge_group"}:
                    expected = False
                with self.subTest(job=job, event=event, draft=draft,
                                  cancelled=cancelled, upstream_success=success):
                    self.assertEqual(condition_value(expression, event, draft, cancelled, success), expected)

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
