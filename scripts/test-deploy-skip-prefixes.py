"""Exercise the ordered-commit hazard and fail-closed proof boundaries."""

import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "prefix_proof", Path(__file__).with_name("check-deploy-skip-prefixes.py")
)
PROOF = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROOF)


class PrefixProofTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("Cargo.toml", '[package]\nname="fixture"\n')
        self.write("build.rs", 'fn main() { let _ = include_str!("version.txt"); }\n')
        self.write("version.txt", "one\n")
        self.write("src/lib.rs", "pub fn answer() {}\n")
        self.write("benchmark/census/agent/input.txt", "first\n")
        self.manifest(["benchmark/census/agent/"])
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        subprocess.run(["git", "-C", str(self.root), "add", "."], check=True)

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def manifest(self, prefixes):
        self.write("deploy-skip-prefixes.json", json.dumps({"version": 1, "prefixes": prefixes}))

    def test_safe_fixture_and_actual_positive_control(self):
        self.assertEqual(PROOF.check(self.root)["include_controls"], 1)

    def test_ordered_include_then_data_change_remains_refused(self):
        self.write("src/lib.rs", 'const INPUT: &str = include_str!("../benchmark/census/agent/input.txt");\n')
        for value in ("first\n", "second\n"):
            self.write("benchmark/census/agent/input.txt", value)
            with self.assertRaisesRegex(ValueError, "referenced"):
                PROOF.check(self.root)

    def test_joined_path_and_manifest_read_are_conservatively_refused(self):
        for path, text in (
            ("build.rs", 'let p = "benchmark/".to_owned() + "census/agent/input.txt";'),
            ("Cargo.toml", '# benchmark/census/agent/ is a build input'),
        ):
            with self.subTest(path=path):
                original = (self.root / path).read_text()
                self.write(path, original + "\n" + text)
                with self.assertRaisesRegex(ValueError, "referenced"):
                    PROOF.check(self.root)
                self.write(path, original)

    def test_missing_positive_control_is_not_zero_reference_proof(self):
        self.write("build.rs", "fn main() {}")
        with self.assertRaisesRegex(ValueError, "positive include control"):
            PROOF.check(self.root)

    def test_runtime_traversal_empty_duplicate_and_absent_prefixes_refuse(self):
        for prefixes in (["src/"], ["shapes/"], ["../docs/"], [],
                         ["benchmark/census/agent/"] * 2, ["benchmark/absent/"]):
            with self.subTest(prefixes=prefixes):
                self.manifest(prefixes)
                with self.assertRaises(ValueError):
                    PROOF.check(self.root)

    def test_missing_inventory_and_symlink_refuse(self):
        (self.root / "src/lib.rs").unlink()
        (self.root / "src/lib.rs").symlink_to(self.root / "version.txt")
        with self.assertRaisesRegex(ValueError, "nonregular"):
            PROOF.check(self.root)


if __name__ == "__main__":
    unittest.main()
