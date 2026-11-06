#!/usr/bin/env python3
"""Fail if README.md loses its badge row or the DOI/arXiv citation links.

#696 dropped the row in a README rewrite and nothing noticed for twelve days
(aegis-58r). Each badge must stay inside the marked block as a link AND an
image, and must point at intigraph, not a retired owner or stack name.

Exit 0 = row intact. 1 = something is missing. 2 = cannot read the README.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

# (what, link href, image src fragment)
REQUIRED = [
    ("license", "LICENSE", "img.shields.io/badge/License-Apache_2.0"),
    ("CI", "https://github.com/intigraph/quipu/actions/workflows/ci.yml",
     "https://github.com/intigraph/quipu/actions/workflows/ci.yml/badge.svg"),
    ("crates.io", "https://crates.io/crates/quipu-ai", "img.shields.io/crates/v/quipu-ai"),
    ("docs", "https://intigraph.github.io/quipu/", "img.shields.io/badge/docs-"),
    ("Zenodo DOI", "https://doi.org/10.5281/zenodo.21878428", "10.5281%2Fzenodo.21878428"),
    ("arXiv", "https://arxiv.org/abs/2608.16813", "img.shields.io/badge/arXiv-2608.16813"),
    ("stack", "https://github.com/intigraph/tambo", "img.shields.io/badge/stack-"),
]
# Deliberate former-name refusal control: these patterns must remain rejected.
RETIRED = ("github.com/scbrown/", "github.com/scbrown/caboodle", "caboodle")
BLOCK = re.compile(r"<!-- badges:start\b.*?-->(.*?)<!-- badges:end -->", re.S)
LINK = re.compile(r'<a href="([^"]+)"><img src="([^"]+)" alt="[^"]+"/></a>')


def problems(text: str) -> list[str]:
    block = BLOCK.search(text)
    if block is None:
        return ["badge block markers <!-- badges:start --> ... <!-- badges:end --> are missing"]
    badges = LINK.findall(block.group(1))
    found = []
    for what, href, src in REQUIRED:
        if not any(h == href and src in s for h, s in badges):
            found.append(f"{what} badge missing: expected <a href=\"{href}\"> around an image containing {src!r}")
    for href, src in badges:
        for retired in RETIRED:
            if retired in href or retired in src:
                found.append(f"badge points at a retired name ({retired}): {href}")
    for citation in ("https://doi.org/10.5281/zenodo.21878428", "https://arxiv.org/abs/2608.16813"):
        if citation not in text:
            found.append(f"citation link missing from README: {citation}")
    return found


def main(argv: list[str]) -> int:
    path = Path(argv[1]) if len(argv) > 1 else Path(__file__).resolve().parents[1] / "README.md"
    try:
        text = path.read_text()
    except OSError as error:
        print(f"CANNOT TELL: {path}: {error}", file=sys.stderr)
        return 2
    found = problems(text)
    for problem in found:
        print(f"FAIL: {problem}")
    if found:
        print(f"{len(found)} README badge problem(s). Restore the row; see aegis-58r.")
        return 1
    print(f"OK: {len(REQUIRED)} README badges and both citation links present")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
