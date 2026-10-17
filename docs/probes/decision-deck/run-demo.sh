#!/usr/bin/env bash
set -euo pipefail
base=${1:-0cb06945e5f43104cbd4dd7a6301426ab243bc24}
printf 'Base %s: ' "$base"
if git cat-file -e "$base:src/store/decision_deck.rs" 2>/dev/null; then
    printf 'Decision Deck module present\n'
else
    printf 'Decision Deck module absent\n'
fi
printf 'Candidate: temporary stores and software WebAuthn authenticators only\n'
printf '$ just test --no-default-features --lib store::decision_deck -- --nocapture\n'
diagnostics=$(mktemp)
trap 'rm -f -- "$diagnostics"' EXIT
if ! just test --no-default-features --lib store::decision_deck -- --nocapture 2>"$diagnostics"; then
    cat "$diagnostics" >&2
    exit 1
fi
