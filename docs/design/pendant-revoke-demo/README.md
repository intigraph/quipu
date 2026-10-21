# Revoke carried-forward pendant suppression

The same explicit revoke command runs against the same disposable private
SQLite fixture. Baseline CLI `21bd8e8e` refuses the unsupported option (exit1).
Candidate code `ece955a1` returns `revoked: true, graphs_restored: 1`; a query
shows the restored fact and the unrelated positive control. Repeating returns
`revoked: false, graphs_restored: 0`. Combining rollback and revoke refuses.

The candidate recording used the working-tree code committed as `ece955a1`,
before its source commit; it is a private build, not an installed-service claim.
The baseline binary was clean0.14.1/21bd8e8e. Neither command contacted a live
endpoint. [Terminal output](output.log) and [timing](timing.log) are retained in
this repository. Download both and replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Create a new private fixture and reproduce on candidate source:

```sh
scratch=$(mktemp -d)
cargo run --example pendant_revoke_fixture -- "$scratch/store.db"
cargo run --bin quipu -- compose --repo scbrown/demo --pack-key packs/main \
  --revoke-tombstone '<urn:a> <urn:q> "secret" .' \
  --reason undo-local-retraction --db "$scratch/store.db"
```

The fixture refuses to overwrite an existing file. It creates only local source
and destination stores, an explicit empty shape set and an internal-destination
share. It installs no policy and uses no external provider.

The CURRENT suite has13 tests, including a negative control showing that deleting
only the ledger row causes the next composition to recreate it. Revocation stops
that recurrence by restoring known retractions before removing suppression.
It also covers repository/pack isolation, invalid input, repeat behavior, a new
later local retraction, and rollback after an injected ledger-update failure.

**A stripped graph stays stripped on replay and rollback.** Some snapshots
omitted the suppressed statement before storing their facts. Revocation never
fabricates those omitted facts. The tests distinguish replay of the exact same
share from a fresh source-pendant snapshot, which can bring the fact back after
suppression is revoked. See the [operation contract](../../book/src/concepts/pack-composition.md#undo-a-local-retraction).
