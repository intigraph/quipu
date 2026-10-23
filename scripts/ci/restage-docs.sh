#!/usr/bin/env bash
# Restage the book after a release uploads its Wasm assets, or say why not.
#
# Called by release.yml's Wasm job, AFTER the assets are on the release. The
# restage is a side effect: the book page serves the bundle from this site, so
# it only shows a new release once docs.yml runs again. It is not part of the
# release itself.
#
# A docs.yml that an operator DISABLED is a decision, not a fault. v0.16.1
# measured the old inline `gh workflow run docs.yml`: HTTP 422 "Cannot trigger
# a 'workflow_dispatch' on a disabled workflow" turned the Wasm job red, and
# `crates` (needs: wasm == success) never published. A deliberate docs hold
# blocked a crates.io release it had nothing to do with.
#
# So the workflow's state decides, read first and named in the output:
#   active                                  -> dispatch; a failed dispatch FAILS
#   disabled_manually / disabled_inactivity -> notice, exit 0, nothing dispatched
#   anything else, or an unreadable state   -> FAIL (never guessed into a skip)
#
#   usage: restage-docs.sh <repo> [workflow-file] [ref]
#          restage-docs.sh --selftest
#
#   exit 0  dispatched, or skipped because the workflow is disabled
#   exit 1  state unreadable or unknown, or the dispatch failed
#   exit 2  called wrong

set -uo pipefail

restage() {
    local repo="$1" workflow="${2:-docs.yml}" ref="${3:-main}" state

    if ! state=$(gh api "repos/${repo}/actions/workflows/${workflow}" --jq .state); then
        echo "::error::docs restage: could not read the state of ${workflow}; not dispatched" >&2
        return 1
    fi
    case "$state" in
        active)
            if ! gh workflow run "$workflow" --repo "$repo" --ref "$ref"; then
                echo "::error::docs restage: dispatch of active ${workflow} failed" >&2
                return 1
            fi
            echo "docs restage dispatched: ${workflow} on ${ref}"
            ;;
        disabled_manually|disabled_inactivity)
            echo "::notice::docs restage skipped: ${workflow} ${state}"
            if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
                echo "Docs restage skipped: \`${workflow}\` is ${state}. The release assets are unaffected; the book page shows this release once ${workflow} runs again." >>"$GITHUB_STEP_SUMMARY"
            fi
            ;;
        *)
            echo "::error::docs restage: ${workflow} has unexpected state '${state}'; not dispatched" >&2
            return 1
            ;;
    esac
}

selftest() {
    local dir fail=0 out rc
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' RETURN
    # A stub gh: STUB_STATE answers the state read (STUB_STATE_RC its exit),
    # STUB_RUN_RC answers the dispatch, and every dispatch is logged.
    cat >"$dir/gh" <<'STUB'
#!/usr/bin/env bash
if [ "$1" = api ]; then
    [ "${STUB_STATE_RC:-0}" = 0 ] || { echo "HTTP 502" >&2; exit "$STUB_STATE_RC"; }
    echo "$STUB_STATE"; exit 0
fi
if [ "$1 $2" = "workflow run" ]; then
    echo "$*" >>"$STUB_LOG"
    [ "${STUB_RUN_RC:-0}" = 0 ] || echo "HTTP 422: dispatch refused" >&2
    exit "${STUB_RUN_RC:-0}"
fi
exit 99
STUB
    chmod +x "$dir/gh"

    check() {
        local name="$1" want_rc="$2" want_dispatches="$3" want_text="$4"
        shift 4
        : >"$dir/log"
        out=$(env PATH="$dir:$PATH" STUB_LOG="$dir/log" GITHUB_STEP_SUMMARY= "$@" \
            bash -c "$(declare -f restage); restage owner/repo docs.yml main" 2>&1)
        rc=$?
        local dispatches
        dispatches=$(wc -l <"$dir/log")
        if [ "$rc" = "$want_rc" ] && [ "$dispatches" = "$want_dispatches" ] \
            && grep -qF -- "$want_text" <<<"$out"; then
            echo "ok   $name"
        else
            echo "FAIL $name: rc=$rc (want $want_rc), dispatches=$dispatches (want $want_dispatches), output: $out"
            fail=1
        fi
    }

    check "active -> dispatched"               0 1 "docs restage dispatched" STUB_STATE=active
    check "disabled_manually -> notice, green" 0 0 "docs restage skipped: docs.yml disabled_manually" STUB_STATE=disabled_manually
    check "disabled_inactivity -> notice"      0 0 "docs restage skipped: docs.yml disabled_inactivity" STUB_STATE=disabled_inactivity
    check "active but dispatch fails -> red"   1 1 "dispatch of active docs.yml failed" STUB_STATE=active STUB_RUN_RC=1
    check "unknown state -> red, no dispatch"  1 0 "unexpected state 'disabled_fork'" STUB_STATE=disabled_fork
    check "unreadable state -> red"            1 0 "could not read the state" STUB_STATE=active STUB_STATE_RC=1
    check "empty state -> red"                 1 0 "unexpected state ''" STUB_STATE=

    if [ "$fail" = 0 ]; then
        echo "restage-docs selftest: all arms passed"
    fi
    return "$fail"
}

case "${1:-}" in
    --selftest) selftest ;;
    ""|-*) echo "usage: restage-docs.sh <repo> [workflow-file] [ref] | --selftest" >&2; exit 2 ;;
    *) restage "$@" ;;
esac
