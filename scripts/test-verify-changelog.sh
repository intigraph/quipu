#!/usr/bin/env bash
# test-verify-changelog.sh — prove verify-changelog.sh fails in BOTH directions.
#
# WHY THIS EXISTS: the guard shipped checking only one direction. It computed
# `missing = expected - actual` and returned OK whenever nothing was missing — so a
# CHANGELOG section holding 221 commits when git-cliff attributed 1 passed clean,
# printing the discrepancy on its own summary line before saying OK. That is the
# shape the bug actually takes here (full-history dump for an empty release: v0.3.5,
# #42/v0.3.7, #59/v0.3.17), so the guard was blind to its own reason for existing
# while looking green.
#
# A guard is not tested until a case that SHOULD fail it does. Each case below
# asserts an exit code and a substring, so "the check ran and found nothing" can
# never be confused with "the check cannot find anything".
#
# Hermetic: builds a throwaway git repo per case: it asserts nothing about quipu's
# own history, so fixing a real CHANGELOG section can never silently defuse it.
#
# Usage: scripts/test-verify-changelog.sh
# Exit:  0 = all cases behaved as specified; 1 = at least one did not.
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v git-cliff >/dev/null 2>&1 || { echo "ERROR: git-cliff not installed" >&2; exit 2; }

pass=0; fail=0

# Build a scratch crate: one released commit (v1.0.0), then two releasable commits.
# Echoes "<tmpdir> <hashA> <hashB> <hashC>".
make_repo() {
  local d; d="$(mktemp -d)"
  (
    cd "$d"
    git init -q .
    git config user.email t@example.com
    git config user.name t
    mkdir -p scripts src docs
    cp "$REPO_ROOT/cliff.toml" .
    cp "$REPO_ROOT/scripts/verify-changelog.sh" scripts/
    cp "$REPO_ROOT/scripts/fix-changelog.sh" scripts/
    cp "$REPO_ROOT/scripts/filter-packaged-commits.py" scripts/
    cp "$REPO_ROOT/scripts/check-release-window.py" scripts/
    cp "$REPO_ROOT/scripts/changelog-duplicates.py" scripts/
    cp "$REPO_ROOT/scripts/release-pr-freshness.sh" scripts/
    printf '[package]\nname="probe"\nversion="1.0.0"\nedition="2021"\nexclude=["docs/"]\n' > Cargo.toml
    echo a > src/lib.rs; git add -A; git commit -qm "feat: the released thing"
    git tag v1.0.0
    echo b > src/lib.rs; git commit -qam "feat: the second thing"
    echo c > src/lib.rs; git commit -qam "fix: the third thing"
  ) >/dev/null 2>&1
  local a b c
  a="$(git -C "$d" rev-parse --short=7 HEAD~2)"
  b="$(git -C "$d" rev-parse --short=7 HEAD~1)"
  c="$(git -C "$d" rev-parse --short=7 HEAD)"
  echo "$d $a $b $c"
}

# entry <hash> -> one CHANGELOG bullet in the repo's rendered shape
entry() { echo "- Some change([$1](https://example.invalid/commit/$1))"; }

# check <name> <expected-exit> <expected-substring> <tmpdir>
check() {
  local name="$1" want_rc="$2" want_txt="$3" d="$4" out rc
  out="$(cd "$d" && ./scripts/verify-changelog.sh 2>&1)"; rc=$?
  if [[ "$rc" == "$want_rc" ]] && grep -qF -- "$want_txt" <<<"$out"; then
    echo "  PASS  $name (exit $rc)"
    pass=$((pass + 1))
  else
    echo "  FAIL  $name — wanted exit $want_rc containing '$want_txt', got exit $rc:" >&2
    sed 's/^/          /' <<<"$out" >&2
    fail=$((fail + 1))
  fi
  rm -rf "$d"
}

echo "verify-changelog.sh — both-direction guard tests"

# 1. CONTROL. The section matches git-cliff exactly. Must pass, or every release
#    blocks on a defect that is not there.
read -r d a b c <<<"$(make_repo)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "exact match passes" 0 "none missing, none extra" "$d"

