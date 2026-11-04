#!/usr/bin/env bash
# arming: agent-invoked; runs prebuilt library probes against disposable SQLite files.
set -euo pipefail
: "${STARTUP_BASE_PROBE:?set the baseline probe executable}"
: "${STARTUP_HEAD_PROBE:?set the candidate probe executable}"
for variant in BASE CANDIDATE; do
    if [ "$variant" = BASE ]; then probe="$STARTUP_BASE_PROBE"; else probe="$STARTUP_HEAD_PROBE"; fi
    printf '%s: same disposable SQLite input; no witness or enrollment adapter\n' "$variant"
    output="$("$probe")"
    printf '%s\n' "$output"
    printf '%s\n' "$output" | rg -q '^unenrolled_startup=ADMITTED$'
    printf '%s\n' "$output" | rg -q '^readonly_inspection=AVAILABLE$'
    if [ "$variant" = BASE ]; then expected=ADMITTED; else expected=REFUSED; fi
    printf '%s\n' "$output" | rg -q "^writable_startup=$expected$"
    printf '%s\n' "$output" | rg -q "^explicit_adoption=$expected$"
    if [ "$variant" = CANDIDATE ]; then
        printf '%s\n' "$output" | rg -q '^schema_unchanged=true$'
    fi
    printf '\n'
done
