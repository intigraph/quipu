#!/usr/bin/env python3
"""Keep PR wheel fan-out small; retain full platform coverage outside PRs."""
import argparse
import itertools
import json
import re
import subprocess
import sys
import tempfile

PLATFORMS = [
    {"name": "linux-x86_64", "runner": "ubuntu-24.04", "manylinux": "auto"},
    {"name": "linux-aarch64", "runner": "ubuntu-24.04-arm", "manylinux": "auto"},
    {"name": "macos-arm64", "runner": "macos-15"},
]
PYTHONS = ["3.11", "3.12", "3.13", "3.14"]
SHA = re.compile(r"[0-9a-f]{40}\Z")
PLATFORM_CODE = re.compile(
    rb"macos|darwin|apple|\bunix\b|\bposix\b|std::os|\blibc\b|target_(?:os|arch|family|vendor|env)",
    re.IGNORECASE,
)


def git_bytes(*args):
    """Bound both Git's runtime and captured output; uncertainty runs all platforms."""
    with tempfile.TemporaryFile() as output:
        subprocess.run(
            ["git", "--no-pager", *args], stdout=output, stderr=subprocess.DEVNULL,
            check=True, timeout=30,
        )
        if output.tell() > 8 * 1024 * 1024:
            raise ValueError("diff exceeds selector budget")
        output.seek(0)
        return output.read()


def mac_required(paths, patch):
    for path in paths:
        if path.startswith("src/") and not path.endswith(".rs"):
            return True
        if path in {"Cargo.toml", "Cargo.lock", "build.rs", "pyproject.toml"}:
            return True
        if path.startswith(("bindings/", ".cargo/", "src/render/")):
            return True
        if path in {"src/store/ownership.rs", "scripts/sync-python-binding.py"}:
            return True
        if path.startswith("tests/fixtures/bindings-conformance/"):
            return True
        # Workflow/selector changes exercise the full matrix, including its consumers.
        if path.startswith(".github/") or path.startswith("scripts/"):
            return True
        if not (path.startswith(("src/", "tests/", "docs/")) or
                path in {"README.md", "CHANGELOG.md", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE"}):
            return True
    return bool(PLATFORM_CODE.search(patch))


def select(event, base, head):
    if event != "pull_request":
        return True, "full platform coverage outside pull requests"
    if not SHA.fullmatch(base) or not SHA.fullmatch(head):
        return True, "unknown PR revisions; full platform coverage"
    try:
        revision = f"{base}...{head}"
        names = git_bytes("diff", "--no-ext-diff", "--no-textconv", "--no-renames",
                          "--name-only", "-z", revision, "--")
        paths = [name.decode("utf-8") for name in names.split(b"\0") if name]
        if not paths:
            return True, "empty or unavailable changed-path evidence; full coverage"
        patch = git_bytes("diff", "--no-ext-diff", "--no-textconv", "--no-renames",
                          "--unified=0", revision, "--", "src", "tests")
        return mac_required(paths, patch), "PR paths and platform-sensitive diff inspected"
    except (OSError, ValueError, subprocess.SubprocessError):
        return True, "unreadable PR diff; full platform coverage"


def matrices(include_mac):
    wheels = [dict(platform) for platform in PLATFORMS
              if include_mac or platform["name"] != "macos-arm64"]
    tests = [{"name": platform["name"], "runner": platform["runner"], "python": python}
             for platform, python in itertools.product(wheels, PYTHONS)]
    return {"wheels": {"include": wheels}, "tests": {"include": tests}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event", required=True)
    parser.add_argument("--base", default="")
    parser.add_argument("--head", default="")
    args = parser.parse_args()
    include_mac, reason = select(args.event, args.base, args.head)
    print(f"macOS={include_mac}: {reason}", file=sys.stderr)
    for name, matrix in matrices(include_mac).items():
        print(f"{name}={json.dumps(matrix, separators=(',', ':'))}")


if __name__ == "__main__":
    main()
