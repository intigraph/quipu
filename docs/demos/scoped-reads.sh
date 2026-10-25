#!/usr/bin/env bash
# Private in-memory library demonstration; never starts a listener.
set -uo pipefail
cd "$(dirname "$0")/../.."
printf '$ just demo scoped-reads\n'
build_log=$(mktemp)
trap 'rm -f -- "$build_log"' EXIT
just demo scoped-reads 2>"$build_log"
result=$?
if [ "$result" -ne 0 ]; then
    cat "$build_log" >&2
fi
printf 'Demonstration exit: %s\n' "$result"
exit "$result"
