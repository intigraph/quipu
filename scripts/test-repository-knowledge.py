#!/usr/bin/env python3
"""Curated-source refusal controls. Lookup fixtures are not index coverage proof.

# arming: ci; release contract invokes this script.
"""

import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "knowledge", ROOT / "scripts/build-repository-knowledge.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


builder_spec = importlib.util.spec_from_file_location(
    "share_builder", ROOT / "scripts/build-repository-share.py"
)
builder = importlib.util.module_from_spec(builder_spec)
builder_spec.loader.exec_module(builder)


class ReleaseSnapshot(unittest.TestCase):
    def test_real_git_archive_excludes_untracked_key_and_preserves_source(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            source = root / "source"
            source.mkdir()
            subprocess.run(["git", "init", "-q", str(source)], check=True)
            (source / "README.md").write_text("tracked pointer source\n")
            subprocess.run(["git", "-C", str(source), "add", "README.md"], check=True)
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(source),
                    "-c",
                    "user.name=fixture",
                    "-c",
                    "user.email=fixture@example.org",
                    "commit",
                    "-qm",
                    "fixture",
                ],
                check=True,
            )
            (source / "private.key").write_text("synthetic untracked sentinel")
            revision = subprocess.check_output(
                ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
            ).strip()
            copied = builder.materialize_release(source, revision, root / "copied")
            self.assertEqual(
                (copied / "README.md").read_bytes(), (source / "README.md").read_bytes()
            )
            self.assertFalse((copied / "private.key").exists())
            self.assertEqual(
                (source / "private.key").read_text(), "synthetic untracked sentinel"
            )
            self.assertFalse((source / ".bobbin").exists())


class AgentReferences(unittest.TestCase):
    def test_current_tracked_sources_and_each_missing_or_empty_member(self):
        paths = subprocess.check_output(
            ["git", "-C", str(ROOT), "ls-files", "-z"], text=True
        ).split("\0")
        required = builder.agent_references(ROOT, paths)
        self.assertEqual(len(required), 4)
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for path in required:
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((ROOT / path).read_bytes())
            for path in required:
                with self.subTest(path=path):
                    with self.assertRaisesRegex(ValueError, "agent reference missing"):
                        builder.agent_references(root, [p for p in paths if p != path])
                    target = root / path
                    original = target.read_bytes()
                    target.write_bytes(b"")
                    with self.assertRaisesRegex(ValueError, "agent reference missing"):
                        builder.agent_references(root, paths)
                    target.unlink()
                    with self.assertRaisesRegex(ValueError, "agent reference missing"):
                        builder.agent_references(root, paths)
                    target.write_bytes(original)

    def test_legacy_shared_entrypoint_checks_actual_tracked_sources(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/check-repository-agent-references.py"), str(ROOT)],
            capture_output=True, text=True, check=True,
        )
        self.assertEqual(len(json.loads(result.stdout)["agent_references"]), 4)
        self.assertFalse(json.loads(result.stdout)["receiver_checked"])

    @unittest.skipUnless(os.environ.get("QUIPU_TEST_BIN"), "requires native test artifact")
    def test_native_pointer_lookup_refuses_each_missing_reference(self):
        from urllib.parse import quote

        binary = Path(os.environ["QUIPU_TEST_BIN"])
        paths = subprocess.check_output(
            ["git", "-C", str(ROOT), "ls-files", "-z"], text=True
        ).split("\0")
        required = builder.agent_references(ROOT, paths)
        with tempfile.TemporaryDirectory() as folder:
            private = Path(folder)
            source = private / "references.ttl"
            triples = [
                '<http://aegis.gastown.local/ontology/doc/quipu/'
                + quote(path, safe="")
                + '> <http://www.w3.org/2000/01/rdf-schema#label> '
                + json.dumps(path) + ' ; <http://aegis.gastown.local/ontology/filePath> '
                + json.dumps(path) + ' ; <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> '
                + '<http://aegis.gastown.local/ontology/Document> .\n'
                for path in required
            ]
            triples.append(
                '<http://aegis.gastown.local/ontology/doc/quipu/README.md> '
                '<http://www.w3.org/2000/01/rdf-schema#label> "README" ; '
                '<http://aegis.gastown.local/ontology/filePath> "README.md" ; '
                '<http://www.w3.org/1999/02/22-rdf-syntax-ns#type> '
                '<http://aegis.gastown.local/ontology/Document> .\n'
            )
            for missing in [None, *range(len(required))]:
                candidate = private / (str(missing) + ".db")
                source.write_text("".join(t for i, t in enumerate(triples) if i != missing))
                builder.command([binary, "knot", source, "--db", candidate], cwd=private)
                if missing is None:
                    builder.verify_agent_references(binary, candidate, required)
                    result = builder.command(
                        [sys.executable, ROOT / "scripts/check-repository-agent-references.py",
                         ROOT, "--receiver", binary, candidate], cwd=private,
                    )
                    self.assertTrue(json.loads(result)["receiver_checked"])
                else:
                    with self.assertRaisesRegex(ValueError, "missing after import"):
                        builder.verify_agent_references(binary, candidate, required)
            # A label alone must not hide a lost or misdirected filePath.
            wrong = "".join(triples).replace(
                '<http://aegis.gastown.local/ontology/filePath> ' + json.dumps(required[0]),
                '<http://aegis.gastown.local/ontology/filePath> "wrong.md"',
            )
            source.write_text(wrong)
            candidate = private / "wrong-path.db"
            builder.command([binary, "knot", source, "--db", candidate], cwd=private)
            with self.assertRaisesRegex(ValueError, "missing after import"):
                builder.verify_agent_references(binary, candidate, required)
            source.write_text("".join(triples).replace(
                ' ; <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> '
                '<http://aegis.gastown.local/ontology/Document>', '', 1,
            ))
            candidate = private / "missing-document-type.db"
            builder.command([binary, "knot", source, "--db", candidate], cwd=private)
            with self.assertRaisesRegex(ValueError, "missing after import"):
                builder.verify_agent_references(binary, candidate, required)