# 2. UNDER-DOCUMENTED (the direction the guard already had). c is omitted.
read -r d a b c <<<"$(make_repo)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; } > "$d/CHANGELOG.md"
check "missing commit fails" 1 "is missing commits" "$d"

# 3. OVER-DOCUMENTED — THE REGRESSION THIS FILE EXISTS FOR. Commit a belongs to
#    v1.0.0 and is documented again under 1.1.0. The one-directional guard
#    returned 0 here.
read -r d a b c <<<"$(make_repo)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$a"; entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "extra commit fails" 1 "do not belong to this release" "$d"

# 4. THE #59 SHAPE: nothing releasable, and the section dumps all history anyway.
#    Reproduces the empty-delta full-history dump end to end.
read -r d a b c <<<"$(make_repo)"
git -C "$d" tag v1.1.0                     # everything through c is already released
{ echo "# Changelog"; echo; echo "## [1.2.0] - 2026-01-01"; echo;
  entry "$a"; entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "empty-delta full-history dump fails" 1 "nothing to release" "$d"

# 5. git-cliff renders a literal [0000000] for entries it cannot attribute (v0.3.11
#    has one). It is changelog noise, not a misfiled commit — it must not fail a
#    release for a defect that does not exist.
read -r d a b c <<<"$(make_repo)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; entry "0000000"; } > "$d/CHANGELOG.md"
check "unresolvable placeholder hash is ignored" 0 "none missing, none extra" "$d"

# 6. A substantive commit that ALSO edits CHANGELOG.md is still release CONTENT and
#    must still be documented. The exemption used to be "touches CHANGELOG.md",
#    which let exactly this commit stop being required — seen live on #60.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo x >> src/lib.rs
  printf '# Changelog\n\nnote\n' > CHANGELOG.md
  git add -A && git commit -qm "fix: real work that also edits the changelog"
) >/dev/null 2>&1
e="$(git -C "$d" rev-parse --short=7 HEAD)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"     # e deliberately omitted
check "substantive commit touching CHANGELOG.md is still required" 1 "$e" "$d"

# 7. ...but a pure release-mechanics commit (CHANGELOG/Cargo only) is still exempt,
#    or every release would fail on release-plz's own commit.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  printf '# Changelog\n\nnote\n' > CHANGELOG.md
  sed -i 's/version="1.0.0"/version="1.1.0"/' Cargo.toml
  git add -A && git commit -qm "chore: release v1.1.0"
) >/dev/null 2>&1
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "pure release-mechanics commit stays exempt" 0 "none missing, none extra" "$d"

# 7b. aegis-0zr: since #902 the release commit also syncs the Python binding's
#     manifest and lock. Those are release mechanics too; without them in the
#     exemption every release commit failed its own verify.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  mkdir -p bindings/python
  printf '[package]\nname="probe-py"\nversion="1.0.0"\n' > bindings/python/Cargo.toml
  echo lock > bindings/python/Cargo.lock
  git add -A && git commit -qm "chore: binding scaffold" && git tag quipu-ai-v1.0.0
  printf '# Changelog\n\nnote\n' > CHANGELOG.md
  sed -i 's/version="1.0.0"/version="1.1.0"/' Cargo.toml bindings/python/Cargo.toml
  echo lock2 > bindings/python/Cargo.lock
  git add -A && git commit -qm "chore: release v1.1.0"
) >/dev/null 2>&1
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo; } > "$d/CHANGELOG.md"
check "release commit syncing the Python binding stays exempt" 0 "none missing, none extra" "$d"

# 8. The post-rename package-qualified tag is the preferred baseline. A newer
# historical v-tag must not pull the range back across the rename boundary.
read -r d a b c <<<"$(make_repo)"
git -C "$d" tag quipu-ai-v1.0.0 HEAD~2
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "package-qualified baseline is accepted" 0 "range quipu-ai-v1.0.0..HEAD" "$d"

# The fixer's truncation guard must translate the package-qualified tag back to
# the plain version heading used by CHANGELOG.md.
read -r d a b c <<<"$(make_repo)"
git -C "$d" tag quipu-ai-v1.0.0 HEAD~2
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; echo; echo "## [1.0.0] - 2025-12-01"; echo;
  entry "$a"; } > "$d/CHANGELOG.md"
