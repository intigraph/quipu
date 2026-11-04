#!/usr/bin/env python3
"""Is a commit's change to the Python binding ONLY the canonical version sync?

    scripts/python-binding-sync-only.py <commit>
    exit 0 = yes, 1 = no (the change is content), 2 = could not tell

`scripts/sync-python-binding.py` changes exactly three version strings: the
binding manifest's `[package] version`, and in the binding's Cargo.lock the
`version` of the binding's own package and of the core crate's path entry. It
sets all three to the root crate's version. That is release mechanics.

Anything else in those two files (a dependency, a feature, a registry pin, a
lock entry) is content and must stay documented, even in a commit that also
syncs the version (aegis-0zr review, #910). So: mask those three version
strings on both sides of the commit, require the rest to be byte-identical,
and require the new versions to equal the root crate's at that commit.

Plain text, no tomllib, so it runs on the Python 3.10 that ubuntu-22.04 ships.
"""

from __future__ import annotations

import re
import subprocess
import sys

MANIFEST = "bindings/python/Cargo.toml"
LOCK = "bindings/python/Cargo.lock"
MASK = 'version = "<synced>"'


def show(commit: str, path: str) -> str | None:
    """The file at `commit`, or None if it does not exist there."""
    result = subprocess.run(
        ["git", "show", f"{commit}:{path}"], capture_output=True, text=True
    )
    return result.stdout if result.returncode == 0 else None


def package_field(manifest: str, field: str) -> str | None:
    """`field` in the `[package]` table of a manifest, by its exact line."""
    table = re.search(r"(?ms)^\[package\]\n(.*?)(?=^\[|\Z)", manifest)
    if not table:
        return None
    line = re.search(rf'(?m)^{field}\s*=\s*"([^"]*)"\s*$', table.group(1))
    return line.group(1) if line else None


def mask_manifest(manifest: str) -> tuple[str, str | None]:
    table = re.search(r"(?ms)^\[package\]\n(.*?)(?=^\[|\Z)", manifest)
    if not table:
        return manifest, None
    body, n = re.subn(r'(?m)^version\s*=\s*"([^"]*)"\s*$', MASK, table.group(1), count=1)
    if n != 1:
        return manifest, None
    version = package_field(manifest, "version")
    return manifest[: table.start(1)] + body + manifest[table.end(1) :], version


def mask_lock(lock: str, names: set[str]) -> tuple[str, dict[str, str]]:
    """Mask the version of each named package entry; report what was found."""
    found: dict[str, str] = {}

    def entry(match: re.Match[str]) -> str:
        name, version = match.group(2), match.group(4)
        if name not in names:
            return match.group(0)
        found[name] = version
        return f"{match.group(1)}{match.group(2)}{match.group(3)}{MASK}"

    masked = re.sub(
        r'(?m)^(\[\[package\]\]\nname = ")([^"]+)("\n)version = "([^"]*)"$',
        entry,
        lock,
    )
    return masked, found


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    commit = sys.argv[1]
    parent = f"{commit}^"
    root = show(commit, "Cargo.toml")
    core = package_field(root, "name") if root else None
    want = package_field(root, "version") if root else None
    new_manifest, old_manifest = show(commit, MANIFEST), show(parent, MANIFEST)
    new_lock, old_lock = show(commit, LOCK), show(parent, LOCK)
    if not (core and want and new_manifest and old_manifest and new_lock and old_lock):
        # A created or deleted binding file, or no root package: not a sync.
        return 1
    binding = package_field(new_manifest, "name")
    if not binding or package_field(old_manifest, "name") != binding:
        return 1

    old_m, _ = mask_manifest(old_manifest)
    new_m, new_version = mask_manifest(new_manifest)
    if old_m != new_m or new_version != want:
        return 1

    names = {binding, core}
    old_l, old_found = mask_lock(old_lock, names)
    new_l, new_found = mask_lock(new_lock, names)
    if old_l != new_l or set(old_found) != names or new_found != {n: want for n in names}:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