class AlignmentCarrier(unittest.TestCase):
    def test_explicit_empty_positive_missing_corrupt_and_nonempty_negative(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            target = root / "docs/knowledge"
            target.mkdir(parents=True)
            for name in ("alignments.ttl", "alignment-status.json"):
                shutil.copyfile(ROOT / "docs/knowledge" / name, target / name)
            self.assertIn("@prefix sssom:", builder.alignment_carrier(root))
            data = target / "alignments.ttl"
            original = data.read_text()
            data.unlink()
            with self.assertRaises(FileNotFoundError):
                builder.alignment_carrier(root)
            data.write_text(original + "<urn:x> <urn:p> <urn:y> .\n")
            with self.assertRaisesRegex(ValueError, "corrupt"):
                builder.alignment_carrier(root)
            import hashlib

            status = json.loads((target / "alignment-status.json").read_text())
            status["carrier_sha256"] = hashlib.sha256(data.read_bytes()).hexdigest()
            (target / "alignment-status.json").write_text(json.dumps(status))
            with self.assertRaisesRegex(ValueError, "mapping data"):
                builder.alignment_carrier(root)
            data.write_text(
                "@prefix owl: <http://www.w3.org/2002/07/owl#> . <urn:x> <urn:p> <urn:y> .\n"
            )
            status["carrier_sha256"] = hashlib.sha256(data.read_bytes()).hexdigest()
            (target / "alignment-status.json").write_text(json.dumps(status))
            with self.assertRaisesRegex(ValueError, "mapping data"):
                builder.alignment_carrier(root)


    def test_native_alignment_transport_is_advertised_and_hash_bound(self):
        import hashlib

        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            private = root / "private"
            private.mkdir()
            for name, usage in (
                ("current", "[--queries <name>]... [--alignments <file>] [--turtle]"),
                ("older", "[--queries <name>]... [--turtle]"),
            ):
                engine = root / name
                # Real engines print share usage to stderr and exit nonzero.
                engine.write_text(f"#!/bin/sh\necho 'usage: quipu share {usage}' >&2\nexit 1\n")
                engine.chmod(0o755)
            self.assertTrue(builder.native_alignments(root / "current", private))
            self.assertFalse(builder.native_alignments(root / "older", private))
            reviewed = ROOT / "docs/knowledge/alignments.ttl"
            stage = root / "share"
            stage.mkdir()
            shutil.copyfile(reviewed, stage / "alignments.ttl")
            manifest = {
                "alignments_hash": "sha256:"
                + hashlib.sha256(reviewed.read_bytes()).hexdigest(),
                "files": {"alignments": "alignments.ttl"},
            }
            (stage / "manifest.json").write_text(json.dumps(manifest))
            builder.verify_carried_alignments(stage, reviewed)
            for corrupt in (
                {**manifest, "alignments_hash": manifest["alignments_hash"][7:]},
                {**manifest, "files": {}},
            ):
                (stage / "manifest.json").write_text(json.dumps(corrupt))
                with self.assertRaisesRegex(ValueError, "reviewed alignments"):
                    builder.verify_carried_alignments(stage, reviewed)
            (stage / "manifest.json").write_text(json.dumps(manifest))
            with (stage / "alignments.ttl").open("a") as copied:
                copied.write("<urn:x> <urn:p> <urn:y> .\n")
            with self.assertRaisesRegex(ValueError, "reviewed alignments"):
                builder.verify_carried_alignments(stage, reviewed)
            (stage / "alignments.ttl").unlink()
            with self.assertRaisesRegex(ValueError, "reviewed alignments"):
                builder.verify_carried_alignments(stage, reviewed)


class NativeIsolation(unittest.TestCase):
    def test_real_subprocess_cannot_see_synthetic_global_config_or_endpoints(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            host, private = root / "host", root / "private"
            private.mkdir()
            config = host / ".config/bobbin"
            config.mkdir(parents=True)
            (config / "config.toml").write_text('[server]\nurl="http://127.0.0.1:9"\n')
            self.assertTrue((config / "config.toml").exists())
            code = (
                "import os,json;from pathlib import Path;"
                'visible=(Path(os.environ["HOME"])/".config/bobbin/config.toml").exists();'
                "remote=any(k in os.environ for k in "
                '("BOBBIN_SERVER","BOBBIN_QUIPU_REMOTE","QUIPU_SERVER","QUIPU_AUTH_TOKEN","GH_TOKEN"));'
                'print(json.dumps({"global_config_visible":visible,"remote_or_upload_env_visible":remote}));'
                "raise SystemExit(3 if visible or remote else 0)"
            )
            environment = {
                "HOME": str(host),
                "XDG_CONFIG_HOME": str(host / ".config"),
                "BOBBIN_SERVER": "http://127.0.0.1:9",
                "BOBBIN_QUIPU_REMOTE": "http://127.0.0.1:9",
                "QUIPU_SERVER": "http://127.0.0.1:9",
                "QUIPU_AUTH_TOKEN": "synthetic-only",
                "GH_TOKEN": "synthetic-only",
            }
            with patch.dict(os.environ, environment):
                output = builder.command([sys.executable, "-c", code], cwd=private)
                policy_code = code.replace(
                    "3 if visible or remote else 0", "0 if visible and remote else 3"
                )
                policy = builder.command(
                    [sys.executable, "-c", policy_code],
                    env=dict(os.environ),
                    cwd=private,
                )
            self.assertIn('"global_config_visible": false', output)
            self.assertIn('"remote_or_upload_env_visible": false', output)
            self.assertIn('"global_config_visible": true', policy)
            self.assertIn('"remote_or_upload_env_visible": true', policy)


class Knowledge(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.registry = json.loads(
            (ROOT / "docs/knowledge/repository-knowledge.json").read_text()
        )
        for source in {r["source"] for r in self.registry["records"]}:
            target = self.root / source
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / source, target)
        (self.root / "docs/knowledge").mkdir(parents=True)
        self.save()

    def save(self):
        (self.root / "docs/knowledge/repository-knowledge.json").write_text(
            json.dumps(self.registry)
        )

    def render(self, lookup=lambda _: "actual-index-label-fixture"):
        return module.render(self.root, "1" * 40, lookup)

    def test_source_bound_nonempty_content(self):
        text = self.render()
        self.assertIn("2592000", text)
        self.assertIn("reviewAfter", text)
        self.assertIn('knowledgeKind> "idea"', text)
        self.assertIn('interfaceKind> "file-format"', text)
        self.assertIn('supersessionStatus> "not-recorded-in-source"', text)
        self.assertIn('startHereOrder> "3"^^', text)
        self.assertNotIn("/content>", text)
        self.assertNotIn("owl#sameAs", text)

    def test_absent_or_ambiguous_index_pointer_refuses(self):
        for value in (None, False, ""):
            with (
                self.subTest(value=value),
                self.assertRaisesRegex(ValueError, "absent or ambiguous"),
            ):
                self.render(lambda _: value)

    def test_each_agent_reference_requires_its_real_index_pointer(self):
        required = builder.agent_references(ROOT, [r["source"] for r in self.registry["records"]])
        for record in self.registry["records"]:
            if record["source"] not in required:
                continue
            subject = record["subject"]
            with self.subTest(source=record["source"]):
                with self.assertRaisesRegex(ValueError, "absent or ambiguous"):
                    self.render(lambda s: None if s == subject else "indexed-label")

    def test_changed_source_refuses(self):
        path = self.root / self.registry["records"][0]["source"]
        path.write_text(path.read_text() + "\nchanged\n")
        with self.assertRaisesRegex(ValueError, "witness changed"):
            self.render()

    def test_guessed_identity_refuses(self):
        self.registry["records"][0]["subject"] = "https://example.org/guessed"
        self.save()
        with self.assertRaisesRegex(ValueError, "source-backed"):
            self.render()

    def test_missing_required_content_refuses(self):
        self.registry["records"] = [
            r for r in self.registry["records"] if r["kind"] != "decision"
        ]
        self.save()
        with self.assertRaisesRegex(ValueError, "inventory incomplete"):
            self.render()

    def test_kind_metadata_deletions_refuse_in_renderer(self):
        for kind, fields in (("interface", ("interfaceKind", "publicName")),
                             ("decision", ("status", "supersessionStatus"))):
            for deleted in ((fields[0],), (fields[1],), fields):
                with self.subTest(kind=kind, deleted=deleted):
                    self.registry = json.loads((ROOT / "docs/knowledge/repository-knowledge.json").read_text())
                    for record in self.registry["records"]:
                        if record["kind"] == kind:
                            for key in deleted:
                                record["properties"].pop(key)
                    self.save()
                    with self.assertRaisesRegex(ValueError, "required metadata"):
                        self.render()

    @unittest.skipUnless(os.environ.get("QUIPU_TEST_BIN"), "native schema control requires explicit test artifact")
    def test_native_schema_kind_metadata_deletions_refuse(self):
        import rdflib
        a = rdflib.Namespace(module.A)
        valid = self.render()
        data = self.root / "kind-metadata.ttl"
        args = [os.environ["QUIPU_TEST_BIN"], "validate", "--shapes",
                str(ROOT / "docs/knowledge/repository-knowledge.shapes.ttl"),
                "--data", str(data), "--format", "json"]
        data.write_text(valid)
        positive = subprocess.run(args, capture_output=True, text=True, cwd=self.root)
        self.assertEqual(positive.returncode, 0, positive.stderr + positive.stdout)
        for kind, fields in (("interface", ("interfaceKind", "publicName")),
                             ("decision", ("status", "supersessionStatus"))):
            for deleted in ((fields[0],), (fields[1],), fields):
                with self.subTest(kind=kind, deleted=deleted):
                    graph = rdflib.Graph().parse(data=valid, format="turtle")
                    subjects = set(graph.subjects(a.knowledgeKind, rdflib.Literal(kind)))
                    self.assertTrue(subjects)
                    for subject in subjects:
                        for key in deleted:
                            graph.remove((subject, a[key], None))
                    data.write_text(graph.serialize(format="turtle"))
                    result = subprocess.run(args, capture_output=True, text=True, cwd=self.root)
                    self.assertNotEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_bad_freshness_and_private_summary_refuse(self):
        self.registry["max_age_seconds"] = 0
        self.save()
        with self.assertRaisesRegex(ValueError, "freshness"):
            self.render()
        self.registry["max_age_seconds"] = 30
        self.registry["records"][0]["summary"] = "read /home/example/file"
        self.save()
        with self.assertRaisesRegex(ValueError, "private summary"):
            self.render()

    @unittest.skipUnless(
        os.environ.get("QUIPU_TEST_BIN"),
        "native schema control requires explicit test artifact",
    )
    def test_actual_native_schema_positive_and_missing_freshness_negative(self):
        data = self.root / "curated.ttl"
        data.write_text(self.render())
        args = [
            os.environ["QUIPU_TEST_BIN"],
            "validate",
            "--shapes",
            str(ROOT / "docs/knowledge/repository-knowledge.shapes.ttl"),
            "--data",
            str(data),
            "--format",
            "json",
        ]
        positive = subprocess.run(args, capture_output=True, text=True, cwd=self.root)
        self.assertEqual(positive.returncode, 0, positive.stderr + positive.stdout)
        data.write_text(
            "\n".join(
                line
                for line in data.read_text().splitlines()
                if "/reviewAfter>" not in line
            )
        )
        negative = subprocess.run(args, capture_output=True, text=True, cwd=self.root)
        self.assertNotEqual(negative.returncode, 0, negative.stdout)


if __name__ == "__main__":
    unittest.main()
