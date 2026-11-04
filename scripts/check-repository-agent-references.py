#!/usr/bin/env python3
"""Shared agent-reference checks for the legacy shell producer.

# arming: library; the shell producer invokes this before indexing and after import.
"""

import argparse
import importlib.util
import json
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--receiver", nargs=2, metavar=("QUIPU", "DATABASE"))
    args = parser.parse_args()
    source = args.source.resolve()
    spec = importlib.util.spec_from_file_location(
        "repository_share", source / "scripts/build-repository-share.py"
    )
    builder = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(builder)
    paths = builder.command(["git", "-C", source, "ls-files", "-z"]).split("\0")
    references = builder.agent_references(source, paths)
    if args.receiver:
        control = next((p for p in ("README.md", "README.rst") if p in paths), None)
        if control is None:
            raise ValueError("repository README positive control missing")
        builder.verify_agent_references(*args.receiver, (control, *references))
    print(json.dumps({"agent_references": list(references),
                      "receiver_checked": args.receiver is not None}))


if __name__ == "__main__":
    main()
