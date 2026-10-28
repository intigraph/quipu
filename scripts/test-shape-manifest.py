#!/usr/bin/env python3
"""Discriminating controls for the source/deployment classification contract."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("shape_manifest", ROOT / "shapes/verify_manifest.py")
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class Manifest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "one.ttl").write_text("# fixture\n")
        self.value = {"schema_version": 1, "files": [
            {"file": "one.ttl", "action": "load", "name": "one", "reason": "fixture"}]}

    def write(self, value=None):
        (self.root / "MANIFEST").write_text(json.dumps(self.value if value is None else value))

    def test_real_tree_and_preserved_names(self):
        value = M.plan(ROOT / "shapes")
        self.assertTrue({
            "aegis-ontology", "governance", "provenance", "code-entities",
            "contributor-knowledge", "demoted-derivation", "class-subsumption"}
            <= {r["name"] for r in value["load"]})
        self.assertTrue({"skill-catalogue.ttl", "render.ttl", "rule-editions.ttl", "rule-answers.ttl", "read-policy.ttl"}
                        <= {r["file"] for r in value["exclude"]})
        self.assertNotIn("read-policy.ttl", {r["file"] for r in value["load"]})

    def test_new_file_refuses_until_classified(self):
        self.write()
        M.plan(self.root)
        (self.root / "new.ttl").write_text("# new profile\n")
        with self.assertRaisesRegex(ValueError, "unclassified"):
            M.plan(self.root)
        self.value["files"].append({"file": "new.ttl", "action": "exclude", "reason": "not activated"})
        self.write()
        self.assertEqual(len(M.plan(self.root)["exclude"]), 1)
        self.value["files"][-1].update(action="load", name="new")
        self.write()
        self.assertEqual(len(M.plan(self.root)["load"]), 2)

    def test_malformed_stale_duplicate_and_missing_reasons_refuse(self):
        mutations = [lambda x: x.update(schema_version=True),
                     lambda x: x.update(extra=True),
                     lambda x: x["files"].append(copy.deepcopy(x["files"][0])),
                     lambda x: x["files"][0].update(file="missing.ttl"),
                     lambda x: x["files"][0].update(file="../one.ttl"),
                     lambda x: x["files"][0].update(action="skip"),
                     lambda x: x["files"][0].update(reason=" "),
                     lambda x: x["files"][0].update(name="../bad"),
                     lambda x: x["files"][0].update(source_dir="/other")]
        for mutate in mutations:
            value = copy.deepcopy(self.value)
            mutate(value)
            self.write(value)
            with self.subTest(value=value), self.assertRaises((ValueError, OSError)):
                M.plan(self.root)

    def test_duplicate_json_keys_and_symlink_refuse(self):
        (self.root / "MANIFEST").write_text('{"schema_version":1,"schema_version":1,"files":[]}')
        with self.assertRaisesRegex(ValueError, "duplicate"):
            M.plan(self.root)
        self.write()
        (self.root / "one.ttl").unlink()
        (self.root / "one.ttl").symlink_to(ROOT / "shapes/provenance.ttl")
        with self.assertRaisesRegex(ValueError, "symlink"):
            M.plan(self.root)

    def test_nested_files_are_not_an_escape_from_classification(self):
        (self.root / "profiles").mkdir()
        (self.root / "profiles/two.ttl").write_text("# nested\n")
        self.write()
        with self.assertRaisesRegex(ValueError, "unclassified"):
            M.plan(self.root)
        self.value["files"].append({"file": "profiles/two.ttl", "action": "load", "name": "two", "reason": "fixture"})
        self.write()
        self.assertEqual(M.plan(self.root)["load"][-1]["dest_file"], "two.ttl")

    def test_colliding_names_or_destinations_refuse(self):
        (self.root / "profiles").mkdir()
        (self.root / "profiles/one.ttl").write_text("# another source\n")
        (self.root / "two.ttl").write_text("# another source\n")
        for filename, name in [("two.ttl", "one"), ("profiles/one.ttl", "other")]:
            value = copy.deepcopy(self.value)
            value["files"].append({"file": filename, "action": "load", "name": name, "reason": "fixture"})
            self.write(value)
            with self.assertRaisesRegex(ValueError, "duplicate load"):
                M.plan(self.root)


if __name__ == "__main__":
    unittest.main()
