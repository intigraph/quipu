#!/usr/bin/env bash
set -euo pipefail
before=${1:?usage: run.sh BEFORE_BIN AFTER_BIN}
after=${2:?usage: run.sh BEFORE_BIN AFTER_BIN}
state=$(mktemp -d)
trap 'rm -rf -- "$state"' EXIT
for side in before after; do
    binary=$before
    [ "$side" = before ] || binary=$after
    printf '\n%s\n' "$side: actual CLI solicitation, isolated hook state, no HTTP"
    "$binary" --version
    printf '%s\n' '$ quipu hook session-capture < same-stop-event.json'
    printf '%s\n' '{"session_id":"capture-auth-demo","cwd":"/tmp/crew/demo","stop_hook_active":false}' |
        env -u QUIPU_AUTH_TOKEN -u QUIPU_AUTH_TOKEN_FILE \
            GT_CREW=demo QUIPU_HOOK_STATE_DIR="$state/$side" \
            QUIPU_SERVER=https://example.invalid QUIPU_HOOK_GROUP=demo \
            "$binary" hook session-capture |
        python3 -c '
import json, sys
response = json.load(sys.stdin)
message = response["reason"]
assert response["decision"] == "block"
assert "capture-auth-demo" in message
print(message.split("\n\n")[1])
facts = {
    "solicitation_present": True,
    "token_file_fallback": "QUIPU_AUTH_TOKEN_FILE" in message,
    "missing_credential_stops_before_http": "before HTTP" in message,
    "authenticated_mcp_preferred": "authenticated Quipu MCP" in message,
    "skip_preserved": "reply '\''skip'\''" in message,
}
print(json.dumps(facts, indent=2))
assert facts["skip_preserved"]
if sys.argv[1] == "after":
    assert all(facts.values()), "candidate did not emit the credential contract"
else:
    assert not facts["token_file_fallback"]
    assert not facts["missing_credential_stops_before_http"]
' "$side"
done
