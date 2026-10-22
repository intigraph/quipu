#!/usr/bin/env python3
"""Completion-order, provenance, race and retry-budget controls; no network."""
import copy
import unittest
from unittest.mock import patch

import rearm_release_correctness as retry
from test_wait_release_correctness import check

SHA = "a" * 40
REPO = "example/project"


def run(workflow, ident):
    return dict(id=ident, event="push", head_branch="main", head_sha=SHA,
                head_repository={"full_name": REPO}, path=f".github/workflows/{workflow}.yml",
                status="completed", conclusion="success", run_attempt=1)


class RearmTests(unittest.TestCase):
    def setUp(self):
        self.ci = run("ci", 10)
        self.release = run("release", 20)
        self.fresh = None
        self.head = SHA
        self.checks = [check(1)]
        self.jobs = [dict(name=retry.DEFERRED_JOB, conclusion="success")]
        self.posts = []
        self.incomplete = False
        self.ref_reads = 0
        self.advance = False

    def api(self, path, *, method="GET"):
        if method == "POST":
            self.posts.append(path)
            return {}
        if path.endswith("/git/ref/heads/main"):
            self.ref_reads += 1
            return {"object": {"sha": "b" * 40 if self.advance and self.ref_reads > 1 else self.head}}
        if "/workflows/" in path:
            r = self.ci if "/ci.yml/" in path else self.release
            return dict(total_count=2 if self.incomplete else 1, workflow_runs=[copy.deepcopy(r)])
        if "/attempts/1/jobs" in path:
            return dict(total_count=len(self.jobs), jobs=self.jobs)
        if path.endswith("/actions/runs/20"):
            # Detail responses can contain extra API fields: compare admission,
            # not unrelated payload serialization.
            return dict(self.fresh or self.release, referenced_workflows=[])
        raise AssertionError(path)

    def invoke(self):
        with patch.object(retry, "api", side_effect=self.api), \
                patch.object(retry, "fetch", return_value=self.checks):
            return retry.rearm(REPO, SHA)

    def test_terminal_ci_rearms_only_deferred_release(self):
        self.assertIn("rearmed", self.invoke())
        self.assertEqual(self.posts, [f"repos/{REPO}/actions/runs/20/rerun"])

    def test_failure_rearms_to_real_waiter_failure_not_success(self):
        self.ci["conclusion"] = "failure"
        self.checks = [check(1, conclusion="failure")]
        self.assertIn("rearmed", self.invoke())

    def test_both_completion_orders_wait_for_other_workflow(self):
        for workflow in (self.ci, self.release):
            workflow["status"] = "in_progress"
            self.invoke()
            self.assertEqual(self.posts, [])
            workflow["status"] = "completed"

    def test_pending_correctness_never_rearms(self):
        for checks in ([], [check(2, "queued", None)], [check(2, "in_progress", None)]):
            self.checks = checks
            self.invoke()
            self.assertEqual(self.posts, [])

    def test_not_deferred_or_already_retried_never_rearms(self):
        for mutate in (lambda: self.jobs.clear(),
                       lambda: self.release.update(run_attempt=2),
                       lambda: self.release.update(conclusion="failure")):
            self.setUp()
            mutate()
            self.invoke()
            self.assertEqual(self.posts, [])

    def test_superseded_sha_never_rearms(self):
        self.head = "b" * 40
        self.invoke()
        self.assertEqual(self.posts, [])

    def test_foreign_or_nonpush_runs_never_rearm(self):
        for workflow in ("ci", "release"):
            for key, value in (("event", "pull_request"), ("head_branch", "feature"),
                               ("head_sha", "b" * 40), ("path", ".github/workflows/other.yml"),
                               ("head_repository", {"full_name": "foreign/project"})):
                self.setUp()
                getattr(self, workflow)[key] = value
                self.invoke()
                self.assertEqual(self.posts, [], (workflow, key))

    def test_admission_race_never_posts(self):
        for changes in (dict(status="queued"), dict(run_attempt=2), dict(head_sha="b" * 40)):
            self.fresh = dict(self.release, **changes)
            self.invoke()
            self.assertEqual(self.posts, [])

    def test_main_advance_during_admission_never_posts(self):
        self.advance = True
        self.invoke()
        self.assertEqual(self.posts, [])

    def test_truncated_inventory_is_unknown_without_post(self):
        self.incomplete = True
        with self.assertRaisesRegex(RuntimeError, "incomplete"):
            self.invoke()
        self.assertEqual(self.posts, [])


if __name__ == "__main__":
    unittest.main()