fix_out="$(cd "$d" && ./scripts/fix-changelog.sh --check 2>&1)"; fix_rc=$?
if [[ "$fix_rc" -eq 1 ]] && grep -qF "WOULD REWRITE" <<<"$fix_out"; then
  echo "  PASS  fixer accepts package-qualified previous tag (exit $fix_rc)"
  pass=$((pass + 1))
else
  echo "  FAIL  fixer rejected package-qualified previous tag:" >&2
  sed 's/^/          /' <<<"$fix_out" >&2
  fail=$((fail + 1))
fi
rm -rf "$d"

# 9. An excluded-path-only commit does not change the packaged crate. It must not
#    be required by the guard when release-plz correctly omits it.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo note > docs/ci.md
  git add -A && git commit -qm "docs(ci): excluded documentation"
) >/dev/null 2>&1
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "excluded-only commit is not required" 0 "none missing, none extra" "$d"

# 10. The inverse is equally important: documenting that excluded-only commit is
#    over-documentation and must fail, preserving the guard's two-way contract.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo note > docs/ci.md
  git add -A && git commit -qm "docs(ci): excluded documentation"
) >/dev/null 2>&1
e="$(git -C "$d" rev-parse --short=7 HEAD)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; entry "$e"; } > "$d/CHANGELOG.md"
check "documented excluded-only commit fails as extra" 1 "do not belong to this release" "$d"

# 11. The generator shares the same boundary: it removes an excluded commit from
#     a generated section while retaining both packaged commits.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo note > docs/ci.md
  git add -A && git commit -qm "docs(ci): excluded documentation"
) >/dev/null 2>&1
e="$(git -C "$d" rev-parse --short=7 HEAD)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; entry "$e"; echo; echo "## [1.0.0] - 2025-12-01"; echo;
  entry "$a"; } > "$d/CHANGELOG.md"
fix_out="$(cd "$d" && ./scripts/fix-changelog.sh 2>&1)"; fix_rc=$?
if [[ "$fix_rc" -eq 0 ]] \
   && grep -qF "[$b]" "$d/CHANGELOG.md" \
   && grep -qF "[$c]" "$d/CHANGELOG.md" \
   && ! grep -qF "[$e]" "$d/CHANGELOG.md"; then
  echo "  PASS  fixer removes excluded-only commit (exit $fix_rc)"
  pass=$((pass + 1))
else
  echo "  FAIL  fixer did not preserve packaged hashes while dropping $e:" >&2
  sed 's/^/          /' <<<"$fix_out" >&2
  fail=$((fail + 1))
fi
rm -rf "$d"

# The raw window must catch an entry absent from BOTH cliff and the changelog.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo bare >> src/lib.rs
  git commit -qam "Support a feature with no prefix"
) >/dev/null 2>&1
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; } > "$d/CHANGELOG.md"
check "bare subject omitted by both tools fails" 1 "nonconventional commits omitted" "$d"

read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo bare >> src/lib.rs
  git commit -qam "Support a feature with no prefix"
) >/dev/null 2>&1
e="$(git -C "$d" rev-parse --short=7 HEAD)"
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; entry "$e"; } > "$d/CHANGELOG.md"
check "explicit entry repairs immutable bare subject" 0 "none missing, none extra" "$d"

# #202 is a squash commit with a multiline body. The exact subject correction
# must match its FIRST LINE, then survive the automatic fixer's regeneration.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo rotation >> src/lib.rs
  git commit -qam $'Support bearer rotation with a restart-safe grace deadline (#202)\n\n* feat(auth): bounded grace\n\n* fix(auth): restart deadline'
) >/dev/null 2>&1
e="$(git -C "$d" rev-parse --short=7 HEAD)"
git -C "$d" tag quipu-ai-v1.0.0 HEAD~3
{ echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo;
  entry "$b"; entry "$c"; echo; echo "## [1.0.0] - 2025-12-01"; echo;
  entry "$a"; } > "$d/CHANGELOG.md"
