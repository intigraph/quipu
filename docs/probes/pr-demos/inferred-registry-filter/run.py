#!/usr/bin/env python3
"""Replay actual source-test binaries; the before binary removes only the SQL guard."""
import os
import subprocess

for label, variable, expected in [
    ("BEFORE: SQL leaf exclusion sabotaged", "DEMO_BEFORE", 101),
    ("AFTER: all exclusions present", "DEMO_AFTER", 0),
]:
    print(label, flush=True)
    print(f"$ ${variable} smdlnm_ --nocapture", flush=True)
    result = subprocess.run(
        [os.environ[variable], "smdlnm_", "--nocapture"],
        env={**os.environ, "RUST_BACKTRACE": "0"},
        check=False,
    )
    print(f"exit: {result.returncode} (expected {expected})", flush=True)
    if result.returncode != expected:
        raise SystemExit("unexpected source-test result")
