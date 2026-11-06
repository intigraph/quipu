#!/usr/bin/env bash
set -euo pipefail
cargo test --lib store_stream::bootstrap_tests::shared_applier_logical_bootstrap_preserves_stored_ask_dataset_and_history -- --exact --nocapture