fix_out="$(cd "$d" && ./scripts/fix-changelog.sh 2>&1)"; fix_rc=$?
if [[ "$fix_rc" -eq 0 ]] && grep -qF "[$e]" "$d/CHANGELOG.md"; then
  echo "  PASS  regeneration retains corrected multiline squash subject"
  pass=$((pass + 1))
else
  echo "  FAIL  regeneration dropped corrected squash subject: $fix_out" >&2
  fail=$((fail + 1))
fi
window_out="$(cd "$d" && python3 scripts/check-release-window.py 2>&1)"; window_rc=$?
if [[ "$window_rc" -eq 0 ]] && grep -qF "1 bare subjects explicitly documented" <<<"$window_out"; then
  echo "  PASS  pre-release guard uses package-qualified window"
  pass=$((pass + 1))
else
  echo "  FAIL  pre-release window guard: $window_out" >&2
  fail=$((fail + 1))
fi
check "regenerated bare-subject correction verifies" 0 "none missing, none extra" "$d"

# Pending coverage must use the latest release as its base and survive the
# generator even when a repaired bare commit changes excluded documentation.
read -r d a b c <<<"$(make_repo)"
(
  cd "$d"
  echo recipe >> src/lib.rs
  git commit -qam "Warn when full text backups omit vectors without a recipe"
  echo refusal >> src/lib.rs
  git commit -qam "Refuse incomplete text backup recipes unless explicitly waived"
  echo ledger > docs/ledger.json
  git add docs/ledger.json
  git commit -qm "Refresh conformance ledgers for the pack recipe guard"
  git tag quipu-ai-v1.0.0 HEAD~5
) >/dev/null 2>&1
hashes="$(git -C "$d" log -3 --format=%h --abbrev=7)"
{ echo "# Changelog"; echo; echo "## [Unreleased]"; echo;
  entry "$b"; entry "$c";
  for h in $hashes; do entry "$h"; done
  echo; echo "## [1.0.0] - 2025-12-01"; echo; entry "$a";
} > "$d/CHANGELOG.md"
fix_out="$(cd "$d" && ./scripts/fix-changelog.sh 2>&1)"; fix_rc=$?
retained=0
for h in $hashes; do
  if grep -qF "[$h]" "$d/CHANGELOG.md"; then retained=$((retained + 1)); fi
done
if [[ "$fix_rc" -eq 0 && "$retained" -eq 3 ]] \
   && grep -q '^## \[Unreleased\]$' "$d/CHANGELOG.md"; then
  echo "  PASS  pending bare-subject repairs survive regeneration, including excluded docs"
  pass=$((pass + 1))
else
  echo "  FAIL  pending repairs: fixer exit $fix_rc, retained $retained/3: $fix_out" >&2
  fail=$((fail + 1))
fi
check "unreleased verifies against the latest ancestor release" 0 "none missing, none extra" "$d"

# --- DUPLICATION (aegis-fbekec) ---------------------------------------------
# quipu 0.7.0 shipped 55 bullets of 34 distinct — 21 duplicated, every category
# heading twice — to the release page and crates.io with this guard exiting 0.
# Membership (missing/extra) cannot count, so no refinement of the existing
# comparison reaches this; it needs its own check and its own cases.
read -r d a b c < <(make_repo)
{ echo "# Changelog"; echo; echo "## [Unreleased]"; echo;
  echo "### Added"; echo; entry "$b"; entry "$c";
  echo; echo "### Added"; echo; entry "$b"; entry "$c";
  echo; echo "## [1.0.0] - 2025-12-01"; echo; entry "$a";
} > "$d/CHANGELOG.md"
check "duplicated bullets fail even though nothing is missing or extra" \
  1 "documents the same entries more than once" "$d"

