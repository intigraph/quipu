#!/usr/bin/env bash
# Opt-in private producer: derive labels only from an already-scrubbed share.
# No live service reads and no publication. The output is a separate share.
set -euo pipefail
umask 077
if [[ $# != 5 ]]; then
  echo "usage: $0 <quipu-bin> <repository-share-dir> <source-repo> <new-output-dir> <reviewed-producer-policy.ttl>" >&2
  exit 2
fi
quipu_bin=$(readlink -f "$1")
repository_share=$(readlink -f "$2")
source_repo=$(readlink -f "$3")
output=$4
producer_policy=$(readlink -f "$5")
test -s "$producer_policy"
[[ ! -e "$output" ]] || { echo 'output already exists' >&2; exit 2; }
private=$(mktemp -d)
trap 'rm -rf "$private"' EXIT
python3 - "$repository_share/manifest.json" "$private/identity.json" <<'PY'
import json,re,sys
m=json.load(open(sys.argv[1]))
assert re.fullmatch(r'sha256:[0-9a-f]{64}',m['share_id'])
json.dump({'parent':m['share_id'],'timestamp':m['created_at']},open(sys.argv[2],'w'))
PY
parent=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["parent"])' "$private/identity.json")
timestamp=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["timestamp"])' "$private/identity.json")
db="$private/producer.db"
"$quipu_bin" shapes load repository-share "$repository_share/shapes.ttl" --db "$db"
"$quipu_bin" import "$repository_share" --db "$db" > "$private/import.json"
python3 - "$private/import.json" "$parent" <<'PY'
import json,sys
m=json.load(open(sys.argv[1]))
assert m['share_id']==sys.argv[2] and m['outcome']=='staged' and m['promotion']['eligible']
PY
"$quipu_bin" import promote "$parent" --db "$db"
"$quipu_bin" shapes load derived-groups "$source_repo/shapes/derived-groups.ttl" --db "$db"
"$quipu_bin" shapes load graph-labels "$source_repo/shapes/graph-labels.ttl" --db "$db"
"$quipu_bin" shapes load community-producer-policy "$source_repo/docs/knowledge/publication-policy.shapes.ttl" --db "$db"
"$quipu_bin" knot "$producer_policy" --graph urn:quipu:private-community-policy --db "$db"
"$quipu_bin" project --algorithm louvain --persist --timestamp "$timestamp" --db "$db" > "$private/groups.json"
graph=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["derived_graph"])' "$private/groups.json")
"$quipu_bin" share --graph "$graph" --shapes derived-groups --parent-share "$parent" --turtle --output "$output" --db "$db"
