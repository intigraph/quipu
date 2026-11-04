#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Offline preparation/readback contracts; this program never publishes or fetches."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import re
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path, PurePosixPath

NAME = "quipu-signing"
VERSION = "0.1.0"
MANIFEST = "crates/quipu-signing/Cargo.toml"
OPT_IN = "prepare-quipu-signing-0.1.0"
MAX_ARCHIVE = 16 * 1024 * 1024
# Published immutable release, independently read back from crates.io on 2026-10-30.
# Its VCS receipt binds 3943cd305e8d271a7ed481493ba22a307fa675a8.
ADMITTED_CHECKSUM = "9590d3ae3ad7383f3b3325fd7649e873f26026460fc9767251028aa2a00c6575"
REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"


def refuse(message: str) -> None:
    raise ValueError(message)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def select(name: str, manifest: str, opt_in: str) -> str:
    """Only the unchanged default root route may run without explicit opt-in."""
    if (name, manifest, opt_in) == ("quipu-ai", "Cargo.toml", ""):
        return "root"
    if (name, manifest, opt_in) == (NAME, MANIFEST, OPT_IN):
        return "shared-preparation"
    refuse("unknown package/manifest or absent/invalid shared preparation opt-in")
    return "unreachable"


def read_json(path: Path) -> dict:
    value = json.loads(path.read_text())
    if not isinstance(value, dict):
        refuse("expected a JSON object")
    return value


def source_identity(root: Path, metadata: dict, expected: str, ref: str) -> dict:
    if expected != VERSION or not re.fullmatch(r"\d+\.\d+\.\d+", expected):
        refuse("shared package intended version must be exactly 0.1.0")
    if not ref or re.match(r"rehearsal-|test-", ref) or ref.endswith("-rehearsal"):
        refuse("empty or rehearsal ref")
    manifest = root / MANIFEST
    document = tomllib.loads(manifest.read_text())
    package = document["package"]
    if (package.get("name"), package.get("version"), package.get("publish")) != (
        NAME,
        VERSION,
        False,
    ):
        refuse("source package identity or publish=false boundary changed")
    packages = metadata.get("packages", [])
    if len(packages) != 1:
        refuse("expected exactly one metadata package")
    actual = packages[0]
    if (actual.get("name"), actual.get("version"), actual.get("publish")) != (
        NAME,
        VERSION,
        [],
    ) or Path(actual.get("manifest_path", "")).resolve() != manifest.resolve():
        refuse("Cargo metadata does not bind the intended manifest/name/version")
    return {"name": NAME, "version": VERSION, "manifest": MANIFEST}


def archive_members(data: bytes) -> dict[str, bytes]:
    if len(data) > MAX_ARCHIVE:
        refuse("archive too large")
    result = {}
    total = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or len(path.parts) < 2:
                refuse("unsafe archive path")
            if path.parts[0] != f"{NAME}-{VERSION}":
                refuse("archive package prefix mismatch")
            if member.isdir():
                continue
            if not member.isfile() or member.name in result:
                refuse("archive link, special file or duplicate member")
            total += member.size
            if total > MAX_ARCHIVE:
                refuse("expanded archive too large")
            stream = archive.extractfile(member)
            if stream is None:
                refuse("unreadable archive member")
            result[member.name] = stream.read()
    return result


def prepare(
    root: Path, metadata: dict, archive: bytes, expected: str, ref: str
) -> dict:
    identity = source_identity(root, metadata, expected, ref)
    members = archive_members(archive)
    prefix = f"{NAME}-{VERSION}/"
    if members.get(prefix + "Cargo.toml.orig") != (root / MANIFEST).read_bytes():
        refuse("packaged original manifest differs from source")
    normalized = tomllib.loads(members[prefix + "Cargo.toml"].decode())
    package = normalized["package"]
    if (package.get("name"), package.get("version"), package.get("publish")) != (
        NAME,
        VERSION,
        False,
    ):
        refuse("normalized package identity/publish boundary mismatch")
    sources = {}
    base = root / Path(MANIFEST).parent
    for name, data in members.items():
        relative = name.removeprefix(prefix)
        if relative in {"Cargo.toml", "Cargo.toml.orig", ".cargo_vcs_info.json"}:
            continue
        source = base / relative
        if not source.is_file() or source.is_symlink() or source.read_bytes() != data:
            refuse(f"packaged source does not match checkout: {relative}")
        sources[relative] = digest(data)
    tracked = (
        subprocess.run(
            ["git", "ls-files", "-z", "--", Path(MANIFEST).parent.as_posix()],
            cwd=root,
            check=True,
            capture_output=True,
        )
        .stdout.decode()
        .split("\0")
    )
    expected_sources = {
        Path(name).relative_to(Path(MANIFEST).parent).as_posix()
        for name in tracked
        if name and name != MANIFEST
    }
    if set(sources) != expected_sources:
        refuse("archive does not contain the exact tracked package source set")
    if "src/lib.rs" not in sources or "Cargo.lock" not in sources:
        refuse("archive lacks library or lockfile")
    return {
        "schema": "quipu-signing-preparation/v1",
        **identity,
        "manifest_sha256": digest((root / MANIFEST).read_bytes()),
        "archive_sha256": digest(archive),
        "source_sha256": sources,
        "distribution_verified": False,
        "publication_authorized": False,
        "next": "separate publisher admission, then exact version JSON and archive readback",
    }


