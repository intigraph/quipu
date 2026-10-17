#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 ]] || { echo 'usage: run-demo.sh <base-quipu> <candidate-quipu>' >&2; exit 2; }
base=$1
candidate=$2
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cat > "$work/fixture.ttl" <<'TTL'
@prefix ex: <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
ex:a1 ex:link ex:a2, ex:a3; rdfs:label "Alpha". ex:a2 ex:link ex:a3.
ex:b1 ex:link ex:b2, ex:b3; rdfs:label "Beta". ex:b2 ex:link ex:b3.
TTL
for mode in base candidate; do
  binary=$base
  [[ $mode == candidate ]] && binary=$candidate
  echo "$mode version:"
  "$binary" --version
  db="$work/$mode.db"
  "$binary" knot "$work/fixture.ttl" --db "$db" >/dev/null
  "$binary" project --algorithm louvain --predicate http://example.org/link --db "$db" > "$work/first.json"
  "$binary" project --algorithm louvain --predicate http://example.org/link --db "$db" > "$work/repeat.json"
  "$binary" retract http://example.org/a1 --predicate http://example.org/link --db "$db" >/dev/null
  "$binary" retract http://example.org/a2 --predicate http://example.org/link --db "$db" >/dev/null
  "$binary" project --algorithm louvain --predicate http://example.org/link --db "$db" > "$work/after.json"
  python3 - "$mode" "$work" <<'PY'
import json,sys,pathlib
mode,root=sys.argv[1:];root=pathlib.Path(root)
a,b,c=[json.loads((root/name).read_text()) for name in ['first.json','repeat.json','after.json']]
def beta(result):
    return next(g['community'] for g in result['communities'] if 'http://example.org/b1' in g['entities'])
print(json.dumps({'mode':mode,'communities':len(a['communities']),'labels':[g.get('label') for g in a['communities']], 'unchanged_repeat':a['communities']==b['communities'], 'beta_identity_retained':beta(a)==beta(c)},indent=2))
PY
done
