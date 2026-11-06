#!/usr/bin/env bash
set -euo pipefail
stty -onlcr
printf '%s\n' 'model-tests --exact ownership_handoff_model::tests::abandoned_attempt_cannot_activate_its_stale_destination --nocapture'
exec "${OWNERSHIP_MODEL_TEST_BINARY:?Set this to a copied, compiled model test executable}" \
  --exact ownership_handoff_model::tests::abandoned_attempt_cannot_activate_its_stale_destination --nocapture
