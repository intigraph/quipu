#!/usr/bin/env python3
"""Print real registry values from the controlled private stall-shape test."""

import subprocess

result = subprocess.run(
    [
        "just",
        "test",
        "--no-default-features",
        "--lib",
        "metrics::inflight::tests::stalled_write_shape",
        "--",
        "--nocapture",
    ],
    capture_output=True,
    text=True,
    check=True,
)
rows = [
    line
    for line in (result.stdout + result.stderr).splitlines()
    if line.startswith("stall-shape ")
]
if len(rows) != 8:
    raise SystemExit("expected eight measured control rows; no demonstration available")
print("Before: completed-response metrics do not expose pending writes.")
print("Candidate: actual registry, controlled 35-write fixture and simulated age.")
print("\n".join(rows))
print("No production replay or historical telemetry reconstruction.")
