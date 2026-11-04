#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
sample=$(mktemp)
trap 'rm -f "$sample"' EXIT
printf '%s\n' 'Synthetic fixture: 20,000 vectors, 384 dimensions, eight alternating runs.'
printf '%s\n' 'Before: frozen 5d2fe613 scan. After: current SQLite vector_search.'
printf '%s\n' '$ just vector-scan benchmark'
if ! just vector-scan benchmark >"$sample" 2>&1; then
    printf '%s\n' 'Benchmark failed. Run the displayed command for compiler/test diagnostics.'
    exit 1
fi
# Keep the real measurement, equality controls and exit summary readable.
# Build/cache paths and compiler warnings are outside this recording.
sed -n '/^running 1 test/,$p' "$sample"
grep -q '^test result: ok\. 1 passed; 0 failed' "$sample"
