#!/bin/bash
set -euo pipefail
printf '%s\n' 'Baseline: f22e4634 + regression tests (retained real failures)'
cat docs/probes/pr-demos/query-correctness/before.txt
printf '\n%s\n' 'Candidate implementation: 013538f7 (owned in-memory fixtures)'
printf '%s\n' '$ cargo test --lib f79_iri_order -- --nocapture'
cargo test --offline --locked --lib f79_iri_order -- --nocapture 2>&1 | python3 docs/probes/pr-demos/query-correctness/filter-output.py
printf '%s\n' '$ cargo test --features shacl,onnx,server --bin quipu-server f79_standard_results -- --nocapture'
cargo test --offline --locked --features shacl,onnx,server --bin quipu-server f79_standard_results -- --nocapture 2>&1 | python3 docs/probes/pr-demos/query-correctness/filter-output.py
