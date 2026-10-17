"""Regression controls for keeping owner-curated presentation intact."""

import unittest

from row_ops import build_row_ops, resolve_ask_bindings


class CuratedRowTests(unittest.TestCase):
    def test_patch_preserves_prose_and_requires_observed_version(self):
        existing = {"title": "Concise title", "next": "One operator step",
                    "state": "Waiting for a named gate", "priority": 1, "version": 7}
        proposed = {"item": "sample-one", "priority": 0, "owner": "operator", "rank": 2,
                    "title": "Raw long tracker description", "state": "deferred", "next": ""}
        operation = build_row_ops([proposed], [], {
            "priorities": {"sample-one": {"doc_id": "one", "version": 7}},
        })[0]
        self.assertEqual(operation["op"], "update")
        self.assertEqual(operation["if_version"], existing["version"])
        updated = {**existing, **operation["data"]}
        for field in ("title", "next", "state"):
            self.assertEqual(updated[field], existing[field])
        self.assertEqual(updated["priority"], 0)
        self.assertEqual(set(operation["data"]), {"priority", "owner", "item", "rank"})

    def test_legacy_composite_uses_explicit_existing_doc_not_a_new_id(self):
        curated = {"asks": [{"item": "sample-one"}], "existing_doc_ids": ["one"]}
        bindings = resolve_ask_bindings({"asks": {
            "sample-one / sample-related": {"doc_id": "one", "version": 4},
        }}, curated)
        operation = build_row_ops([], [{"item": "sample-one", "ask": "Raw replacement"}], bindings)[0]
        self.assertEqual(operation["doc_id"], "one")
        self.assertEqual(operation["if_version"], 4)
        self.assertEqual(operation["data"], {"item": "sample-one"})

    def test_missing_version_never_becomes_an_unconditional_write(self):
        for version in (None, 0, -1, "7", True):
            with self.subTest(version=version), self.assertRaises(ValueError):
                build_row_ops([], [{"item": "sample-one"}], {
                    "asks": {"sample-one": {"doc_id": "one", "version": version}},
                })

    def test_missing_or_ambiguous_identity_refuses(self):
        curated = {"asks": [{"item": "sample-one"}], "existing_doc_ids": ["one"]}
        for bindings in ({"asks": {}}, {"asks": {
            "first": {"doc_id": "one", "version": 1},
            "second": {"doc_id": "one", "version": 2},
        }}):
            with self.subTest(bindings=bindings), self.assertRaises(ValueError):
                resolve_ask_bindings(bindings, curated)


if __name__ == "__main__":
    unittest.main()