# The 0.7.0 shape exactly: the duplication is NOT in the first `## [` heading.
# Both tools resolve "newest section" as that first heading, so a duplicate check
# scoped to it would certify the section ABOVE the broken one — which is what
# actually happened. This case fails if anyone re-scopes the check.
read -r d a b c < <(make_repo)
{ echo "# Changelog"; echo; echo "## [Unreleased]"; echo; entry "$c";
  echo; echo "## [1.0.0] - 2025-12-01"; echo;
  echo "### Added"; echo; entry "$a"; entry "$a";
} > "$d/CHANGELOG.md"
check "duplication BELOW a populated Unreleased is still caught" \
  1 "the [1.0.0] section documents the same entries more than once" "$d"

# NEGATIVE CONTROL: a populated Unreleased above a release is legitimate here and
# must stay passing. Without this arm the guard above could be "fixed" into
# rejecting the repo's normal between-releases state.
read -r d a b c < <(make_repo)
{ echo "# Changelog"; echo; echo "## [Unreleased]"; echo; entry "$b"; entry "$c";
  echo; echo "## [1.0.0] - 2025-12-01"; echo; entry "$a";
} > "$d/CHANGELOG.md"
check "populated Unreleased above a release still verifies" \
  0 "none missing, none extra" "$d"

# The corrector must REFUSE to leave a duplicated file behind, not just the
# verifier refuse to pass one.
read -r d a b c < <(make_repo)
{ echo "# Changelog"; echo; echo "## [Unreleased]"; echo; entry "$b"; entry "$c";
  echo; echo "## [1.0.0] - 2025-12-01"; echo;
  echo "### Added"; echo; entry "$a"; entry "$a";
} > "$d/CHANGELOG.md"
fix_out="$(cd "$d" && ./scripts/fix-changelog.sh 2>&1)"; fix_rc=$?
if [[ "$fix_rc" -ne 0 ]] && grep -qF "duplicated" <<<"$fix_out"; then
  echo "  PASS  fixer refuses to write a duplicated changelog (exit $fix_rc)"
  pass=$((pass + 1))
else
  echo "  FAIL  fixer accepted duplication: exit $fix_rc: $fix_out" >&2
  fail=$((fail + 1))
fi
rm -rf "$d"

# A release PR may carry an empty or populated Unreleased section above the
# pending release. Explicit version selection must repair THAT artifact, preserve
# its neighbours byte-for-byte, retain the date, and converge on a second pass.
for pending in empty populated; do
  read -r d a b c < <(make_repo)
  { echo "# Changelog"; echo; echo "## [Unreleased]"; echo;
    [[ "$pending" == empty ]] || entry "$b";
    echo; echo "## [1.1.0] - 2026-01-01"; echo;
    echo "### Added"; entry "$b"; entry "$b";
    echo; echo "## [1.0.0] - 2025-12-01"; echo; entry "$a";
  } > "$d/CHANGELOG.md"
  cp "$d/CHANGELOG.md" "$d/before.md"
  before="$(cd "$d" && ./scripts/verify-changelog.sh --version 1.1.0 2>&1)"; before_rc=$?
  fix_out="$(cd "$d" && ./scripts/fix-changelog.sh --version 1.1.0 2>&1)"; fix_rc=$?
  verify_out="$(cd "$d" && ./scripts/verify-changelog.sh --version 1.1.0 2>&1)"; verify_rc=$?
  stable_out="$(cd "$d" && ./scripts/fix-changelog.sh --check --version 1.1.0 2>&1)"; stable_rc=$?
  python3 - "$d" <<'PYTEST'
