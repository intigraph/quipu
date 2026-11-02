#!/usr/bin/env bash
# arming: agent-invoked; retained original failure + actual in-memory regression.
set -euo pipefail
: "${TRANSITION_FINITE_TEST_BINARY:?Build the no-default library test artifact from this candidate}"
ROOT=$(git rev-parse --show-toplevel)
cat "$ROOT/docs/demos/transition-trust-root/finite-before.txt"
printf '\nCandidate native control (no live store or key operation):\n'
out=$("$TRANSITION_FINITE_TEST_BINARY" --exact \
  governance::transition::tests::udr9_a_finite_unsigned_root_tier_cannot_authorize_a_persisted_transition \
  --nocapture --color never)
printf '%s\n' "$out"
# libtest exits 0 when --exact matches nothing; the control must actually run.
grep -q '^running 1 test$' <<<"$out" && grep -q 'test result: ok. 1 passed' <<<"$out" \
  || { echo "finite-probe: the control did not run exactly once (wrong binary or renamed test)" >&2; exit 1; }
