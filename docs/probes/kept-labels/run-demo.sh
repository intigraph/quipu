#!/usr/bin/env bash
set -euo pipefail
if [[ -t 1 ]]; then stty -onlcr; fi
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cargo test --features shacl --lib --no-run --message-format=json >"$scratch/build.jsonl" 2>"$scratch/build.log"
test_binary=$(python3 - "$scratch/build.jsonl" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    event = json.loads(line)
    if event.get('reason') == 'compiler-artifact' and event.get('profile', {}).get('test') and event.get('executable'):
        print(event['executable'])
PY
)
"$test_binary" label_followup_ --nocapture
