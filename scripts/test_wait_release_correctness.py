#!/usr/bin/env python3
"""Both-direction unit tests for the cross-workflow release gate."""

import unittest
import contextlib
import io
import os
from pathlib import Path
import tempfile
from unittest.mock import patch

import wait_release_correctness as waiter
from wait_release_correctness import FAILURE, PENDING, SUCCESS, classify


def check(
    ident: int,
    status: str = "completed",
    conclusion: str | None = "success",
    name: str = "Release correctness",
    app: str = "github-actions",
) -> dict:
    return {
        "id": ident,
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "app": {"slug": app},
    }


class ClassifyTests(unittest.TestCase):
    def test_absent_is_pending(self):
        self.assertEqual(classify([])[0], PENDING)

    def test_in_progress_is_pending(self):
        self.assertEqual(classify([check(1, "in_progress", None)])[0], PENDING)

    def test_queued_is_pending(self):
        self.assertEqual(classify([check(1, "queued", None)])[0], PENDING)

    def test_success_passes(self):
        self.assertEqual(classify([check(1)])[0], SUCCESS)

    def test_failure_refuses(self):
        self.assertEqual(classify([check(1, conclusion="failure")])[0], FAILURE)

    def test_spoofed_check_name_is_ignored(self):
        self.assertEqual(classify([check(1, app="external-ci")])[0], PENDING)

    def test_newest_rerun_is_authoritative(self):
        runs = [check(10, conclusion="failure"), check(11)]
        self.assertEqual(classify(runs)[0], SUCCESS)


class WaiterTests(unittest.TestCase):
    def run_waiter(self, runs=None, error=None):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "outputs"
            summary = Path(directory) / "summary"
            env = {"GITHUB_REPOSITORY": "example/project", "GITHUB_OUTPUT": str(output),
                   "GITHUB_STEP_SUMMARY": str(summary), "CI_GATE_MAX_ATTEMPTS": "1",
                   "CI_GATE_INTERVAL_SECONDS": "0"}
            log = io.StringIO()
            with patch.dict(os.environ, env), patch.object(waiter.sys, "argv", ["waiter", "a" * 40]), \
                    patch.object(waiter, "fetch", return_value=runs, side_effect=error) as fetch, \
                    contextlib.redirect_stdout(log):
                code = waiter.main()
            fetch.assert_called_once_with("example/project", "a" * 40)
            return code, output.read_text(), summary.read_text(), log.getvalue()

    def test_budget_pending_is_deferred_never_ready(self):
        for status in ("queued", "in_progress"):
            with self.subTest(status=status):
                code, output, summary, log = self.run_waiter([check(1, status, None)])
                self.assertEqual(code, 0)
                self.assertEqual(output, "ready=false\n")
                self.assertIn("DEFERRED", summary)
                self.assertIn("::warning", log)

    def test_absent_or_unreadable_never_permits_publication(self):
        for runs, error in (([], None), (None, RuntimeError("API unavailable"))):
            code, output, summary, _ = self.run_waiter(runs, error)
            self.assertEqual((code, output), (0, "ready=false\n"))
            self.assertIn("DEFERRED", summary)

    def test_completed_failure_still_fails(self):
        for conclusion in ("failure", "cancelled", "skipped", "timed_out", None):
            with self.subTest(conclusion=conclusion):
                code, output, summary, _ = self.run_waiter([check(1, conclusion=conclusion)])
                self.assertEqual((code, output), (1, "ready=false\n"))
                self.assertIn("FAILED", summary)

    def test_only_success_is_ready(self):
        code, output, summary, _ = self.run_waiter([check(1)])
        self.assertEqual((code, output), (0, "ready=true\n"))
        self.assertIn("READY", summary)


if __name__ == "__main__":
    unittest.main()
