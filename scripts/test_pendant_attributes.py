#!/usr/bin/env python3
"""Exercise repository pack attributes with real Git, including exclusions."""

import pathlib
import shutil
import subprocess
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
MEMBERS = ("export.nt", "shapes.ttl", "manifest.json", "manifest.ttl")


class PendantAttributes(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = pathlib.Path(self.temp.name)
        subprocess.run(
            ["git", "init", "--quiet", str(self.repo)], check=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        shutil.copyfile(ROOT / ".gitattributes", self.repo / ".gitattributes")

    def merge_attributes(self, paths):
        result = subprocess.run(
            ["git", "-C", str(self.repo), "check-attr", "-z", "merge", "--stdin"],
            input=b"\0".join(path.encode() for path in paths) + b"\0",
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        fields = result.stdout.decode().split("\0")[:-1]
        self.assertEqual(len(fields), 3 * len(paths))
        self.assertEqual(fields[1::3], ["merge"] * len(paths))
        return dict(zip(fields[0::3], fields[2::3]))

    def test_pendant_members_select_driver_at_root_and_nested(self):
        paths = [f"{prefix}pendant/{name}"
                 for prefix in ("", "docs/book/src/datalinks/", "path with spaces/")
                 for name in MEMBERS]
        self.assertEqual(self.merge_attributes(paths), dict.fromkeys(paths, "quipu"))

    def test_legacy_qpack_members_keep_driver(self):
        paths = [f"{prefix}qpack/{name}"
                 for prefix in ("", "nested/") for name in MEMBERS]
        self.assertEqual(self.merge_attributes(paths), dict.fromkeys(paths, "quipu"))

    def test_unrelated_and_binary_paths_do_not_select_driver(self):
        paths = ["other/export.nt", "pendant-other/export.nt",
                 "pendant/export.nq", "pendant/queries.ttl",
                 "pendant/module.pendant.db", "module.pendant.db",
                 "nested/qpack-other/manifest.json"]
        self.assertEqual(self.merge_attributes(paths), dict.fromkeys(paths, "unspecified"))


if __name__ == "__main__":
    unittest.main()
