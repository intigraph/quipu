#!/usr/bin/env bash
set -euo pipefail
: "${BEFORE_BIN:?Set BEFORE_BIN to the retained baseline executable}"
: "${AFTER_BIN:?Set AFTER_BIN to the candidate executable}"
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
cd "$fixture"
echo 'BEFORE: retained release 0.16.2 / 4cd9e48d (local fixture; no service)'
echo '$ quipu signer --help'
if "$BEFORE_BIN" signer --help; then echo 'Expected baseline refusal'; exit 1; else echo 'Expected refusal observed'; fi
echo 'AFTER: source candidate over 8fa29473 (working-tree build; no installation)'
echo '$ quipu signer --help'
"$AFTER_BIN" signer --help
echo '$ quipu signer status --db demo.db'
"$AFTER_BIN" signer status --db demo.db
key=$(python3 - <<'PY'
import base64
print(base64.urlsafe_b64encode(bytes([0xa4,1,1,3,0x27,0x20,6,0x21,0x58,0x20])+bytes(range(1,33))).decode().rstrip('='))
PY
)
expiry=$(python3 -c 'import time;print(int(time.time())+86400)')
echo '$ quipu signer enroll <public test credential> --graph https://example.org/answers --db demo.db'
"$AFTER_BIN" signer enroll --credential-id Y3JlZGVudGlhbC0x --cose-key "$key" --scheme webauthn-eddsa --origin https://signer.example.org --rp-id signer.example.org --actor operator@example.org --graph https://example.org/answers --expires-at "$expiry" --db demo.db
echo '$ quipu signer status --db demo.db'
"$AFTER_BIN" signer status --db demo.db
echo '$ quipu signer revoke Y3JlZGVudGlhbC0x --db demo.db'
"$AFTER_BIN" signer revoke Y3JlZGVudGlhbC0x --db demo.db
echo '$ quipu signer status --db demo.db'
"$AFTER_BIN" signer status --db demo.db
echo 'Observed: enrollment and revocation persist; enabled stays false. No graph protection or activation.'
