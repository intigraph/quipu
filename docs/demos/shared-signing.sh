#!/bin/sh
# Two disposable fixed-seed fixtures, never operational signing identities.
set -eu
: "${LEGACY_SIGNING_FIXTURE:?Set the baseline fixture executable}"
: "${SHARED_SIGNING_FIXTURE:?Set the shared-crate fixture executable}"
fixture_dir=$(mktemp -d)
trap 'rm -rf "$fixture_dir"' EXIT HUP INT TERM
printf '%s\n' 'Unchanged Quipu v1 module, disposable seed and public fields:'
"$LEGACY_SIGNING_FIXTURE" > "$fixture_dir/legacy.json"
cat "$fixture_dir/legacy.json"
printf '%s\n' 'Extracted shared crate, identical disposable input:'
"$SHARED_SIGNING_FIXTURE" > "$fixture_dir/shared.json"
cat "$fixture_dir/shared.json"
cmp "$fixture_dir/legacy.json" "$fixture_dir/shared.json"
printf '%s\n' 'PASS: message, evidence hash, public key and signature are byte-identical.'
