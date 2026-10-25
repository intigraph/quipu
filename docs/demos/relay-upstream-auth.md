# Relay upstream authentication: private HTTP fixture

This is a replayable terminal recording, not a production relay probe.
[Compressed output](relay-upstream-auth.output.gz), [timing](relay-upstream-auth.timing),
and [repeatable runner](relay-upstream-auth.sh) are retained with this change.
Replay with:

```sh
gzip -dc docs/demos/relay-upstream-auth.output.gz > /tmp/relay-upstream-auth.output
scriptreplay --log-out /tmp/relay-upstream-auth.output \
  --log-timing docs/demos/relay-upstream-auth.timing
```

The first fixture uses the actual HTTP bearer policy: anonymous transport is
refused, a private file authenticates both stream and challenge, a wrong rotated
token is refused once per operation, and removing the file causes zero further
requests. The second fixture passes real signed replication from a protected
HTTP source through a relay to a leaf, checks data equality after a new commit,
and verifies a fresh challenge proof.

Before this change (base `268641e4fdfa2a3aa6d753f6369881a12381acf5`), the
same anonymous-policy controls were refused and direct bearer controls passed;
the credential-file configuration failed as an unknown field. The source head
and complete validation are recorded in the pull request. The recording shows
the candidate's actual results; the historical failure is not reenacted.

To repeat after compiling the server test binary, set `RELAY_TEST_BINARY` to the
compiled `quipu-server` test executable and run the runner under the project's
HOME escape guard. All bearer values and stores are disposable fixture data.
This proves transport and replication behavior, not production provisioning,
graph-read grants, serving leases or revocation of an already open stream.
