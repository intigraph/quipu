#!/usr/bin/env bash
set -euo pipefail
: "${BEFORE_TEST_BINARY:?compiled preceding library-test binary required}"
: "${AFTER_TEST_BINARY:?compiled candidate library-test binary required}"
run_case() {
    local label=$1 binary=$2 expected=$3 actual=0 log
    log=$(mktemp)
    printf '%s: synthetic 3,000-fact named graph, combined presence preflight\n' "$label"
    "$binary" named_graph_presence_preflight_work --nocapture --test-threads=1 >"$log" 2>&1 || actual=$?
    awk '/NAMED_PREFLIGHT|test result:/' "$log"
    rm -f -- "$log"
    test "$actual" -eq "$expected"
    printf 'Observed exit %s (expected %s); answer markers and header checked.\n' "$actual" "$expected"
}
run_case BEFORE "$BEFORE_TEST_BINARY" 101
run_case AFTER "$AFTER_TEST_BINARY" 0
