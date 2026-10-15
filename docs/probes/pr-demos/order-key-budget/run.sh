#!/bin/bash
set -euo pipefail
printf '%s\n' 'Before: candidate94db, retained real independent memory measurement'
cat docs/probes/pr-demos/order-key-budget/before.txt
printf '%s\n' 'After: 76bbbbc8908212d876f87ccba8f5cf2776c27cbd, private in-memory library fixture'
printf '%s\n' '$ cargo test --lib large_literal_keys_refuse -- --nocapture'
cargo test --offline --locked --lib large_literal_keys_refuse -- --nocapture 2>&1 | python3 docs/probes/pr-demos/query-correctness/filter-output.py
