#!/usr/bin/env bash
# arming: agent-invoked; retained original failure + actual in-memory regression.
set -euo pipefail
: "${TRANSITION_FINITE_TEST_BINARY:?Build the no-default library test artifact from this candidate}"
ROOT=$(git rev-parse --show-toplevel)
cat "$ROOT/docs/demos/transition-trust-root/finite-before.txt"
printf '\nCandidate native control (no live store or key operation):\n'
"$TRANSITION_FINITE_TEST_BINARY" --exact \
  governance::transition::tests::udr9_a_finite_unsigned_root_tier_cannot_authorize_a_persisted_transition \
  --nocapture --color never
