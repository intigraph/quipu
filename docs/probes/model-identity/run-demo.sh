#!/usr/bin/env bash
set -euo pipefail
# Preserve exact LF bytes so whitespace hooks do not alter recording/timing.
if [ -t 1 ]; then stty -onlcr; fi
# Build the shacl library tests with cargo test --features shacl --lib --no-run.
# Supply that build's test executable, without installing any binary.
readonly test_bin=${MODEL_IDENTITY_TEST_BIN:?Set MODEL_IDENTITY_TEST_BIN to the built library test executable}
printf '%s\n' 'Synthetic source fixture: unchanged text, Dummy8 -> different16 or different8.' 'Observe queued vectors and current dimensions; no service or model download.' '$MODEL_IDENTITY_TEST_BIN swap_republish_rebuilds_or_surfaces_mismatch --nocapture'
"$test_bin" swap_republish_rebuilds_or_surfaces_mismatch --nocapture
printf '%s\n' 'Control: partial rebuild, provider rollback and raw writes keep mismatched/UNKNOWN provenance observable.' '$MODEL_IDENTITY_TEST_BIN --exact embedding::tests::model_provenance_counts_partial_rebuild_and_raw_overwrite'
"$test_bin" --exact embedding::tests::model_provenance_counts_partial_rebuild_and_raw_overwrite
