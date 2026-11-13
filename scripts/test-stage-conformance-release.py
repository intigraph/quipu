#!/usr/bin/env python3
"""Actual staging controls: wrong release and path traversal never become docs."""
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("stage_report", Path(__file__).with_name("stage-conformance-release.py"))
stage_report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage_report)


class StageReport(unittest.TestCase):
    def test_exact_tag_and_safe_members(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "report.tar.gz"
            files = {"conformance.md": b"Measured report", "provenance.json": json.dumps({
                "schema": "quipu-release-conformance/v1", "release_tag": "v-test",
                "revision": "a" * 40, "run_url": "https://github.com/intigraph/quipu/actions/runs/123"}).encode(),
                "badges/shacl-core.json": b"{}"}
            def write(members):
                with tarfile.open(archive, "w:gz") as target:
                    for name, body in members.items():
                        entry = tarfile.TarInfo(name)
                        entry.size = len(body)
                        target.addfile(entry, io.BytesIO(body))
            write(files)
            stage_report.stage(archive, root / "good", "v-test")
            self.assertEqual((root / "good/conformance.md").read_bytes(), files["conformance.md"])
            with self.assertRaises(ValueError):
                stage_report.stage(archive, root / "wrong", "other-tag")
            self.assertFalse((root / "wrong").exists())
            write(dict(files, **{"../escape.json": b"{}"}))
            with self.assertRaises(ValueError):
                stage_report.stage(archive, root / "unsafe", "v-test")
            self.assertFalse((root / "unsafe").exists())


if __name__ == "__main__":
    unittest.main()