from pathlib import Path
import sys
p = Path(sys.argv[1])
before, after = (p / "before.md").read_text(), (p / "CHANGELOG.md").read_text()
assert before.split("## [1.1.0]")[0] == after.split("## [1.1.0]")[0]
assert before.split("## [1.0.0]")[1] == after.split("## [1.0.0]")[1]
assert "## [1.1.0] - 2026-01-01" in after
PYTEST
  neighbours_rc=$?
  if [[ "$before_rc" -ne 0 && "$fix_rc" -eq 0 && "$verify_rc" -eq 0 && "$stable_rc" -eq 0 && "$neighbours_rc" -eq 0 ]]; then
    echo "  PASS  explicit pending release below $pending Unreleased: repaired, verified, stable"
    pass=$((pass + 1))
  else
    echo "  FAIL  explicit $pending: before=$before_rc fix=$fix_rc verify=$verify_rc stable=$stable_rc neighbours=$neighbours_rc" >&2
    printf '%s\n' "$fix_out" "$verify_out" "$stable_out" >&2
    fail=$((fail + 1))
  fi
  # Default selection skips only the empty placeholder. --at must select the
  # named release from the referenced file, not the current working copy.
  if [[ "$pending" == empty ]]; then
    (cd "$d" && ./scripts/verify-changelog.sh >/dev/null 2>&1); default_rc=$?
    (cd "$d" && ./scripts/fix-changelog.sh --check >/dev/null 2>&1); default_fix=$?
    if [[ "$default_rc" -eq 0 && "$default_fix" -eq 0 ]]; then
      echo "  PASS  default skips empty Unreleased in both tools"
      pass=$((pass + 1))
    else
      echo "  FAIL  empty placeholder default: verify=$default_rc fix=$default_fix" >&2
      fail=$((fail + 1))
    fi
  fi
  (cd "$d" && git add CHANGELOG.md && git commit -qm "chore: release v1.1.0")
  printf 'invalid working copy\n' > "$d/CHANGELOG.md"
  (cd "$d" && ./scripts/verify-changelog.sh --at HEAD --version 1.1.0 >/dev/null 2>&1); at_rc=$?
  git -C "$d" show HEAD:CHANGELOG.md > "$d/CHANGELOG.md"
  if [[ "$at_rc" -eq 0 ]]; then
    echo "  PASS  explicit release from --at ignores working copy ($pending)"
    pass=$((pass + 1))
  else
    echo "  FAIL  explicit release from --at: $at_rc" >&2
    fail=$((fail + 1))
  fi
  # An absent target must fail without rewriting anything, in both tools.
  cp "$d/CHANGELOG.md" "$d/after.md"
  (cd "$d" && ./scripts/fix-changelog.sh --version 9.9.9 >/dev/null 2>&1); bad_fix=$?
  (cd "$d" && ./scripts/verify-changelog.sh --version 9.9.9 >/dev/null 2>&1); bad_verify=$?
  if [[ "$bad_fix" -eq 2 && "$bad_verify" -eq 2 ]] && cmp -s "$d/after.md" "$d/CHANGELOG.md"; then
    echo "  PASS  absent explicit release refuses without writing ($pending)"
    pass=$((pass + 1))
  else
    echo "  FAIL  absent explicit release: fix=$bad_fix verify=$bad_verify" >&2
    fail=$((fail + 1))
  fi
  rm -rf "$d"
done

