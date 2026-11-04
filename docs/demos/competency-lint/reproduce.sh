#!/usr/bin/env bash
set -eu
base=${QUIPU_BASE_BIN:-quipu}
candidate=${QUIPU_CANDIDATE_BIN:?set QUIPU_CANDIDATE_BIN to the candidate quipu binary}
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
cat > "$fixture/competency.toml" <<'TOML'
schema = "quipu-competency/1"
[[question]]
id = "present"
ask = "Is someone present?"
query = "ASK { ?p a <urn:Person> }"
justifies = ["urn:PersonShape"]
expect = "ask=true"
[[command]]
id = "record-person"
does = "Record a person"
inputs = {person = "iri"}
template = '<{person}> a <urn:Person> .'
satisfies = ["urn:PersonShape"]
write_class = "example-class"
key = ["person"]
example = {person = "urn:alice"}
TOML
printf 'Before: installed CLI\n$ quipu --version\n'
"$base" --version
printf '$ quipu competency lint ./pendant\n'
set +e
"$base" competency lint "$fixture"
rc=$?
set -e
printf 'exit=%s (expected unsupported command)\n\n' "$rc"
[ "$rc" -eq 2 ]
printf 'After: candidate CLI, same fixture\n$ quipu --version\n'
"$candidate" --version
printf '$ quipu competency lint ./pendant\n'
"$candidate" competency lint "$fixture"
printf '\nSabotage: replace ASK with BROKEN\n'
python3 - "$fixture/competency.toml" <<'PY'
import pathlib,sys
p=pathlib.Path(sys.argv[1]);p.write_text(p.read_text().replace('ASK {', 'BROKEN {'))
PY
printf '$ quipu competency lint ./pendant\n'
set +e
"$candidate" competency lint "$fixture"
rc=$?
set -e
printf 'exit=%s (expected invalid query, named present)\n' "$rc"
[ "$rc" -eq 1 ]
printf '\nNo store opened; no query or command executed.\n'
