#!/usr/bin/env bash
# release-pr-freshness.sh — verify a release PR AS IT WOULD MERGE onto main now.
#
# WHY (aegis-0zr): the daily proposal refreshed release PR #905 at about 05:40Z.
# #901 and #903 merged to main after that, and #905 then squash-merged on top of
# them as 4fe4d08b. The squash commit's tree contains #901 and #903, but the 0.16.5
# section, written before they existed, lists neither. Checking the release
# branch's own tip cannot see this: the branch does not contain those commits.
#
# This script builds the exact commit GitHub's squash merge would create — the
# merge-tree of <main> and the release branch, as ONE commit whose only parent is
# <main>, with the release subject — and runs verify-changelog.sh against it. A
# release PR that passes here ships a section that covers every packaged commit
# on main at the time of the check.
#
# Usage: scripts/release-pr-freshness.sh <release-branch-ref> [<main-ref>]
#        (<main-ref> defaults to HEAD)
# Exit:  0 = fresh; 1 = stale (sections miss commits, or the merge conflicts);
#        2 = usage/environment error.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

[[ $# -ge 1 && $# -le 2 ]] || { sed -n '2,20p' "$0" | sed 's/^# \?//' >&2; exit 2; }
BR="$1"
MAIN="${2:-HEAD}"
br_sha="$(git rev-parse --verify "${BR}^{commit}")" || exit 2
main_sha="$(git rev-parse --verify "${MAIN}^{commit}")" || exit 2

VERSION="$(git show "${br_sha}:Cargo.toml" \
  | python3 -c 'import sys, tomllib; print(tomllib.loads(sys.stdin.read())["package"]["version"])')" \
  || { echo "ERROR: no package version in ${BR}:Cargo.toml" >&2; exit 2; }

# --write-tree exits 1 on a conflict and prints the conflicted paths. A release
# PR that cannot merge cleanly is stale by definition: it must be regenerated.
if ! tree="$(git merge-tree --write-tree --no-messages "$main_sha" "$br_sha" 2>&1)"; then
  echo "STALE — release PR ${BR} no longer merges cleanly onto ${MAIN} (${main_sha:0:8}):" >&2
  printf '%s\n' "$tree" | sed -n '2,$p' | sed 's/^/  /' >&2
  exit 1
fi
tree="$(printf '%s\n' "$tree" | head -1)"

# The squash merge GitHub will make: one commit, parent = main, the PR title as
# subject. The subject matters: verify-changelog.sh exempts a commit that touches
# only release-mechanics files, which is what the real release commit is.
squash="$(GIT_AUTHOR_NAME=release-pr-freshness GIT_AUTHOR_EMAIL=noreply@example.invalid \
  GIT_COMMITTER_NAME=release-pr-freshness GIT_COMMITTER_EMAIL=noreply@example.invalid \
  git commit-tree "$tree" -p "$main_sha" -m "chore: release v${VERSION}")"

echo "release-pr-freshness: ${BR} (${br_sha:0:8}) squashed onto ${MAIN} (${main_sha:0:8}) as ${squash:0:8}"
rc=0
"$REPO_ROOT/scripts/verify-changelog.sh" --at "$squash" --version "$VERSION" || rc=$?
case "$rc" in
  0) echo "FRESH — the ${VERSION} section covers every packaged commit on ${MAIN}." ;;
  1) echo "STALE — ${MAIN} moved after the release PR was prepared; regenerate it (dispatch release-pr-daily.yml) before merging." >&2 ;;
esac
exit "$rc"
