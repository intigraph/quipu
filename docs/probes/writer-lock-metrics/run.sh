#!/usr/bin/env bash
set -euo pipefail
# Private in-memory native routes only; no server connection or production write.
just test --features full --bin quipu-server writer_timing_native_knot_stage_and_detached_promotion -- --nocapture
