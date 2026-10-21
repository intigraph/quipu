#!/usr/bin/env bash
set -euo pipefail
: "${BASE_BUNDLE:?point to the base nodejs bundle}"
: "${CANDIDATE_BUNDLE:?point to the candidate nodejs bundle}"
DEMO_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
printf '%s\n' '$ node demo.mjs "$BASE_BUNDLE" --expect-missing'
node "$DEMO_DIR/demo.mjs" "$BASE_BUNDLE" --expect-missing
printf '\n%s\n' '$ node demo.mjs "$CANDIDATE_BUNDLE"'
node "$DEMO_DIR/demo.mjs" "$CANDIDATE_BUNDLE"
