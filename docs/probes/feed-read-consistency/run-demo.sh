#!/usr/bin/env bash
set -euo pipefail
if [ -t 1 ]; then stty -onlcr; fi
readonly test_bin=${FEED_TEST_BIN:?Set FEED_TEST_BIN to the built library test executable}
printf '%s\n' 'Private SQLite fixtures: deterministic committed write between page statements; missing/decode metadata errors.' 'Also verify real uncommitted writer-savepoint isolation and caller-owned read transaction preservation.' '$FEED_TEST_BIN feed_followup_ --nocapture'
"$test_bin" feed_followup_ --nocapture
