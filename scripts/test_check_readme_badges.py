#!/usr/bin/env python3
"""Show the README badge guard passing on the real README and firing on breakage."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("badges", ROOT / "scripts/check-readme-badges.py")
badges = importlib.util.module_from_spec(spec)
spec.loader.exec_module(badges)
README = (ROOT / "README.md").read_text()


class BadgeGuard(unittest.TestCase):
    def test_real_readme_passes(self):
        self.assertEqual(badges.problems(README), [])

    def test_removed_block_fails(self):
        start, end = README.index("<!-- badges:start"), README.index("<!-- badges:end -->")
        broken = README[:start] + README[end + len("<!-- badges:end -->"):]
        self.assertTrue(any("markers" in p for p in badges.problems(broken)))

    def test_each_missing_badge_fails(self):
        for what, href, _ in badges.REQUIRED:
            with self.subTest(what=what):
                broken = README.replace(f'<a href="{href}"><img', '<a href="#"><img', 1)
                self.assertTrue(any(what in p for p in badges.problems(broken)), what)

    def test_plain_text_link_does_not_count_as_a_badge(self):
        line = next(l for l in README.splitlines() if "arxiv.org/abs/2608.16813" in l and "<img" in l)
        broken = README.replace(line, '  <a href="https://arxiv.org/abs/2608.16813">arXiv</a>')
        self.assertTrue(any("arXiv badge missing" in p for p in badges.problems(broken)))

    def test_retired_owner_fails(self):
        broken = README.replace("https://github.com/intigraph/tambo\"><img",
                                "https://github.com/intigraph/tambo\"><img", 1)
        found = badges.problems(broken)
        self.assertTrue(any("retired" in p for p in found), found)


if __name__ == "__main__":
    unittest.main()
