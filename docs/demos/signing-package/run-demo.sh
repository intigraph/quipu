#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' '$ default route (existing callers)'
python3 scripts/ci/signing-package.py select
printf '%s\n' '$ shared package without opt-in: expected refusal'
if python3 scripts/ci/signing-package.py select \
  --name quipu-signing --manifest crates/quipu-signing/Cargo.toml; then
  echo 'unexpected admission' >&2
  exit 1
fi
printf '%s\n' '$ explicit preparation-only route'
python3 scripts/ci/signing-package.py select \
  --name quipu-signing --manifest crates/quipu-signing/Cargo.toml \
  --opt-in prepare-quipu-signing-0.1.0
printf '%s\n' 'No registry request, credential, publication or consumer migration.'
