#!/usr/bin/env bash
# Run only in a private source fixture: one source file is patched and restored.
set -euo pipefail
if [[ "${1:-}" != --fixture-copy ]]; then
    echo 'Use --fixture-copy in an isolated checkout; this probe patches source.' >&2
    exit 2
fi
state=src/ownership_handoff_model/state.rs
patch=docs/demos/ownership-handoff-model/retirement-mutant.patch
git diff --quiet -- "$state"
git diff --cached --quiet -- "$state"
logs=${MODEL_DEMO_LOG_DIR:-$(mktemp -d)}
mkdir -p "$logs"
cp "$state" "$logs/state.original.rs"
trap 'cp "$logs/state.original.rs" "$state"' EXIT
printf 'CANDIDATE source %s; synthetic in-memory model\n' "$(git rev-parse HEAD)"
just demo ownership-handoff-model > "$logs/candidate.out" 2> "$logs/candidate.err"
cat "$logs/candidate.out"
git apply --check "$patch"
git apply "$patch"
printf '\nCOUNTERFACTUAL: same source, only retirement prerequisite removed\n'
if just demo ownership-handoff-model > "$logs/mutant.out" 2> "$logs/mutant.err"; then
    status=0
else
    status=$?
fi
cat "$logs/mutant.out"
[[ "$status" == 101 ]]
rg -q 'retirement prerequisite lost' "$logs/mutant.err"
rg -q 'admitted_owners=2' "$logs/mutant.out"
printf 'Counterfactual failed the model assertion (actual exit %s).\n' "$status"
cp "$logs/state.original.rs" "$state"
git diff --quiet -- "$state"
printf '\nRESTORED CANDIDATE (same input)\n'
just demo ownership-handoff-model > "$logs/restored.out" 2> "$logs/restored.err"
cat "$logs/restored.out"
