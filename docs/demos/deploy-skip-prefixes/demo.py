"""Isolated source proof demo: no deployment or production file writes."""

import importlib.util
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location(
    "prefix_tests", ROOT / "scripts/test-deploy-skip-prefixes.py"
)
TESTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TESTS)
base = "ce86ba4cc1b5bf0bcf98a67c2de8a6e1f075946b"
workflow = subprocess.check_output(
    ["git", "-C", str(ROOT), "show", base + ":.github/workflows/ci.yml"], text=True
)
print("BASE", base, "standing prefix job present:", "deploy-skip-proof:" in workflow)
fixture = TESTS.PrefixProofTests()
fixture.setUp()
try:
    print("CANDIDATE safe fixture:", TESTS.PROOF.check(fixture.root))
    fixture.write(
        "src/lib.rs",
        'const DATA: &str = include_str!("../benchmark/census/agent/input.txt");\n',
    )
    for label, value in (("A introduces include", "first"), ("B changes data only", "second")):
        fixture.write("benchmark/census/agent/input.txt", value)
        try:
            TESTS.PROOF.check(fixture.root)
        except ValueError as error:
            print(label + ": REFUSED: " + str(error))
        else:
            raise AssertionError("unsafe proof accepted")
finally:
    fixture.doCleanups()
