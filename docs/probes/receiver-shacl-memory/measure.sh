#!/usr/bin/env bash
# Measure `quipu validate` peak RSS for a repository share, for the whole export
# and for line prefixes of it, then once per shape group if requested.
# Usage: measure.sh <quipu-binary> <share-dir> [percent ...]
# Needs GNU time (/usr/bin/time). Writes nothing outside a temporary directory.
set -euo pipefail
bin=$1 share=$2; shift 2
percents=("${@:-12 25 50 100}")
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
lines=$(wc -l < "$share/export.nt")
printf 'triples=%s\n' "$lines"
for p in ${percents[@]}; do
  head -n $((lines * p / 100)) "$share/export.nt" > "$work/data.nt"
  /usr/bin/time -f "percent=$p triples=$((lines * p / 100)) rss_kb=%M wall_s=%e" \
    -o "$work/time.txt" "$bin" validate --shapes "$share/shapes.ttl" --data "$work/data.nt" \
    > /dev/null 2>&1 || true   # rc 1 = results present; the peak is still valid
  cat "$work/time.txt" | tail -1
done
printf '@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix ex: <http://example.org/> .\nex:S a sh:NodeShape ; sh:targetClass ex:Nothing .\n' > "$work/trivial.ttl"
/usr/bin/time -f "baseline_load_only rss_kb=%M wall_s=%e" -o "$work/time.txt" \
  "$bin" validate --shapes "$work/trivial.ttl" --data "$share/export.nt" > /dev/null 2>&1 || true
tail -1 "$work/time.txt"
