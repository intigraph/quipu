#!/usr/bin/env python3
"""Recognize only canonical core/Python Cargo version synchronization."""

import subprocess
import sys
import tomllib


def document(ref, path):
    listed = subprocess.run(
        ["git", "ls-tree", "--name-only", ref, "--", path],
        capture_output=True, check=True,
    )
    if not listed.stdout:
        return None
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"], capture_output=True, check=True
    )
    return tomllib.loads(result.stdout.decode())


def manifest_pair(commit, path, name, previous, version):
    before, after = document(f"{commit}^", path), document(commit, path)
    if before is None or after is None:
        return False
    if before["package"]["name"] != name or after["package"]["name"] != name:
        return False
    if before["package"]["version"] != previous or after["package"]["version"] != version:
        return False
    after["package"]["version"] = previous
    return before == after


def lock_pair(commit, path, names, previous, version):
    before, after = document(f"{commit}^", path), document(commit, path)
    if before is None or after is None:
        return before is None and after is None
    for lock, expected in ((before, previous), (after, version)):
        for package in lock.get("package", []):
            # Registry/git packages with a matching name are not local release
            # identities. Keep their full entries, including versions, intact.
            if package.get("name") in names and "source" not in package:
                if package.get("version") != expected:
                    return False
                del package["version"]
    return before == after


def canonical(commit, version):
    core = document(f"{commit}^", "Cargo.toml")
    previous = core["package"]["version"]
    return (
        previous != version
        and manifest_pair(commit, "Cargo.toml", "quipu-ai", previous, version)
        and manifest_pair(commit, "bindings/python/Cargo.toml", "quipu-kg", previous, version)
        and lock_pair(commit, "Cargo.lock", {"quipu-ai"}, previous, version)
        and lock_pair(commit, "bindings/python/Cargo.lock", {"quipu-ai", "quipu-kg"}, previous, version)
    )


if __name__ == "__main__":
    try:
        sys.exit(0 if canonical(*sys.argv[1:]) else 1)
    except (KeyError, TypeError, ValueError, OSError, subprocess.CalledProcessError):
        # Unreadable, malformed or unexpected Cargo data cannot earn exemption.
        sys.exit(1)
