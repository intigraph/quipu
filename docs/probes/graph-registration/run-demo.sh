#!/usr/bin/env bash
set -euo pipefail
if [[ -t 1 ]]; then stty -onlcr; fi
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
run_fixture() {
    local filter=$1
    shift
    cargo test "$@" --no-run --message-format=json >"$scratch/build.jsonl" 2>"$scratch/build.log"
    local test_binary
    test_binary=$(python3 - "$scratch/build.jsonl" <<'PY'
import json, sys
executables = []
for line in open(sys.argv[1]):
    event = json.loads(line)
    if event.get('reason') == 'compiler-artifact' and event.get('profile', {}).get('test') and event.get('executable'):
        executables.append(event['executable'])
assert len(executables) == 1, 'expected exactly one fixture executable'
print(executables[0])
PY
)
    "$test_binary" "$filter" --nocapture
}
run_fixture graph_registry_repair --features shacl --lib
run_fixture registry_followup_ --features shacl --test graph_registry_repair
run_fixture registry_followup_ --features shacl,onnx,server --bin quipu-server
