#!/usr/bin/env bash
set -euo pipefail
"${CARGO:-cargo}" test --locked --offline --lib sparql::triple_alias::tests::alias_snapshot_demo -- --ignored --exact --nocapture --test-threads=1 2>&1 | sed -n '/ALIAS_SNAPSHOT_DEMO/p; /test result:/p; /^error:/p'
