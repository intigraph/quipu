#!/usr/bin/env python3
"""Run the public in-memory example and retain only its actual fixture output."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[3]
result = subprocess.run(
    ["cargo", "run", "--quiet", "--no-default-features", "--example", "episode_addresses"],
    cwd=root, capture_output=True, text=True, check=False,
)
if result.returncode:
    sys.stderr.write(result.stderr)
    raise SystemExit(result.returncode)
print("Synthetic in-memory fixture; no server or deployment writes.")
print(result.stdout, end="")
