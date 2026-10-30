#!/usr/bin/env bash
set -euo pipefail
if [[ -t 1 ]]; then stty -onlcr; fi
candidate=$(mktemp)
result=$(mktemp)
cp scripts/verify-changelog.sh "$candidate"
trap 'cp "$candidate" scripts/verify-changelog.sh; rm -f "$candidate" "$result"' EXIT
printf '%s\n' 'Same disposable Git fixtures and canonical locks, including unrelated dependencies.'
printf '%s\n' 'Base a7f41abd: just changelog-verify-test'
git show a7f41abdc2e7ccef2220ff74fe5c95399dcc13cf:scripts/verify-changelog.sh > scripts/verify-changelog.sh
set +e
just changelog-verify-test > "$result" 2>&1
rc=$?
set -e
grep -E 'binding release|binding exemption|passed,.*failed' "$result"
printf 'Observed base exit: %s\n' "$rc"
[[ "$rc" == 1 ]]
for arm in lock-dependency root-lock-dependency registry-namesake; do
  grep -q "FAIL  binding release $arm remains required" "$result"
done
cp "$candidate" scripts/verify-changelog.sh
printf 'Candidate verifier blob: %s\n' "$(git hash-object scripts/verify-changelog.sh)"
printf '%s\n' 'Candidate: just changelog-verify-test'
just changelog-verify-test > "$result" 2>&1
grep -E 'binding release|binding exemption|passed,.*failed' "$result"
printf '%s\n' 'Observed candidate exit: 0; canonical locks accepted, unrelated entries required.'
printf '%s\n' 'Retained canonical 0.16.6 release: just changelog-verify --version 0.16.6'
just changelog-verify --version 0.16.6
