#!/usr/bin/env python3
"""Actual staging controls: wrong release and path traversal never become docs."""
import importlib.util
import io
import json
import os
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("stage_report", Path(__file__).with_name("stage-conformance-release.py"))
stage_report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage_report)


class StageReport(unittest.TestCase):
    def test_missing_assets_hold_publication_but_transport_errors_fail(self):
        workflow = (Path(__file__).parents[1] / ".github/workflows/docs.yml").read_text()
        step = workflow.split("      - name: Stage the latest release conformance report\n", 1)[1]
        shell = step.split("        run: |\n", 1)[1].split("\n      - name:", 1)[0]
        shell = "\n".join(line[10:] for line in shell.splitlines())
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gh = root / "gh"
            gh.write_text('#!/bin/sh\nif [ "$PROBE_TRANSPORT" = fail ]; then exit 9; fi\nprintf \'%s\\n\' \'{"tagName":"v-before-reports","assets":[]}\'\n')
            gh.chmod(0o755)
            output = root / "outputs"
            env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", GITHUB_OUTPUT=str(output))
            held = subprocess.run(["bash", "-c", shell], env=env, capture_output=True, text=True)
            self.assertEqual(held.returncode, 0, held.stderr)
            self.assertEqual(output.read_text(), "ready=false\n")
            self.assertIn("Public deployment held", held.stdout)
            output.unlink()
            failed = subprocess.run(["bash", "-c", shell], env=dict(env, PROBE_TRANSPORT="fail"), capture_output=True)
            self.assertEqual(failed.returncode, 9)
            self.assertFalse(output.exists())
        self.assertIn("github.ref == 'refs/heads/main' && steps.conformance.outputs.ready == 'true'", workflow)
        self.assertIn("github.ref == 'refs/heads/main' && needs.build.outputs.release_report_ready == 'true'", workflow)

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