# Execute the actual main workflow step. A populated Unreleased draft can lag
# behind main; the package's released section must retain its tagged boundary.
# workflow_step <exact step name, or "* on main"> — print that release.yml step's run body.
workflow_step() {
  python3 - "$REPO_ROOT/.github/workflows/release.yml" "$1" <<'PYTEST'
from pathlib import Path
import sys
lines = Path(sys.argv[1]).read_text().splitlines()
want = sys.argv[2]
def match(line):
    if not line.startswith("      - name:"):
        return False
    name = line.split("name:", 1)[1].strip()
    return name.endswith(" on main") if want == "* on main" else name == want
starts = [i for i, line in enumerate(lines) if match(line)]
assert len(starts) == 1, f"expected one workflow step named {want!r}"
start = starts[0]
end = next((i for i in range(start + 1, len(lines))
            if lines[i].startswith("      - ")), len(lines))
run = next(i for i in range(start, end) if lines[i].startswith("        run:"))
value = lines[run].split("run:", 1)[1].strip()
if value == "|":
    body = []
    for line in lines[run + 1:end]:
        if line and not line.startswith("          "):
            break
        body.append(line[10:])
    print("\n".join(body))
else:
    print(value)
PYTEST
}
main_step="$(workflow_step '* on main')" || exit 1
cut_step="$(workflow_step 'Refuse to cut a version whose section misses packaged commits')" || exit 1
read -r d a b c < <(make_repo)
{
  echo '# Changelog'; echo; echo '## [Unreleased]'; echo; entry "$b"
  echo; echo '## [1.1.0] - 2026-01-01'; echo; entry "$b"; entry "$c"
  echo; echo '## [1.0.0] - 2025-12-01'; echo; entry "$a"
} > "$d/CHANGELOG.md"
(
  cd "$d"
  sed -i 's/version="1.0.0"/version="1.1.0"/' Cargo.toml
  git add Cargo.toml CHANGELOG.md; git commit -qm 'chore: release v1.1.0'
  git tag quipu-ai-v1.1.0
  echo fourth >> src/lib.rs; git commit -qam 'feat: fourth change after release'
) || exit 1
fourth="$(git -C "$d" rev-parse --short=7 HEAD)"
for arm in stale-draft missing-released missing-pending complete-pending; do
  case "$arm" in
    stale-draft) want=0; version=1.1.0 ;;
    missing-released)
      sed -i "/\[$c\]/d" "$d/CHANGELOG.md"
      want=1; version=1.1.0 ;;
    missing-pending)
      git -C "$d" show quipu-ai-v1.1.0:CHANGELOG.md > "$d/CHANGELOG.md"
      sed -i 's/version="1.1.0"/version="1.2.0"/' "$d/Cargo.toml"
      sed -i '/^## \[1.1.0\]/i ## [1.2.0] - 2026-01-02\n' "$d/CHANGELOG.md"
      want=1; version=1.2.0 ;;
    complete-pending)
      sed -i "/^## \[1.2.0\]/a $(entry "$fourth")" "$d/CHANGELOG.md"
      want=0; version=1.2.0 ;;
  esac
  # As the run that cuts the version: an empty state now skips a tagged version.
  out="$(cd "$d" && STATE=release CREATED=true GITHUB_STEP_SUMMARY=/dev/null bash -e -c "$main_step" 2>&1)"; rc=$?
  if [[ "$rc" -eq "$want" ]] && grep -qF "version $version," <<<"$out"; then
    echo "  PASS  main workflow $arm (exit $rc)"
    pass=$((pass + 1))
  else
    echo "  FAIL  main workflow $arm: wanted $want/version $version, got $rc: $out" >&2
    fail=$((fail + 1))
  fi
done
# aegis-3xv: a shipped (tagged) section that is wrong must fail the run that cut
# it, and only that run. Every later push skips it; a run that deferred to the
# release commit's own run skips it too.
git -C "$d" show quipu-ai-v1.1.0:Cargo.toml > "$d/Cargo.toml"
git -C "$d" show quipu-ai-v1.1.0:CHANGELOG.md | grep -vF "[$c]" > "$d/CHANGELOG.md"
# An EMPTY state (release-plz skipped: CI correctness failed) cut nothing, so a
# tagged version is skipped too (aegis-0zr: 861ac848 re-failed shipped 0.16.4).
for arm in "released::0" "released:true:1" "defer::0" "release::1" "::0"; do
  IFS=: read -r state created want <<<"$arm"
  out="$(cd "$d" && STATE="$state" CREATED="$created" GITHUB_STEP_SUMMARY=/dev/null \
    bash -e -c "$main_step" 2>&1)"; rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "  PASS  main workflow shipped-missing state=$state created=${created:-no} (exit $rc)"
    pass=$((pass + 1))
  else
    echo "  FAIL  main workflow shipped-missing state=$state created=${created:-no}: wanted $want, got $rc: $out" >&2
    fail=$((fail + 1))
  fi
done

# ...but an empty state on an UNTAGGED version still verifies it.
sed -i 's/version="1.1.0"/version="1.2.0"/' "$d/Cargo.toml"
sed -i '/^## \[1.1.0\]/i ## [1.2.0] - 2026-01-02\n' "$d/CHANGELOG.md"
out="$(cd "$d" && STATE="" CREATED="" GITHUB_STEP_SUMMARY=/dev/null bash -e -c "$main_step" 2>&1)"; rc=$?
if [[ "$rc" -eq 1 ]] && grep -qF "version 1.2.0," <<<"$out"; then
  echo "  PASS  main workflow empty state on an untagged version still verifies (exit $rc)"
  pass=$((pass + 1))
