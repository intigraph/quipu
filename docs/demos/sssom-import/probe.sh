#!/usr/bin/env bash
set -euo pipefail
: "${BEFORE_QUIPU:?set BEFORE_QUIPU to the base binary}"
: "${AFTER_QUIPU:?set AFTER_QUIPU to the candidate binary}"
task_fixture=$(mktemp -d)
trap 'rm -rf -- "$task_fixture"' EXIT
cd "$task_fixture"
export HOME="$task_fixture" XDG_CONFIG_HOME="$task_fixture/config"
cat > external.tsv <<'TSV'
#mapping_set_id: urn:example:external-set
#curie_map:
#  ex: https://example.org/
subject_id	predicate_id	object_id	mapping_justification	author_id	accepted
ex:incoming	skos:exactMatch	ex:retained	ex:ExternalMatcher	ex:claimed-reviewer	true
TSV
cp external.tsv original.tsv
printf '%s\n' 'BASE (installed binary; exact source revision):'
"$BEFORE_QUIPU" version
printf '%s\n' '$ qp align import external.tsv --out proposals.json --db untouched.db'
if "$BEFORE_QUIPU" align import external.tsv --out proposals.json --db untouched.db; then
 echo 'unexpected base success' >&2; exit 1
else
 task_rc=$?
 [ "$task_rc" -eq 2 ]
fi
printf '\n%s\n' 'CANDIDATE (source fixture; not a deployed service):'
"$AFTER_QUIPU" version
printf '%s\n' '$ qp align import external.tsv --out proposals.json --db untouched.db'
"$AFTER_QUIPU" align import external.tsv --out proposals.json --db untouched.db
python3 - <<'PY'
import json
p=json.load(open('proposals.json'))
r=p['evidence']['rows'][0]
assert p['local_acceptance']=='pending'
assert r['fields']['accepted']=='true'
assert r['justification']=='https://example.org/ExternalMatcher'
print('local_acceptance:',p['local_acceptance'])
print('source accepted field:',r['fields']['accepted'])
print('external justification:',r['justification'])
PY
cmp -s external.tsv original.tsv
test ! -e untouched.db
printf '%s\n' 'original input preserved; database absent'
printf '\n%s\n' '$ qp align import external.tsv --out external.tsv'
if "$AFTER_QUIPU" align import external.tsv --out external.tsv; then
 echo 'unexpected source replacement' >&2; exit 1
fi
cmp -s external.tsv original.tsv
printf '%s\n' 'source replacement refused; original bytes preserved'
