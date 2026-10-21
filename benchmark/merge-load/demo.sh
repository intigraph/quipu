#!/usr/bin/env bash
set -euo pipefail
echo 'Native CLI + actual Explorer WASM: 100 logical offline changes per side, seed42.'
echo 'Private fixtures; full-fact ledger oracle, ten seeded conflicts, decisions and replay.'
uv run --no-project --python 3.11 --with rdflib python benchmark/merge-load/run.py \
  --driver "${MERGE_LOAD_DRIVER:?supply the built driver}" \
  --quipu "${MERGE_LOAD_QUIPU:?supply the built CLI}" \
  --wasm-module "${MERGE_LOAD_WASM:?supply actual Node WASM glue}" \
  --sizes 100 --paths native,wasm --seed 42 \
  --out "benchmark/merge-load/out/demo-$(date +%s)"
