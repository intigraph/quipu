#!/usr/bin/env python3
"""Offline controls for PR wheel selection and artifact-consumer alignment."""
import importlib.util
import os
import json
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("python-wheel-platforms.py")
spec = importlib.util.spec_from_file_location("wheel_platforms", SCRIPT)
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


class PolicyTests(unittest.TestCase):
    def test_shared_core_change_retains_linux_without_macos(self):
        self.assertFalse(selector.mac_required(["src/mapping.rs", "docs/design/mapping.md"], b"+fn map() {}"))
        matrix = selector.matrices(False)
        self.assertEqual({p["name"] for p in matrix["wheels"]["include"]}, {"linux-x86_64", "linux-aarch64"})
        self.assertEqual(len(matrix["tests"]["include"]), 8)

    def test_each_platform_has_exactly_four_matching_consumers(self):
        for include_mac in [False, True]:
            matrix = selector.matrices(include_mac)
            for platform in matrix["wheels"]["include"]:
                tests = [t for t in matrix["tests"]["include"] if t["name"] == platform["name"]]
                self.assertEqual({t["python"] for t in tests}, {"3.11", "3.12", "3.13", "3.14"})
                self.assertEqual({t["runner"] for t in tests}, {platform["runner"]})
            self.assertEqual({t["name"] for t in matrix["tests"]["include"]},
                             {p["name"] for p in matrix["wheels"]["include"]})

    def test_packaging_and_known_platform_paths_require_macos(self):
        for path in ["Cargo.toml", "Cargo.lock", "build.rs", "bindings/python/src/lib.rs",
                     "tests/fixtures/bindings-conformance/v1.json", "src/render/publish.rs",
                     "src/store/ownership.rs", ".cargo/config.toml", ".github/workflows/python-bindings.yml",
                     "scripts/python-wheel-platforms.py"]:
            with self.subTest(path=path):
                self.assertTrue(selector.mac_required([path], b""))

    def test_platform_code_added_or_removed_requires_macos(self):
        for marker in [b'macos', b'Darwin', b'target_os', b'std::os', b'libc', b'unix', b'Apple']:
            for sign in [b'+', b'-']:
                with self.subTest(marker=marker, sign=sign):
                    self.assertTrue(selector.mac_required(["src/new.rs"], sign + marker))

    def test_unknown_paths_require_full_coverage(self):
        self.assertTrue(selector.mac_required(["new-build-system/config"], b""))
        self.assertTrue(selector.mac_required(["src/native-library.a"], b""))

    def test_cli_untrusted_revision_is_data_and_outputs_aligned_json(self):
        with tempfile.TemporaryDirectory() as folder:
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--event", "pull_request", "--base", "a" * 40,
                 "--head", "$(touch unexpected)"], cwd=folder, capture_output=True, text=True, check=True,
            )
            self.assertFalse((Path(folder) / "unexpected").exists())
            outputs = dict(line.split("=", 1) for line in result.stdout.splitlines())
            self.assertEqual(set(outputs), {"wheels", "tests"})
            self.assertEqual(len(json.loads(outputs["wheels"])["include"]), 3)
            self.assertEqual(len(json.loads(outputs["tests"])["include"]), 12)

    def test_every_non_pr_event_retains_all_platforms_without_git(self):
        with patch.object(selector, "git_bytes", side_effect=AssertionError("must not read git")):
            for event in ["push", "schedule", "workflow_dispatch", "merge_group", "unknown"]:
                self.assertTrue(selector.select(event, "", "")[0])
                self.assertEqual(len(selector.matrices(True)["wheels"]["include"]), 3)
                self.assertEqual(len(selector.matrices(True)["tests"]["include"]), 12)

    def test_invalid_revisions_never_execute_git(self):
        with patch.object(selector, "git_bytes", side_effect=AssertionError("must not execute git")):
            for revision in ["", "main", "$(touch unexpected)", "a" * 39, "A" * 40]:
                self.assertTrue(selector.select("pull_request", revision, "b" * 40)[0])

    def test_missing_or_empty_diff_retains_macos(self):
        with patch.object(selector, "git_bytes", side_effect=OSError("unavailable")):
            self.assertTrue(selector.select("pull_request", "a" * 40, "b" * 40)[0])
        with patch.object(selector, "git_bytes", return_value=b""):
            self.assertTrue(selector.select("pull_request", "a" * 40, "b" * 40)[0])


class RealGitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.previous = Path.cwd()
        os.chdir(self.temp.name)
        self.env = dict(os.environ, GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self.git("init", "-q")
        Path("src").mkdir()
        Path("src/generic.rs").write_text("fn shared() {}\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()

    def tearDown(self):
        os.chdir(self.previous)
        self.temp.cleanup()

    def git(self, *args):
        return subprocess.check_output(["git", *args], env=self.env, text=True, stderr=subprocess.DEVNULL)

    def commit(self):
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")

    def test_real_core_diff_reduces_five_macos_jobs(self):
        Path("src/generic.rs").write_text("fn shared() { let value = 1; }\n")
        self.commit()
        mac, _ = selector.select("pull_request", self.base, self.git("rev-parse", "HEAD").strip())
        self.assertFalse(mac)
        full, reduced = selector.matrices(True), selector.matrices(mac)
        self.assertEqual(sum(len(x["include"]) for x in full.values()) -
                         sum(len(x["include"]) for x in reduced.values()), 5)

    def test_removing_last_platform_marker_still_runs_macos(self):
        Path("src/generic.rs").write_text('#[cfg(target_os = "macos")]\nfn shared() {}\n')
        self.commit()
        base = self.git("rev-parse", "HEAD").strip()
        Path("src/generic.rs").write_text("fn shared() {}\n")
        self.commit()
        self.assertTrue(selector.select("pull_request", base, self.git("rev-parse", "HEAD").strip())[0])

    def test_missing_history_is_not_evidence_to_skip_macos(self):
        self.assertTrue(selector.select("pull_request", "0" * 40, self.base)[0])


if __name__ == "__main__":
    unittest.main()