def verify(plan: dict, version: dict, archive: bytes) -> dict:
    """Verify retained bytes, never a boolean receipt or max_version headline."""
    if (
        plan.get("schema"),
        plan.get("name"),
        plan.get("version"),
        plan.get("manifest"),
    ) != ("quipu-signing-preparation/v1", NAME, VERSION, MANIFEST):
        refuse("unknown preparation identity")
    actual = version.get("version")
    if (
        not isinstance(actual, dict)
        or (actual.get("crate"), actual.get("num")) != (NAME, VERSION)
        or actual.get("yanked") is not False
    ):
        refuse("registry version missing, mismatched, yanked or UNKNOWN")
    checksum = digest(archive)
    if checksum != plan.get("archive_sha256") or checksum != actual.get("checksum"):
        refuse("registry archive checksum differs from prepared exact artifact")
    members = archive_members(archive)
    prefix = f"{NAME}-{VERSION}/"
    if digest(members.get(prefix + "Cargo.toml.orig", b"")) != plan.get(
        "manifest_sha256"
    ):
        refuse("registry original manifest differs")
    expected_sources = plan.get("source_sha256")
    if not isinstance(expected_sources, dict) or not expected_sources:
        refuse("missing source witness map")
    actual_sources = {
        key.removeprefix(prefix): digest(data)
        for key, data in members.items()
        if key.removeprefix(prefix)
        not in {"Cargo.toml", "Cargo.toml.orig", ".cargo_vcs_info.json"}
    }
    if actual_sources != expected_sources:
        refuse("registry source witnesses differ")
    return {
        "artifact_verified": True,
        "name": NAME,
        "version": VERSION,
        "archive_sha256": checksum,
        "publication_authorized": False,
        "provenance": "caller-supplied retained bytes; verify actual acquisition separately",
    }


def root_dependency(root: Path) -> bool:
    document = tomllib.loads((root / "Cargo.toml").read_text())
    tables = [document]
    tables.extend(document.get("target", {}).values())
    found = False
    for table in tables:
        for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
            for alias, spec in table.get(kind, {}).items():
                if isinstance(spec, dict) and spec.get("workspace") is True:
                    inherited = (
                        document.get("workspace", {}).get("dependencies", {}).get(alias)
                    )
                    if inherited is None:
                        refuse("unresolved workspace dependency")
                    spec = inherited
                name = spec.get("package", alias) if isinstance(spec, dict) else alias
                if name != NAME:
                    continue
                requirement = spec.get("version") if isinstance(spec, dict) else spec
                if requirement != f"={VERSION}" or (
                    isinstance(spec, dict)
                    and any(k in spec for k in ("git", "path", "registry"))
                ):
                    refuse(
                        "root shared dependency needs exact admitted registry version"
                    )
                found = True
    return found


def root_acquisition(root: Path) -> dict:
    """Require Cargo's exact registry resolution to the qualified immutable bytes.

    Cargo authenticates downloaded contents against this lock checksum. This
    offline preflight never fetches, publishes, or grants publisher authority.
    Caller-supplied preparation/registry fixtures cannot replace the lock.
    """
    lock = root / "Cargo.lock"
    if not lock.is_file():
        refuse("actual distribution acquisition: exact shared registry readback required in Cargo.lock")
    packages = tomllib.loads(lock.read_text()).get("package", [])
    matches = [package for package in packages if package.get("name") == NAME]
    if len(matches) != 1 or any(
        matches[0].get(key) != expected
        for key, expected in (
            ("version", VERSION), ("source", REGISTRY_SOURCE),
            ("checksum", ADMITTED_CHECKSUM),
        )
    ):
        refuse("actual distribution acquisition must lock the admitted registry version/checksum")
    return {"route": "qualified-root", "shared_dependency": True,
            "name": NAME, "version": VERSION, "checksum": ADMITTED_CHECKSUM}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=("select", "prepare", "verify", "root-ready")
    )
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--name", default="quipu-ai")
    parser.add_argument("--manifest", default="Cargo.toml")
    parser.add_argument("--opt-in", default="")
    parser.add_argument("--expected-version", default="")
    parser.add_argument("--ref", default="")
    parser.add_argument("--metadata", type=Path)
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--plan", type=Path)
    parser.add_argument("--registry-version", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "select":
            print(select(args.name, args.manifest, args.opt_in))
            return 0
        if args.command == "root-ready":
            result = (root_acquisition(args.root) if root_dependency(args.root)
                      else {"route": "legacy-root", "shared_dependency": False})
            print(json.dumps(result, sort_keys=True))
            return 0
        if args.command == "prepare":
            if select(args.name, args.manifest, args.opt_in) != "shared-preparation":
                refuse("preparation requires explicit shared route")
            if not args.metadata or not args.archive:
                refuse("preparation requires metadata and package archive")
            result = prepare(
                args.root,
                read_json(args.metadata),
                args.archive.read_bytes(),
                args.expected_version,
                args.ref,
            )
        else:
            if not args.plan or not args.registry_version or not args.archive:
                refuse(
                    "exact shared registry readback required before root progression"
                )
            result = verify(
                read_json(args.plan),
                read_json(args.registry_version),
                args.archive.read_bytes(),
            )
        print(json.dumps(result, sort_keys=True, indent=2))
        return 0
    except (
        ValueError,
        OSError,
        KeyError,
        tarfile.TarError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"REFUSED: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
