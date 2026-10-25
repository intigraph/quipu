#!/usr/bin/env python3
"""Validate co-located shape deployment policy; emit the same plan CI checked.

# arming: ci and library; the mandatory source-invariants job and deploy consumer
"""
import argparse
import json
from pathlib import Path, PurePosixPath
import re
import sys


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate manifest key: {key}")
        result[key] = value
    return result


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"not a regular source file: {path.name}")


def plan(directory):
    directory = Path(directory)
    regular(directory / "MANIFEST")
    manifest = json.loads((directory / "MANIFEST").read_text(), object_pairs_hook=unique_object)
    if (not isinstance(manifest, dict) or set(manifest) != {"schema_version", "files"}
            or type(manifest["schema_version"]) is not int or manifest["schema_version"] != 1
            or not isinstance(manifest["files"], list) or not manifest["files"]):
        raise ValueError("expected schema_version 1 and a nonempty files list")
    result = {"schema_version": 1, "load": [], "exclude": []}
    seen, names, destinations = set(), set(), set()
    for entry in manifest["files"]:
        if not isinstance(entry, dict) or entry.get("action") not in ("load", "exclude"):
            raise ValueError("each file needs action load or exclude")
        expected = {"file", "action", "reason"} | ({"name"} if entry["action"] == "load" else set())
        if set(entry) != expected:
            raise ValueError("unexpected or missing manifest fields")
        filename = entry["file"]
        if (not isinstance(filename, str) or not filename.endswith(".ttl")
                or not all(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", p)
                           for p in filename.split("/"))):
            raise ValueError("shape file must be a normalized relative .ttl path")
        if filename in seen:
            raise ValueError(f"duplicate classification: {filename}")
        seen.add(filename)
        source = directory
        for part in PurePosixPath(filename).parts:
            source /= part
            if source.is_symlink():
                raise ValueError(f"symlink in shape source: {filename}")
        regular(source)
        if not isinstance(entry["reason"], str) or not entry["reason"].strip():
            raise ValueError(f"classification needs a reason: {filename}")
        if entry["action"] == "load":
            name, destination = entry["name"], source.name
            if not isinstance(name, str) or not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", name):
                raise ValueError("load needs a stable registry name")
            if name in names or destination in destinations:
                raise ValueError("duplicate load name or staging filename")
            names.add(name)
            destinations.add(destination)
            result["load"].append({"name": name, "file": filename, "dest_file": destination})
        else:
            result["exclude"].append({"file": filename, "reason": entry["reason"]})
    on_disk = {p.relative_to(directory).as_posix() for p in directory.rglob("*.ttl")}
    if on_disk != seen:
        raise ValueError(f"unclassified shape files: {sorted(on_disk - seen)}; stale entries: {sorted(seen - on_disk)}")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--shapes-dir", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        value = plan(args.shapes_dir)
    except (OSError, ValueError, TypeError, KeyError) as exc:
        print(f"shape manifest REFUSED: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(value, sort_keys=True) if args.json else
          f"Shape manifest OK: {len(value['load'])} load, {len(value['exclude'])} exclude")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
