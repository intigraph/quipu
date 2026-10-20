#!/usr/bin/env bash
# Synthetic local fixture only. QUIPU_DEMO_BIN selects the candidate CLI.
set -euo pipefail
binary=${QUIPU_DEMO_BIN:?Set QUIPU_DEMO_BIN to the candidate quipu executable}
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
cd "$fixture"
mkdir .bobbin
printf "[[quipu.namespaces]]\nprefix = 'public'\n" > .bobbin/config.toml
printf '%s\n' '$ quipu namespaces list --db fixture.db'
"$binary" namespaces list --db fixture.db
printf '%s\n' '$ quipu namespaces apply --db fixture.db'
"$binary" namespaces apply --db fixture.db
printf '%s\n' '{"namespace":"public","type":"urn:example:Host","label":"Example host"}' > request.json
printf '%s\n' '$ quipu entity-ref mint request.json --db fixture.db'
"$binary" entity-ref mint request.json --db fixture.db | tee minted.json
python3 - <<'PY'
import json
with open('minted.json') as stream:
    minted = json.load(stream)
with open('reference.json', 'w') as stream:
    json.dump({'iri': minted['iri'], 'asOf': '2026-10-20T00:00:00Z'}, stream)
PY
printf '%s\n' '$ quipu entity-ref resolve reference.json --db fixture.db'
"$binary" entity-ref resolve reference.json --db fixture.db
printf '%s\n' '{"namespace":"unknown","type":"urn:example:Host"}' > unknown.json
printf '%s\n' '$ quipu entity-ref mint unknown.json --db fixture.db # expected refusal'
if "$binary" entity-ref mint unknown.json --db fixture.db; then
    printf '%s\n' 'ERROR: unknown namespace unexpectedly accepted' >&2
    exit 1
fi
printf '%s\n' 'Expected refusal verified; no network requests or installed-store writes.'
