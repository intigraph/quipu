#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = ["PyYAML==6.0.3"]
# ///
"""Offline controls: synthetic registry bytes are never registry acceptance."""

import copy
import importlib.util
import io
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

import yaml

SCRIPT = Path(__file__).with_name("signing-package.py")
spec = importlib.util.spec_from_file_location("signing_package", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Contract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.base = self.root / "crates/quipu-signing"
        self.base.mkdir(parents=True)
        (self.base / "src").mkdir()
        (self.base / "src/lib.rs").write_text("pub fn fixture() {}\n")
        (self.base / "Cargo.lock").write_text("version = 4\n")
        (self.base / "Cargo.toml").write_text(
            '[package]\nname="quipu-signing"\nversion="0.1.0"\npublish=false\n'
        )
        (self.root / "Cargo.toml").write_text(
            '[package]\nname="quipu-ai"\nversion="0.16.3"\n'
        )
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "add", "crates"], check=True)
        self.metadata = {
            "packages": [
                {
                    "name": module.NAME,
                    "version": module.VERSION,
                    "publish": [],
                    "manifest_path": str(self.base / "Cargo.toml"),
                }
            ]
        }
        self.archive = self.pack()
        self.plan = self.prepare()
        self.registry = {
            "version": {
                "crate": module.NAME,
                "num": module.VERSION,
                "yanked": False,
                "checksum": module.digest(self.archive),
            }
        }

    def pack(self, omit=None, extra=None):
        output = io.BytesIO()
        files = {
            p.relative_to(self.base).as_posix(): p.read_bytes()
            for p in self.base.rglob("*")
            if p.is_file()
        }
        files["Cargo.toml.orig"] = files["Cargo.toml"]
        if omit:
            files.pop(omit)
        files.update(extra or {})
        with tarfile.open(fileobj=output, mode="w:gz") as archive:
            for name, data in files.items():
                item = tarfile.TarInfo(f"{module.NAME}-{module.VERSION}/{name}")
                item.size = len(data)
                archive.addfile(item, io.BytesIO(data))
        return output.getvalue()

    def prepare(self, metadata=None, archive=None, version="0.1.0", ref="main"):
        return module.prepare(
            self.root,
            metadata or self.metadata,
            self.archive if archive is None else archive,
            version,
            ref,
        )

    def test_default_root_equivalent_and_shared_requires_exact_optin(self):
        self.assertEqual(module.select("quipu-ai", "Cargo.toml", ""), "root")
        self.assertEqual(
            module.select(module.NAME, module.MANIFEST, module.OPT_IN),
            "shared-preparation",
        )
        for args in [
            (module.NAME, module.MANIFEST, ""),
            ("other", "Cargo.toml", ""),
            (module.NAME, "../Cargo.toml", module.OPT_IN),
            ("quipu-ai", "Cargo.toml", module.OPT_IN),
        ]:
            with self.subTest(args=args), self.assertRaises(ValueError):
                module.select(*args)

    def test_preparation_is_inert_and_exact(self):
        self.assertFalse(self.plan["publication_authorized"])
        self.assertFalse(self.plan["distribution_verified"])
        self.assertEqual(set(self.plan["source_sha256"]), {"src/lib.rs", "Cargo.lock"})
        self.assertTrue(
            module.verify(self.plan, self.registry, self.archive)["artifact_verified"]
        )

    def test_identity_manifest_version_publish_and_ref_refusals(self):
        for field, value in [
            ("name", "other"),
            ("version", "9.0.0"),
            ("publish", None),
            ("manifest_path", str(self.root / "Cargo.toml")),
        ]:
            metadata = copy.deepcopy(self.metadata)
            metadata["packages"][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.prepare(metadata=metadata)
        for version, ref in [
            ("", "main"),
            ("0.2.0", "main"),
            ("0.1.0-rc.1", "main"),
            ("0.1.0", "test-1"),
            ("0.1.0", "rehearsal-any"),
            ("0.1.0", "v1-rehearsal"),
            ("0.1.0", ""),
        ]:
            with self.subTest(version=version, ref=ref), self.assertRaises(ValueError):
                self.prepare(version=version, ref=ref)
        path = self.base / "Cargo.toml"
        path.write_text(path.read_text().replace("publish=false", "publish=true"))
        with self.assertRaises(ValueError):
            self.prepare()

    def test_package_omission_extra_tamper_and_original_manifest_refuse(self):
        for archive in [
            self.pack(omit="src/lib.rs"),
            self.pack(extra={"foreign": b"bad"}),
            self.pack(extra={"src/lib.rs": b"tampered"}),
            self.pack(extra={"Cargo.toml.orig": b"tampered"}),
        ]:
            with (
                self.subTest(archive=module.digest(archive)),
                self.assertRaises(ValueError),
            ):
                self.prepare(archive=archive)

    def test_archive_unsafe_duplicate_and_link_members_refuse(self):
        for name, kind in [
            ("../escape", "file"),
            ("wrong-0.1.0/src/lib.rs", "file"),
            (f"{module.NAME}-{module.VERSION}/src/lib.rs", "link"),
            (f"{module.NAME}-{module.VERSION}/src/lib.rs", "duplicate"),
        ]:
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode="w:gz") as archive:
                item = tarfile.TarInfo(name)
                if kind == "link":
                    item.type = tarfile.SYMTYPE
                    item.linkname = "/outside"
                archive.addfile(item)
                if kind == "duplicate":
                    archive.addfile(item)
            with self.subTest(name=name, kind=kind), self.assertRaises(ValueError):
                module.archive_members(output.getvalue())

    def test_target_alias_and_workspace_dependencies_cannot_escape_ordering(self):
        manifest = self.root / "Cargo.toml"
        for body in [
            '[target.\'cfg(unix)\'.build-dependencies]\nrenamed={package="quipu-signing",version="=0.1.0"}\n',
            '[workspace.dependencies]\nsigner={package="quipu-signing",version="=0.1.0"}\n[dependencies]\nsigner={workspace=true}\n',
        ]:
            manifest.write_text(body)
            self.assertTrue(module.root_dependency(self.root))
        manifest.write_text("[dependencies]\nsigner={workspace=true}\n")
        with self.assertRaises(ValueError):
            module.root_dependency(self.root)

    def test_registry_missing_unknown_yanked_checksum_and_version_refuse(self):
        for field, value in [
            ("crate", "other"),
            ("num", "0.2.0"),
            ("yanked", True),
            ("yanked", None),
            ("checksum", "0" * 64),
        ]:
            record = copy.deepcopy(self.registry)
            record["version"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                module.verify(self.plan, record, self.archive)
        for record in [{}, {"version": None}, {"crate": {"max_version": "0.1.0"}}]:
            with self.assertRaises(ValueError):
                module.verify(self.plan, record, self.archive)
        with self.assertRaises(ValueError):
            module.verify(self.plan, self.registry, self.archive + b"extra")

    def test_source_witness_change_refuses_even_matching_claimed_checksum(self):
        archive = self.pack(extra={"src/lib.rs": b"tampered"})
        plan = copy.deepcopy(self.plan)
        plan["archive_sha256"] = module.digest(archive)
        registry = copy.deepcopy(self.registry)
        registry["version"]["checksum"] = module.digest(archive)
        with self.assertRaisesRegex(ValueError, "source witnesses"):
            module.verify(plan, registry, archive)

    def test_actual_action_shared_route_runs_preparation_and_never_root_effects(self):
        action = yaml.safe_load(
            (
                SCRIPT.parents[2] / ".github/actions/crates-publish/action.yml"
            ).read_text()
        )
        destination = self.root / "scripts/ci"
        destination.mkdir(parents=True)
        shutil.copyfile(SCRIPT, destination / SCRIPT.name)
        metadata = self.root / "metadata.json"
        metadata.write_text(json.dumps(self.metadata))
        archive = self.root / "fixture.crate"
        archive.write_bytes(self.archive)
        output = self.root / "outputs"
        env = dict(
            os.environ,
            PACKAGE_NAME=module.NAME,
            MANIFEST_PATH=module.MANIFEST,
            SHARED_OPT_IN=module.OPT_IN,
            EXPECTED=module.VERSION,
            REF_NAME="main",
            METADATA=str(metadata),
            ARCHIVE=str(archive),
            GITHUB_OUTPUT=str(output),
        )
        steps = action["runs"]["steps"]
        selected = subprocess.run(
            ["bash", "-c", steps[0]["run"]],
            cwd=self.root,
            env=env,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(selected.returncode, 0, selected.stderr)
        self.assertEqual(output.read_text(), "route=shared-preparation\n")
        preparation = subprocess.run(
            ["bash", "-c", steps[1]["run"]],
            cwd=self.root,
            env=env,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(preparation.returncode, 0, preparation.stderr)
        self.assertIn('"publication_authorized": false', preparation.stdout)
        for step in steps[2:]:
            self.assertIn("steps.route.outputs.route == 'root'", step["if"])
        # The selected executable source has no registry, credential, publish or retry effect.
        self.assertNotIn("uses", steps[1])
        self.assertNotIn("cargo publish", steps[1]["run"])
        env["SHARED_OPT_IN"] = ""
        output.unlink()
        rejected = subprocess.run(
            ["bash", "-c", steps[0]["run"]],
            cwd=self.root,
            env=env,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(rejected.returncode, 0)
        self.assertFalse(output.exists())

    def test_legacy_root_step_bodies_credentials_and_callers_are_unchanged(self):
        baseline = json.loads(
            (SCRIPT.parent / "fixtures/crates-publish-root-v1.json").read_text()
        )
        action = yaml.safe_load(
            (
                SCRIPT.parents[2] / ".github/actions/crates-publish/action.yml"
            ).read_text()
        )
        for name, contract in baseline["inputs"].items():
            self.assertEqual(action["inputs"][name], contract)
        actual = copy.deepcopy(action["runs"]["steps"][3:])
        for step in actual:
            condition = step.pop("if")
            if condition != "steps.route.outputs.route == 'root'":
                prefix = "steps.route.outputs.route == 'root' && ("
                self.assertTrue(
                    condition.startswith(prefix) and condition.endswith(")")
                )
                step["if"] = condition[len(prefix) : -1]
        self.assertEqual(actual, baseline["steps"])

    def test_dependency_first_blocks_unqualified_root_and_has_no_publish_retry(self):
        self.assertFalse(module.root_dependency(self.root))
        manifest = self.root / "Cargo.toml"
        manifest.write_text(
            manifest.read_text() + '\n[dependencies]\nquipu-signing="=0.1.0"\n'
        )
        self.assertTrue(module.root_dependency(self.root))
        command = ["python3", str(SCRIPT), "root-ready", "--root", str(self.root)]
        result = subprocess.run(command, check=False, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("readback required", result.stderr)
        for value in ['"0.1.0"', '{version="=0.1.0",path="crates/quipu-signing"}']:
            manifest.write_text("[dependencies]\nquipu-signing=" + value + "\n")
            with self.assertRaises(ValueError):
                module.root_dependency(self.root)
        manifest.write_text('[dependencies]\nquipu-signing="=0.1.0"\n')
        paths = {}
        for name, value in [("plan", self.plan), ("registry-version", self.registry)]:
            path = self.root / (name + ".json")
            path.write_text(json.dumps(value))
            paths[name] = path
        archive = self.root / "registry.crate"
        archive.write_bytes(self.archive)
        result = subprocess.run(
            command
            + [
                "--plan",
                str(paths["plan"]),
                "--registry-version",
                str(paths["registry-version"]),
                "--archive",
                str(archive),
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("actual distribution acquisition", result.stderr)
        # A retained artifact check cannot authenticate, publish, retry or grant ownership.
        source = SCRIPT.read_text()
        self.assertNotIn("cargo publish", source)
        self.assertNotIn("urlopen", source)


if __name__ == "__main__":
    unittest.main()
