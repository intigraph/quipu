#!/usr/bin/env python3
"""Replay the recorded isolated history response; never contact a service."""
import json
from pathlib import Path


root = Path(__file__).resolve().parent
record = json.loads((root / "response.json").read_text())
assert record["legacy_root_response"]["count"] == 0
assert record["qualified_response"]["count"] == 2
assert record["qualified_response"]["history"][0]["authenticated"] is None
assert record["qualified_response"]["history"][0]["declared"]["host"] == "unit-host"
print(json.dumps(record, indent=2))
