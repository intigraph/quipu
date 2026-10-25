#!/usr/bin/env bash
set -euo pipefail
build_log=$(mktemp)
trap 'rm -f "$build_log"' EXIT
binary=$(just test --locked --features shacl --lib --no-run --message-format=json 2>"$build_log" | python3 -c '
import json, sys
executables = []
for line in sys.stdin:
    record = json.loads(line)
    if record.get("reason") == "compiler-artifact" and record.get("executable"):
        executables.append(record["executable"])
if len(executables) != 1:
    raise SystemExit("expected exactly one library test executable")
print(executables[0])
')
echo 'Synthetic library fixture: an ephemeral Action depends on a project Action.'
echo 'Run the same write with default context and explicit [ephemeral, project] context.'
"$binary" --nocapture explicit_reference_types_preserve_graph_isolation_on_both_paths
