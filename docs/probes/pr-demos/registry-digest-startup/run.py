#!/usr/bin/env python3
"""Show retained failing-first evidence and run the actual qualified controls."""
import os
from pathlib import Path
import subprocess

print("BEFORE: base360f8f49 with the added regression, before the migration fix", flush=True)
print(Path(__file__).with_name("before.txt").read_text(), flush=True)
for key, selected in [
    ("TQA_LIB_TEST", "shape_mapping::backfill_tests"),
    ("TQA_LIB_TEST", "mcp_transport::tests"),
    ("TQA_SERVER_TEST", "background::registry_digests::tests::canonicalization_releases_the_real_writer_mutex"),
]:
    print("AFTER: development/full native test: " + selected, flush=True)
    subprocess.run([os.environ[key], selected], check=True)
print("PASS: no eager history parse; durable bounded progress and refusal controls; bound listener; writer mutex released", flush=True)
