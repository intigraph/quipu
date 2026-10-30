#!/usr/bin/env bash
set -euo pipefail
if [[ -t 1 ]]; then stty -onlcr; fi
baseline=$(mktemp)
trap 'rm -f "$baseline"' EXIT
# Relocate the unmodified baseline verifier body to this same source repository.
git show ad8de357521acdbd9f7a0175a37b7c20d3a21f5c:scripts/verify-changelog.sh | sed '/^REPO_ROOT=/c\REPO_ROOT="$PWD"' > "$baseline"
printf '%s\n' 'Base ad8de357: verify-changelog.sh --version 0.16.6 (same repository and tags)'
set +e
bash "$baseline" --version 0.16.6
rc=$?
set -e
printf 'Observed base exit: %s\n' "$rc"
[[ "$rc" == 1 ]]
printf '%s\n' 'Candidate: just changelog-verify --version 0.16.6'
just changelog-verify --version 0.16.6
printf '%s\n' 'Observed candidate exit: 0; source-only verification, no release writes.'
