#!/bin/sh
set -eu
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
# Frozen source before this repair; no server/database and no graph writes.
git show d2a5fb7d26af9d6f9aecea4ec04a604ebfeb19aa:shapes/code-entities.ttl > "$scratch/before.ttl"
echo 'BEFORE: structural projection under the prior contract'
rc=0
quipu validate --shapes "$scratch/before.ttl" --data docs/demos/structural-shape-compat/fixture.ttl || rc=$?
test "$rc" -eq 1
echo 'AFTER: identical projection under the compatible contract'
quipu validate --shapes shapes/code-entities.ttl --data docs/demos/structural-shape-compat/fixture.ttl
echo 'NEGATIVE CONTROLS: owner labels, structural fields, datatype constraints'
QUIPU_SHAPE_NATIVE=1 python3 tests/test_code_structural_views.py
