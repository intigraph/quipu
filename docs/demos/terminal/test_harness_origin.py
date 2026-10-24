#!/usr/bin/env python3
"""Keep the vendored recorder bound to its reviewed canonical source bytes."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def verify(root):
    origin = json.loads((root / "harness-origin.json").read_text())
    if origin["repository"] != "https://github.com/intigraph/tambo":
        raise ValueError("unexpected canonical repository")
    if not re.fullmatch(r"[0-9a-f]{40}", origin["commit"]):
        raise ValueError("origin must name an exact commit")
    if origin["path"] != "docs/demos/terminal-harness/render.py":
        raise ValueError("unexpected canonical path")
    actual = hashlib.sha256((root / "render.py").read_bytes()).hexdigest()
    if actual != origin["sha256"]:
        raise ValueError("vendored recorder drift")
    manifest = json.loads((root / "manifest.json").read_text())
    if actual != manifest["inputs_sha256"]["render.py"]:
        raise ValueError("recording used different recorder bytes")


class OriginControls(unittest.TestCase):
    def test_recorded_bytes_match_the_pinned_origin(self):
        verify(HERE)

    def test_mutating_the_vendor_without_repinning_refuses(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for name in ("render.py", "harness-origin.json", "manifest.json"):
                shutil.copy2(HERE / name, root / name)
            with (root / "render.py").open("a") as output:
                output.write("\n# drift\n")
            with self.assertRaisesRegex(ValueError, "drift"):
                verify(root)


if __name__ == "__main__":
    unittest.main()
