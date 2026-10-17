# Decision Deck fixture demonstration

Run from the repository root:

```sh
bash docs/probes/decision-deck/run-demo.sh
```

The base is `0cb06945e5f43104cbd4dd7a6301426ab243bc24`. It has the existing
WebAuthn verifier but no Decision Deck module. The candidate is the revision
containing this demonstration; the accompanying PR pins its exact head.

Watch both algorithms admit one explicit request revision, return a matching
read-only receipt and audit row, then refuse the same assertion as a replay.
The other controls exercise forged assertions, malformed client data, protected
graph writes, stale versions, expiry, revocation, rollback and durable recovery.

These are actual library calls against isolated temporary stores and software
authenticators. This is not a browser ceremony, production deployment or human
identity proof. The base lacks the new API, so the before comparison establishes
source availability; the candidate tests demonstrate the new behavior.

`output.cast` is an output-only terminal recording, converted from the real
util-linux `script` output and timing with every captured byte preserved.
The capture's source timing was replayed with `scriptreplay`; conversion was
checked by reconstructing its captured bytes. Replay the cast after downloading:

```sh
asciinema play output.cast
```

`transcript.txt` is the actual captured output with normalized line endings.
A terminal player is required; cast playback was not tested because asciinema
was unavailable. No inline video playback is claimed. Build diagnostics are withheld from the
recording on successful runs; a failed reproducer prints them and exits nonzero.
