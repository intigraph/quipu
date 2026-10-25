#!/usr/bin/env bash
set -euo pipefail
before=${1:?usage: run.sh BEFORE_BIN AFTER_BIN}
after=${2:?usage: run.sh BEFORE_BIN AFTER_BIN}
state=$(mktemp -d)
trap 'rm -rf -- "$state"' EXIT
for side in before after; do
    binary=$before
    [ "$side" = before ] || binary=$after
    printf '\n%s\n' "$side: actual CLI solicitation, isolated state, no HTTP"
    "$binary" --version
    printf '%s\n' '$ quipu hook session-capture < same-stop-event.json'
    printf '%s\n' '{"session_id":"capture-provenance-demo","cwd":"/tmp/crew/demo","stop_hook_active":false}' |
        env -u QUIPU_AUTH_TOKEN -u QUIPU_AUTH_TOKEN_FILE \
            GT_CREW=demo QUIPU_HOOK_STATE_DIR="$state/$side" \
            QUIPU_SERVER=https://example.invalid QUIPU_HOOK_GROUP=demo \
            "$binary" hook session-capture |
        python3 -c '
import json, sys
response = json.load(sys.stdin)
message = response["reason"]
assert response["decision"] == "block"
print(message.split("\n\n")[1])
headers = {h: h in message for h in (
    "X-Quipu-Agent", "X-Quipu-Harness", "X-Quipu-Model",
    "X-Quipu-Session", "X-Quipu-Host")}
facts = {
    "structured_headers": headers,
    "derive_not_guess": "Never type or guess" in message,
    "missing_fields_keep_capture_pending": "keep capture pending" in message,
    "declarations_are_not_authentication": "do not authenticate" in message,
    "authenticated_mcp_preferred": "authenticated Quipu MCP" in message,
    "credential_stop_before_http_preserved": "before HTTP" in message,
}
print(json.dumps(facts, indent=2))
assert facts["authenticated_mcp_preferred"]
assert facts["credential_stop_before_http_preserved"]
if sys.argv[1] == "after":
    assert all(headers.values())
    assert facts["derive_not_guess"]
    assert facts["missing_fields_keep_capture_pending"]
    assert facts["declarations_are_not_authentication"]
else:
    assert not any(headers.values())
' "$side"
done
