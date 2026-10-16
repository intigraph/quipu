"""Display retained private producer records; makes no network calls."""

import json
from pathlib import Path
import sys

if sys.stdout.isatty():
    import termios

    attributes = termios.tcgetattr(sys.stdout.fileno())
    attributes[1] &= ~termios.ONLCR
    termios.tcsetattr(sys.stdout.fileno(), termios.TCSANOW, attributes)

capture = json.loads(Path(__file__).with_name("records.json").read_text())
print("BEFORE (source inspection):", capture["baseline"])
print("AFTER:", capture["origin"])
for record in capture["records"]:
    identity = f"pid={record['server_process_id']} request={record['request_id']}"
    print(record["event"], identity)
    if record["event"] == "query_observation_end":
        print("  query SHA-256:", record["query_sha256"])
        print("  input SHA-256:", record["input_sha256"])
        print(
            "  dispatch/wait/held ms:",
            record["dispatch_ms"],
            record["store_wait_ms"],
            record["store_held_ms"],
        )
    if record["event"] == "request_complete":
        print("  status/result rows:", record["status"], record["result_size"])
print("Private producer control only; production cause and recovery are unproven.")
