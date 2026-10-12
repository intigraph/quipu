#!/bin/bash
set -euo pipefail
export PATH=/usr/bin:/bin
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
candidate=$(readlink -f "${1:-${DEMO_CANDIDATE:?provide a candidate quipu executable}}")
cp "$candidate" "$fixture/quipu"
cd "$fixture"
printf '$ test -e qp\n'
if test -e qp; then exit 1; else printf 'Before finishing install: qp absent\n'; fi
printf '$ ./quipu install-alias\n'
./quipu install-alias | grep '^Verified '
printf '$ readlink qp\n'
readlink qp
printf '$ test qp -ef quipu\n'
test qp -ef quipu && printf 'same inode: PASS\n'
printf '$ compare qp --version with quipu --version\n'
test "$(./qp --version)" = "$(./quipu --version)" && printf 'same version: PASS\n'
printf '$ ./quipu install-alias (repeat)\n'
./quipu install-alias | grep '^Verified '
printf 'idempotence: PASS\n'
printf 'foreign command\n' > qp-foreign
rm qp
mv qp-foreign qp
printf '$ ./quipu install-alias (foreign qp collision)\n'
if ./quipu install-alias > refusal.txt 2>&1; then exit 1; fi
grep -q 'qp collision' refusal.txt
printf 'collision: refused\n'
test "$(cat qp)" = 'foreign command' && printf 'foreign bytes preserved: PASS\n'
