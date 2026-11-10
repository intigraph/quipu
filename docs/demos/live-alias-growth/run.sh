#!/usr/bin/env bash
set -euo pipefail
: "${BASE_PROBE:?Supply the baseline example binary}"
: "${HEAD_PROBE:?Supply the candidate example binary}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
printf 'Baseline library source: 6411b3ed\n'
"$BASE_PROBE" | tee "$work/base"
printf '\nCandidate library source: 0eea52a7\n'
"$HEAD_PROBE" | tee "$work/head"
for side in base head; do
  grep -qx 'same-iri-different-file-ids=true' "$work/$side"
  grep -qx 'local-id-unchanged=true' "$work/$side"
  grep -qx 'remote-only-unchanged=true' "$work/$side"
done
grep -qx 'late-shared-canonical=false' "$work/base"
grep -qx 'later-local-spelling-canonical=false' "$work/base"
grep -qx 'late-shared-canonical=true' "$work/head"
grep -qx 'later-local-spelling-canonical=true' "$work/head"
