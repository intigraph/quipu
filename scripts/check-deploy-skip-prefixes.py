#!/usr/bin/env python3
"""Standing conservative no-reference proof; does not itself authorize a skip."""

import argparse
import json
from pathlib import Path, PurePosixPath
import subprocess


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args], capture_output=True, check=True
    ).stdout


def check(root):
    manifest = json.loads((root / "deploy-skip-prefixes.json").read_text())
    if set(manifest) != {"version", "prefixes"} or manifest["version"] != 1:
        raise ValueError("unsupported prefix manifest")
    prefixes = manifest["prefixes"]
    if not isinstance(prefixes, list) or not prefixes or len(set(prefixes)) != len(prefixes):
        raise ValueError("prefixes must be a nonempty unique list")
    tracked = git(root, "ls-files", "-z").decode().split("\0")
    roots = set()
    for prefix in prefixes:
        if not isinstance(prefix, str) or not prefix.endswith("/"):
            raise ValueError("prefix must end in a directory separator")
        parts = prefix[:-1].split("/")
        if parts[0] not in {"benchmark", "docs"} or any(
            part in {"", ".", ".."} or any(c in part for c in "*?[]\\\n\r\t")
            for part in parts
        ):
            raise ValueError("noncanonical or runtime prefix")
        if not any(path.startswith(prefix) for path in tracked if path):
            raise ValueError("prefix has no tracked files: " + prefix)
        # Scan the broader root as well: joining a root with a suffix must not
        # evade the standing check merely because the full prefix is not literal.
        roots.add(parts[0] + "/")
    sources = [
        path for path in tracked
        if path and (
            path in {"Cargo.toml", "build.rs"}
            or path.startswith(("src/", "build/"))
            and (path.endswith(".rs") or PurePosixPath(path).name == "Cargo.toml")
        )
    ]
    if not {"Cargo.toml", "build.rs"}.issubset(sources) or not any(
        path.startswith("src/") for path in sources
    ):
        raise ValueError("missing build/source inventory")
    hits, controls = [], 0
    for path in sources:
        source = root / path
        if source.is_symlink() or not source.is_file():
            raise ValueError("nonregular source: " + path)
        text = source.read_text()
        controls += text.count("include_str!") + text.count("include_bytes!")
        for number, line in enumerate(text.splitlines(), 1):
            if any(needle in line for needle in roots):
                hits.append(f"{path}:{number}")
    if not controls:
        raise ValueError("positive include control missing")
    if hits:
        raise ValueError("prefix root referenced in build inputs: " + ", ".join(hits))
    return {"prefixes": prefixes, "sources": len(sources), "include_controls": controls}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        print(json.dumps(check(args.root)))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"deploy skip prefix proof REFUSED: {error}\n")


if __name__ == "__main__":
    main()
