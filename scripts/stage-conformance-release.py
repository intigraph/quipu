#!/usr/bin/env python3
"""Stage a verified release report; refuse absent, wrong-tag or unsafe archives."""
from __future__ import annotations

import argparse
import json
import re
import tarfile
from pathlib import Path, PurePosixPath


def stage(archive: Path, destination: Path, tag: str) -> None:
    with tarfile.open(archive, "r:gz") as source:
        files = {}
        for member in source.getmembers():
            name = PurePosixPath(member.name)
            if member.isdir() and str(name) in {".", "badges"}:
                continue
            if not member.isfile() or name.is_absolute() or ".." in name.parts:
                raise ValueError("unsafe release report member")
            if str(name) not in {"conformance.md", "provenance.json"} and not (len(name.parts) == 2 and name.parts[0] == "badges" and name.suffix == ".json"):
                raise ValueError("unexpected release report member")
            if str(name) in files or len(files) >= 20 or member.size > 2 * 1024 * 1024:
                raise ValueError("duplicate/oversize release report member")
            files[str(name)] = source.extractfile(member).read()
        provenance = json.loads(files["provenance.json"])
        if provenance.get("schema") != "quipu-release-conformance/v1" or provenance.get("release_tag") != tag:
            raise ValueError("release report tag differs from selected release")
        if not re.fullmatch(r"[0-9a-f]{40}", provenance.get("revision", "")) or not re.fullmatch(r"https://github.com/[^/]+/[^/]+/actions/runs/[0-9]+", provenance.get("run_url", "")):
            raise ValueError("release report lacks source/run provenance")
        if not files.get("conformance.md"):
            raise ValueError("missing release report page")
    destination.mkdir(parents=True, exist_ok=True)
    for name, content in files.items():
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.is_symlink() or not target.resolve().is_relative_to(destination.resolve()):
            raise ValueError("release report destination escapes owned root")
        target.write_bytes(content)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    stage(args.archive, args.destination, args.tag)