else
  echo "  FAIL  main workflow empty state untagged: wanted 1, got $rc: $out" >&2
  fail=$((fail + 1))
fi

# aegis-0zr: the release-plz job verifies the commit it is ABOUT to tag. 1.2.0's
# section misses the fourth commit here, so the cut is refused; with it listed,
# the cut proceeds.
out="$(cd "$d" && VERSION=1.2.0 bash -e -c "$cut_step" 2>&1)"; rc=$?
if [[ "$rc" -eq 1 ]] && grep -qF "Release not cut" <<<"$out" && grep -qF "$fourth" <<<"$out"; then
  echo "  PASS  pre-cut gate refuses a section missing a packaged commit (exit $rc)"
  pass=$((pass + 1))
else
  echo "  FAIL  pre-cut gate on missing commit: wanted 1, got $rc: $out" >&2
  fail=$((fail + 1))
fi
sed -i "/^## \[1.2.0\]/a $(entry "$fourth")" "$d/CHANGELOG.md"
out="$(cd "$d" && VERSION=1.2.0 bash -e -c "$cut_step" 2>&1)"; rc=$?
if [[ "$rc" -eq 0 ]]; then
  echo "  PASS  pre-cut gate allows a complete section (exit $rc)"
  pass=$((pass + 1))
else
  echo "  FAIL  pre-cut gate on complete section: wanted 0, got $rc: $out" >&2
  fail=$((fail + 1))
fi
rm -rf "$d"

# aegis-0zr: release-pr-freshness.sh checks the release PR AS IT WOULD SQUASH-MERGE
# onto main now. A packaged commit landing on main after the PR was prepared makes
# it stale (the #901/#903 shape); an excluded-only commit does not; a PR prepared
# from current main is fresh.
fresh_repo() {
  local d; read -r d a b c < <(make_repo)
  (
    cd "$d"
    git checkout -qb rel
    sed -i 's/version="1.0.0"/version="1.1.0"/' Cargo.toml
    { echo "# Changelog"; echo; echo "## [1.1.0] - 2026-01-01"; echo; entry "$b"; entry "$c"; } > CHANGELOG.md
    git add -A && git commit -qm "chore: release v1.1.0"
    git checkout -q -
  ) >/dev/null 2>&1
  echo "$d"
}
fresh_check() {
  local name="$1" want="$2" d="$3" out rc
  out="$(cd "$d" && ./scripts/release-pr-freshness.sh rel HEAD 2>&1)"; rc=$?
  if [[ "$rc" -eq "$want" ]]; then
    echo "  PASS  freshness: $name (exit $rc)"
    pass=$((pass + 1))
  else
    echo "  FAIL  freshness: $name: wanted $want, got $rc: $out" >&2
    fail=$((fail + 1))
  fi
  rm -rf "$d"
}
d="$(fresh_repo)"
fresh_check "PR prepared from current main is fresh" 0 "$d"
d="$(fresh_repo)"
(cd "$d" && echo late >> src/lib.rs && git commit -qam "fix: landed after the release PR") >/dev/null 2>&1
fresh_check "packaged commit after the PR makes it stale" 1 "$d"
d="$(fresh_repo)"
(cd "$d" && echo note > docs/late.md && git add -A && git commit -qm "docs: excluded only") >/dev/null 2>&1
fresh_check "excluded-only commit after the PR keeps it fresh" 0 "$d"
d="$(fresh_repo)"
(cd "$d" && printf '# Changelog\n\nhand edit\n' > CHANGELOG.md && git add -A && git commit -qm "chore: hand edit") >/dev/null 2>&1
fresh_check "a conflicting main makes it stale" 1 "$d"

# The detector's own arms, including its parse-error cases.
if python3 "$REPO_ROOT/scripts/changelog-duplicates.py" --selftest >/dev/null 2>&1; then
  echo "  PASS  changelog-duplicates.py selftest (all arms)"
  pass=$((pass + 1))
else
  echo "  FAIL  changelog-duplicates.py selftest" >&2
  fail=$((fail + 1))
fi

echo
echo "  ${pass} passed, ${fail} failed"
[[ "$fail" -eq 0 ]]
