#!/bin/sh
set -eu
cd "$(dirname "$0")/../../.."
printf 'Implementation source: '
git rev-parse HEAD
log=$(mktemp)
trap 'rm -f "$log"' EXIT
cargo run --quiet --features full --example search_context_demo 2>"$log" || { cat "$log"; exit 1; }
