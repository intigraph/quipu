#!/bin/sh
# Private fixture only. Build the server test binary before recording.
set -eu
: "${RELAY_TEST_BINARY:?Set the compiled quipu-server test binary path}"
printf '%s\n' 'Private protected source: anonymous refused; file bearer accepted; wrong/removed file refuses without fallback.'
"$RELAY_TEST_BINARY" --exact relay::http_auth_tests::credential_file_authenticates_both_upstream_operations_without_fallback --nocapture
printf '%s\n' 'Protected source -> relay -> leaf: data equality and signed freshness proof.'
"$RELAY_TEST_BINARY" --exact publication::replicate_stream::tests::relay::protected_http_source_replicates_and_proves_freshness_through_relay --nocapture
