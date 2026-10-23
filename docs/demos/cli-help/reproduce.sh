#!/usr/bin/env bash
# Demonstrate CLI help using two explicit binaries and disposable local stores.
set -euo pipefail
before=$(realpath "${1:?usage: reproduce.sh BEFORE_BINARY AFTER_BINARY}")
after=$(realpath "${2:?usage: reproduce.sh BEFORE_BINARY AFTER_BINARY}")
probe() {
  local binary=$1 label=$2 expected=$3 scratch actual
  scratch=$(mktemp -d)
  mkdir "$scratch/home" "$scratch/cwd"
  printf '\n%s\n$ quipu shapes --help\n' "$label"
  (cd "$scratch/cwd" && env -i HOME="$scratch/home" PATH=/usr/bin:/bin "$binary" shapes --help)
  if [[ -f "$scratch/cwd/.bobbin/quipu/quipu.db" ]]; then actual=yes; else actual=no; fi
  printf 'database created: %s\n' "$actual"
  rm -rf "$scratch"
  [[ "$actual" == "$expected" ]]
}
probe "$before" BEFORE yes
probe "$after" AFTER no
