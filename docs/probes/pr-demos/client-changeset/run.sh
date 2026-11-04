#!/usr/bin/env bash
# arming: agent-invoked — disposable in-memory library demonstration only.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
probe=signed_cut_tail_private_overlay_admission_receipt_and_replica_convergence
metadata=$(mktemp)
trap 'rm -f "$metadata" "$metadata.stderr"' EXIT
printf '%s\n' 'Compile the disposable library fixture, then execute its real harness:'
printf '$ just test --locked --offline --lib %s --no-run --message-format=json\n' "$probe"
if ! just test --locked --offline --lib "$probe" --no-run --message-format=json >"$metadata" 2>"$metadata.stderr"; then
    cat "$metadata.stderr" >&2
    exit 1
fi
binary=$(python3 - "$metadata" <<'PY'
import json, sys
artifacts = [json.loads(line) for line in open(sys.argv[1]) if line.startswith('{')]
binaries = [row['executable'] for row in artifacts
            if row.get('reason') == 'compiler-artifact' and row.get('executable')
            and row.get('profile', {}).get('test') and 'lib' in row['target']['kind']]
assert len(binaries) == 1, 'expected exactly one library test harness'
print(binaries[0])
PY
)
printf '%s\n' 'Execute the compiled fixture harness (one matching test, output uncaptured):'
"$binary" "$probe" --nocapture
printf '%s\n' 'Rebase over canonical catch-up: preserve the old signed operation, sign a fresh operation, admit once and retry:'
"$binary" explicit_rebase_admits_new_operation_once_and_never_retargets_old_signed_bytes --nocapture
