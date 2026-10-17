#!/usr/bin/env bash
set -euo pipefail
: "${IF3_SERVER:?set IF3_SERVER to the candidate server binary}"
: "${IF3_CLI:?set IF3_CLI to a compatible CLI binary}"
printf '%s\n' 'Retained baseline: legacy server.read_only, installed 8170851c:'
cat docs/probes/pr-demos/read-only-replica/baseline.json
printf '%s\n' 'Candidate: server.replica configuration; disposable DB and fake primary only.'
printf '%s\n' 'python3 scripts/test-replica-server.py "$IF3_SERVER" "$IF3_CLI"'
python3 scripts/test-replica-server.py "$IF3_SERVER" "$IF3_CLI"
