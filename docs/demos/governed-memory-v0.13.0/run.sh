#!/usr/bin/env bash
set -euo pipefail

# Run from a fresh scratch copy of this directory. This is illustrative data,
# not a record of historical decisions. No server or credentials are needed.
test ! -e memory.db || { printf 'Use a fresh directory: memory.db exists.\n' >&2; exit 2; }
demo_start_epoch=$(date +%s)
demo_seconds=${DEMO_SECONDS:-60}
demo_pause=${DEMO_PAUSE:-4}

show() {
    printf '\n$ '
    printf '%q ' "$@"
    printf '\n'
    "$@"
}
pause() { sleep "$demo_pause"; }
refused() {
    local demo_rc=0
    show "$@" || demo_rc=$?
    test "$demo_rc" -eq 1 || { printf 'Expected refusal, got exit %s\n' "$demo_rc" >&2; exit 2; }
    printf 'Expected refusal (exit 1).\n'
}

printf 'Quipu 0.13.0: governed memory, using the released CLI.\n'
printf 'Illustrative fixtures in the public Quipu repository; isolated local store.\n'
demo_version=$(quipu --version)
printf '%s\n' "$demo_version"
grep -Fq 'git_sha: ec3b707dc2976e579e634f276e752c7715a9ac99' <<< "$demo_version"
grep -Fq 'git_dirty: false' <<< "$demo_version"
pause

show quipu knot baseline.ttl --shapes memory.shapes.ttl --timestamp 2026-01-01T12:00:00Z --db memory.db
show quipu read 'SELECT ?kind WHERE { ?decision <https://example.org/memory/sourceKind> ?kind }' --db memory.db
pause

printf '\nThe shape names the path and the allowed provenance tags.\n'
refused quipu validate --shapes memory.shapes.ttl --data bad.ttl
refused quipu knot bad.ttl --shapes memory.shapes.ttl --timestamp 2026-01-02T12:00:00Z --db memory.db
printf '\nThe refused decision never entered the store:\n'
show quipu read 'ASK { <https://example.org/memory/decision-2> ?p ?o }' --db memory.db
test "$(quipu read 'ASK { <https://example.org/memory/decision-2> ?p ?o }' --db memory.db)" = false
test "$(quipu read 'ASK { <https://example.org/memory/decision-1> ?p ?o }' --db memory.db)" = true
pause

printf '\nCorrect the tag and carry its source:\n'
show quipu knot good.ttl --shapes memory.shapes.ttl --timestamp 2026-01-02T12:00:00Z --db memory.db
show quipu read 'SELECT ?kind ?source WHERE { <https://example.org/memory/decision-2> <https://example.org/memory/sourceKind> ?kind ; <http://www.w3.org/ns/prov#wasDerivedFrom> ?source }' --db memory.db
pause

printf '\nBefore the second decision:\n'
show quipu unravel --valid-at 2026-01-02T00:00:00Z --db memory.db
pause
printf '\nAfter it:\n'
show quipu unravel --valid-at 2026-01-03T00:00:00Z --db memory.db
pause

demo_remaining=$((demo_seconds - $(date +%s) + demo_start_epoch))
if ((demo_remaining > 0)); then sleep "$demo_remaining"; fi
printf '\nRefused writes stay out. Accepted facts retain their valid-time and source.\n'
