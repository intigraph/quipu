#!/usr/bin/env bash
set -euo pipefail
baseline=${NAMESPACE_BASELINE:?set baseline executable}
candidate=${NAMESPACE_CANDIDATE:?set candidate executable}
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
cd "$fixture"
mkdir .bobbin
printf '[quipu]\nbase_ns = "https://example.org/vocab/"\n' > .bobbin/config.toml
"$candidate" namespaces list --db fixture.db > initial.json
jq -e '.namespaces == []' initial.json >/dev/null
cat >> .bobbin/config.toml <<'CONFIG'
[[quipu.namespaces]]
prefix = "demo"
base_iri = "https://example.org/private/"
visibility = "private"
audience = "org:example"
CONFIG
printf '%s\n' '$ qp namespaces apply --db fixture.db'
"$candidate" namespaces apply --db fixture.db | jq '{count,namespaces}'
printf '[quipu]\nbase_ns = "https://example.org/vocab/"\n' > .bobbin/config.toml
"$candidate" namespaces list --db fixture.db > retained.json
jq '{expected_registry_hash:.registry_hash,namespace:.namespaces[0]}' retained.json > request.json
printf '%s\n' '$ baseline: qp namespaces remove request.json --db fixture.db'
if "$baseline" namespaces remove request.json --db fixture.db > before.txt 2>&1; then
    cat before.txt
    exit 1
fi
cat before.txt
printf '%s\n' '$ candidate: qp namespaces remove request.json --db fixture.db'
"$candidate" namespaces remove request.json --db fixture.db | jq '{removed:.removed.prefix,namespaces}'
"$candidate" namespaces list --db fixture.db > after.json
jq -e --slurpfile initial initial.json '.namespaces == $initial[0].namespaces' after.json >/dev/null
printf '%s\n' 'Effective registry equals the initial empty registry.'
printf '%s\n' '$ candidate: retry the stale predecessor receipt'
if "$candidate" namespaces remove request.json --db fixture.db > stale.txt 2>&1; then
    cat stale.txt
    exit 1
fi
cat stale.txt
