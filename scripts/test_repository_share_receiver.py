"""Native query output must not turn incomplete answers into empty success."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "receiver", Path(__file__).with_name("verify-repository-share-receiver.py"),
)
RECEIVER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RECEIVER)


class NativeReadTest(unittest.TestCase):
    def read(self, output):
        result = subprocess.CompletedProcess([], 0, output, "")
        with patch.object(RECEIVER, "run", return_value=result):
            return RECEIVER.iri_rows("fixture", "scratch.db", "SELECT ?iri WHERE {}")

    def test_complete_positive_and_empty_tables(self):
        self.assertEqual(self.read('iri\n----\n"urn:one"\n\n1 results\n'), ["urn:one"])
        self.assertEqual(self.read("iri\n----\n\n0 results\n"), [])

    def test_truncation_wrong_protocol_and_count_skew_refuse(self):
        for output in ('iri\n----\n"urn:one"', '{"rows":[]}',
                       'iri\n----\n"urn:one"\n\n0 results\n',
                       'iri\n----\n\n1 results\n'):
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                self.read(output)


class InternalNameTest(unittest.TestCase):
    A = "http://aegis.gastown.local/ontology/"
    W = "https://w3id.org/intigraph/quipu/id/"
    SAME = "<http://www.w3.org/2002/07/owl#sameAs>"
    TYPE = "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>"

    def check(self, *lines):
        with tempfile.TemporaryDirectory() as work:
            export = Path(work) / "export.nt"
            export.write_text("\n".join(lines) + "\n")
            RECEIVER.internal_names(export)

    def positive(self):
        return [
            f"<{self.W}doc/README.md> {self.TYPE} <{self.A}Document> .",
            f"<{self.W}doc/README.md> {self.SAME} <{self.A}doc/quipu/README.md> .",
            f"<{self.W}chunk/README.md#C1> <{self.A}inDocument> <{self.W}doc/README.md> .",
            f'<{self.W}chunk/README.md#C1> <{self.A}mentions> "{self.A}doc/quipu/x" .',
        ]

    def test_renamed_and_linked_share_passes(self):
        self.check(*self.positive())

    def test_internal_names_and_missing_links_refuse(self):
        for bad in (
            [f"<{self.A}doc/quipu/README.md> {self.TYPE} <{self.A}Document> ."],
            [f"<{self.A}repo_quipu> {self.TYPE} <{self.A}GitRepo> ."],
            [f"<{self.W}chunk/x> <{self.A}inDocument> <{self.A}doc/quipu/README.md> ."],
            [f"<{self.W}doc/x> <{self.A}touchesRepo> <{self.A}repo_quipu> ."],
            [f"<{self.W}doc/y> {self.SAME} <{self.A}doc/quipu/z> ."],
            [f"<{self.W}doc/y> {self.SAME} <https://example.org/other> ."],
        ):
            with self.subTest(bad=bad), self.assertRaises(RuntimeError):
                self.check(*self.positive(), *bad)
        unlinked = [line for line in self.positive() if self.SAME not in line]
        with self.assertRaises(RuntimeError):
            self.check(*unlinked)
        with self.assertRaises(RuntimeError):
            self.check()


if __name__ == "__main__":
    unittest.main()
