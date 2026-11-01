#!/usr/bin/env bash
# arming: agent-invoked source-only demonstration with owned fixture binaries
set -euo pipefail
: "${TRANSITION_BASE_PROBE:?provide the base example executable}"
: "${TRANSITION_HEAD_PROBE:?provide the candidate example executable}"
printf '%s\n' '$ base: same private in-memory probe, source d707ec62'
base_result=$("$TRANSITION_BASE_PROBE")
printf '%s\n' "$base_result"
diff -u <(cat <<'EOF'
fixture=in-memory keys=ephemeral live_operations=NONE
unendorsed_root=ADMITTED
unendorsed_named_identity=ADMITTED
human_root_endorsed=ADMITTED
legacy_flag_off=ADMITTED
EOF
) <(printf '%s\n' "$base_result")
printf '\n%s\n' '$ candidate: same private in-memory probe, governed ROOT keys'
head_result=$("$TRANSITION_HEAD_PROBE")
printf '%s\n' "$head_result"
diff -u <(cat <<'EOF'
fixture=in-memory keys=ephemeral live_operations=NONE
unendorsed_root=REFUSED
unendorsed_named_identity=REFUSED
human_root_endorsed=ADMITTED
legacy_flag_off=ADMITTED
EOF
) <(printf '%s\n' "$head_result")
printf '\n%s\n' 'PASS: self-enrollment refused; human endorsement and opt-in control preserved'
