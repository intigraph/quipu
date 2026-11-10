#!/usr/bin/env bash
set -euo pipefail
base=c2e3a34979ba4d34144363e01b2350ab623231af
if git cat-file -e "$base:src/client_resolver.rs" 2>/dev/null; then
  echo 'Unexpected baseline resolver consumer'; exit 1
fi
echo "Before $base: resolver artifact/consumer module absent (source check only)."
echo 'After: actual isolated writer, signed resolver evidence and refusal controls.'
build_log=$(mktemp)
trap 'rm -f "$build_log"' EXIT
if ! cargo test --quiet --locked --no-default-features --features shacl --lib client_resolver::tests::observation_tests -- --nocapture --test-threads=1 2>"$build_log"; then
  cat "$build_log"; exit 1
fi
