#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
printf '%s\n' 'Same local Person fixture: Alice age 30 -> 31.' 'BEFORE: installed 99c777; report has no neighborhood.'
"${PENDANT_BASELINE:-quipu}" share diff tests/fixtures/share-review/clean-old tests/fixtures/share-review/clean-new --report --format json | python3 -c 'import json,sys; r=json.load(sys.stdin); print(json.dumps({"summary":r["summary"],"neighborhood_available":"neighborhood" in r}))'
printf '%s\n' 'AFTER: local candidate from the PR implementation; no service deployment.'
"$PENDANT_CANDIDATE" share diff tests/fixtures/share-review/clean-old tests/fixtures/share-review/clean-new --report --format json | python3 -c 'import json,sys; r=json.load(sys.stdin); n=r["neighborhood"]; print(json.dumps({"summary":r["summary"],"touched":n["touched"],"before_context_edges":n["before"]["total_edges"],"after_context_edges":n["after"]["total_edges"],"truncated":n["before"]["truncated"] or n["after"]["truncated"]}))'
printf '%s\n' 'Open graph.html: unchanged adjacent facts remain; Before and After share positions.'

"$PENDANT_CANDIDATE" share diff tests/fixtures/share-review/clean-old tests/fixtures/share-review/clean-new --report --format json > docs/probes/pr-demos/pendant-graph/report.json
python3 scripts/ci/pendant-graph.py docs/probes/pr-demos/pendant-graph/report.json docs/probes/pr-demos/pendant-graph/graph.html
