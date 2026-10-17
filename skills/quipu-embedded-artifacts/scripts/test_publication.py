#!/usr/bin/env python3
"""Native privacy controls; remote inputs and unexecuted bundle are fixtures.

# arming: agent-invoked; requires matching installed quipu/quipu-server.
"""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import demo
import generate


class PublicationTests(unittest.TestCase):
    def fixture(self, private_operator=False, private_description=False,
                block_notice=False, private_title=False):
        temporary = tempfile.TemporaryDirectory(prefix="brief-publication-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        bundle = root / "bundle"
        bundle.mkdir()
        for name in ("quipu_wasm_explorer.js", "quipu_wasm_explorer_bg.wasm"):
            (bundle / name).write_bytes(b"fixture-not-executed")
        original_generate, original_reader = generate.generate, generate.Reader

        def wrapped(config, *arguments):
            if private_operator:
                config["graph"] = "https://private.example/board"
                path = Path(config["row_bindings_file"])
                bindings = json.loads(path.read_text())
                for collection in bindings.values():
                    for binding in collection.values():
                        binding["doc_id"] = "private.example"
                path.write_text(json.dumps(bindings))
                path = Path(config["asks_file"])
                curated = json.loads(path.read_text())
                curated["existing_doc_ids"] = ["private.example"]
                path.write_text(json.dumps(curated))
            reader = generate.Reader

            class ControlledReader(reader):
                def query(self, query, limit):
                    rows = super().query(query, limit)
                    if self.number == 1:
                        if private_description:
                            rows[0]["description"]["value"] = "Details at private.example"
                        if private_title:
                            rows[0]["title"]["value"] = "private.example"
                    if self.number == 4 and block_notice:
                        subject = {"type": "uri", "value": "urn:test:notice-policy"}
                        for predicate, value, kind in (
                            (generate.RDF + "type", generate.AEGIS + "InternalIdentifierPattern", "uri"),
                            (generate.AEGIS + "regex", "Description withheld: private identifiers", "literal"),
                            (generate.AEGIS + "enforcementTier", "block", "literal"),
                            (generate.RDFS + "label", "Metadata-only notice policy", "literal"),
                        ):
                            rows.append({"s": subject, "p": {"type": "uri", "value": predicate},
                                         "o": {"type": kind, "value": value}})
                    return rows

            generate.Reader = ControlledReader
            return original_generate(config, *arguments)

        argv = ["demo", "--out", str(root / "ready"), "--work", str(root / "work"),
                "--bundle", str(bundle), "--quipu", os.environ.get("QUIPU_BIN", "quipu")]
        failure = None
        with patch.object(generate, "generate", wrapped), patch.object(generate, "Reader", original_reader), \
                patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            try:
                demo.main()
            except subprocess.CalledProcessError as error:
                failure = error
        return root, failure

    def test_safe_native_share_has_positive_record_and_private_operator_files(self):
        root, failure = self.fixture()
        self.assertIsNone(failure)
        metadata = json.loads((root / "work/producer/publication-metadata.json").read_text())
        self.assertEqual(metadata["items"]["sample-release"], "https://example.org/item/sample-release")
        self.assertNotIn("graph", metadata)
        self.assertFalse((root / "ready/rows.json").exists())
        self.assertFalse((root / "ready/snapshot.json").exists())
        self.assertEqual([p.name for p in (root / "ready").glob("*.json")], ["pendant.json"])
        self.assertIn("urn:brief:publication-metadata", (root / "work/producer/pendant/export.nt").read_text())
        self.assertTrue((root / "work/producer/rows.json").is_file())
        self.assertEqual((root / "work/producer").stat().st_mode & 0o777, 0o700)

    def test_blocked_operator_identifiers_stay_out_of_every_ready_file(self):
        root, failure = self.fixture(private_operator=True)
        self.assertIsNone(failure)
        for path in (root / "ready").rglob("*"):
            if path.is_file():
                self.assertNotIn(b"private.example", path.read_bytes(), str(path))
        producer = root / "work/producer"
        self.assertIn("private.example", (producer / "provenance.json").read_text())
        operations = json.loads((producer / "rows.json").read_text())["ops"]
        self.assertTrue(operations)
        self.assertTrue(all(op["doc_id"] == "private.example" for op in operations))

    def test_optional_private_description_is_omitted_with_a_visible_notice(self):
        root, failure = self.fixture(private_description=True)
        self.assertIsNone(failure)
        metadata = json.loads((root / "work/producer/publication-metadata.json").read_text())
        self.assertTrue(metadata["omissions"])
        self.assertIn("Description withheld", metadata["omissions"][0]["reason"])
        self.assertNotIn("private.example", (root / "work/producer/pendant/export.nt").read_text())

    def test_native_guard_refuses_a_metadata_only_value(self):
        # Only the browser notice contains this phrase. Metadata must be part
        # of the RDF payload checked by the unchanged native byte-level guard.
        root, failure = self.fixture(private_description=True, block_notice=True)
        self.assertIsNotNone(failure)
        self.assertFalse((root / "ready").exists())
        self.assertIn("Metadata-only notice policy", (root / "work/producer/native.log").read_text())

    def test_required_private_identity_is_refused_without_rewriting(self):
        root, failure = self.fixture(private_title=True)
        self.assertIsNotNone(failure)
        self.assertFalse((root / "ready").exists())
        self.assertIn("Synthetic private identifiers", (root / "work/producer/native.log").read_text())
        self.assertIn("private.example", (root / "work/producer/data.nt").read_text())

    def test_private_work_and_publish_paths_cannot_overlap(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for out, work in ((root / "same", root / "same"),
                              (root / "out", root / "out/private"),
                              (root / "work/ready", root / "work")):
                with self.subTest(out=out, work=work):
                    with self.assertRaisesRegex(ValueError, "must not overlap"):
                        generate.generate({}, out, work, root / "bundle", "not-executed")
                    self.assertFalse(out.exists())
                    self.assertFalse(work.exists())


if __name__ == "__main__":
    unittest.main()
