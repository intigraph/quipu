#!/usr/bin/env python3
"""Print real, allowlisted observation fields from isolated native HTTP tests."""

import json
import subprocess


def main():
    command = [
        "cargo", "test", "--offline", "--locked", "--features", "full",
        "--bin", "quipu-server", "query_usage::tests::http_query_",
        "--", "--nocapture", "--test-threads=1",
    ]
    print("Synthetic in-memory HTTP fixture; no listening socket or live service.", flush=True)
    print("$ " + " ".join(command), flush=True)
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if result.returncode:
        raise SystemExit("Native fixture failed; no success recording produced.")
    records = []
    for line in result.stdout.splitlines():
        position = line.find("{")
        if position < 0:
            continue
        try:
            record = json.loads(line[position:])
        except json.JSONDecodeError:
            continue
        if record.get("event") not in ("query_observation_start", "query_observation_end"):
            continue
        visible = {key: record[key] for key in ("event", "dispatch_ms", "worker_outcome")}
        visible["admission_ms_present"] = "admission_ms" in record
        if "admission_ms" in record:
            visible["admission_ms"] = record["admission_ms"]
        records.append(visible)
        print(json.dumps(visible, sort_keys=True), flush=True)
    assert len(records) == 4, "two lifecycle records for each configuration"
    assert sum(record["admission_ms_present"] for record in records) == 2
    enabled_end = next(record for record in records
                       if record["admission_ms_present"] and record["event"].endswith("_end"))
    assert 0 <= enabled_end["admission_ms"] <= enabled_end["dispatch_ms"]
    for enabled in ("false", "true"):
        assert f"PRIVATE_HTTP_CONTROL status=200 count=1 admission_logging={enabled}" in result.stdout
    print("PASS: both modes return HTTP 200 / one row; disabled omits the field, enabled observes acquisition.")


if __name__ == "__main__":
    main()
