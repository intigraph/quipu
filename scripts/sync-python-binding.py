#!/usr/bin/env python3
"""Keep bindings/python in step with the core crate's version.

release-plz bumps only the root Cargo.toml/Cargo.lock. bindings/python is a
separate (non-workspace) crate, so after a bump its `version` and the
`quipu-ai` path entry in its Cargo.lock are stale: `--locked` builds refuse,
and the wheel would carry the old version. This script rewrites the binding's
`version` to the core's and refreshes ONLY the path entry
(`cargo update -p quipu-ai`), leaving every registry pin untouched.

    scripts/sync-python-binding.py           # sync in place
    scripts/sync-python-binding.py --check   # exit 1 if out of sync

Run it before any `--locked` binding build and in the release PR.
"""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
BINDING = ROOT / "bindings/python"


def core_version() -> str:
    return tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]


def locked_core_version() -> str | None:
    lock = tomllib.loads((BINDING / "Cargo.lock").read_text())
    return next((p["version"] for p in lock["package"] if p["name"] == "quipu-ai"), None)


def main() -> int:
    want = core_version()
    manifest = BINDING / "Cargo.toml"
    text = manifest.read_text()
    have = tomllib.loads(text)["package"]["version"]
    locked = locked_core_version()
    if "--check" in sys.argv[1:]:
        if have == want == locked:
            return 0
        print(
            f"bindings/python out of sync with core {want}: "
            f"Cargo.toml version {have}, Cargo.lock quipu-ai {locked}. "
            "Run scripts/sync-python-binding.py",
            file=sys.stderr,
        )
        return 1
    if have != want:
        text, n = re.subn(
            r'(?m)^version = "[^"]*"', f'version = "{want}"', text, count=1
        )
        assert n == 1, "no [package] version line in bindings/python/Cargo.toml"
        manifest.write_text(text)
    if locked != want:
        subprocess.run(
            ["cargo", "update", "-p", "quipu-ai", "--manifest-path", str(manifest)],
            check=True,
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
