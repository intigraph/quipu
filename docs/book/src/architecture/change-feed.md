# Change Feed

> **Implementation status (2026-08-31):** ✅ **Implemented** (quipu-2ae, from
> the Spanner investigation in `docs/design/spanner-capabilities.md` §4.4).
> `src/store/changes.rs` — `changes_after` with the three capture modes —
> served by `quipu changes` and `GET /changes`.

The append-only fact log has been a change stream all along; the change feed
gives it a **consumer contract**, modeled on Spanner's change streams but
adapted to a pull surface. It is fact-level and lossless where the
[event log](../concepts/governance.md) (`/events`) is semantic and
taxonomized: use `/events` to hear "an entity changed", use `/changes` to
mirror exactly which facts did.

## The contract

- **Records** are `(tx, sequence, op, graph, entity, attribute, value)`,
  derived directly from fact rows — never a second log that can drift from
  the first. `op` is `assert`, `retract`, or `tombstone`.
- **Valid time rides each record** as `valid_from` and `valid_to`, so a
  replica keeps the interval a fact was written with rather than its commit
  time. An assert reports the bound it was written with; when a later
  transaction closes the fact, that closing arrives as its own `retract`
  record, and the assert's record reads the same before and after.
- **Retractions carry `retracted_valid_at`**, the valid-time close of the
  assertion they ended, independently of their transaction `timestamp` and
  audit-row interval. A correction valid in March but promoted in June closes
  the replica's old value in March too. The producer recovers this bound from
  the actual closed assertion rows, including existing promoted history; it
  refuses missing or disagreeing closes rather than inventing a timestamp.
- **The cursor is a transaction id, and pages end on transaction
  boundaries.** Every cursor is therefore a consistent prefix of commit
  history: a reader never observes half a transaction.
- **Ordering**: per entity, records arrive in commit order (transaction,
  then write order within it). Across entities there is no ordering promise.
- **Every page names its source**: `source.store_id` is the store, and
  `source.log_epoch` is the epoch of its change log, minted when the store is
  created and minted again by any operation that removes served history. A
  follower that sees either value change is reading a different history and
  must re-bootstrap rather than keep its cursor. A store reconstructed from a
  share mints its own epoch.
- **Watermark instead of heartbeat**: every page carries `watermark_tx` and
  `watermark_timestamp` — the newest committed transaction. An empty page
  with an advancing watermark means the store is idle (or your graph scope
  is quiet); a watermark that never moves means check the writer.
- **Cursors never expire.** Spanner retains change records for 1–30 days;
  quipu's fact log is permanent, so a consumer can resume from any
  transaction id it ever held, including 0.

## Value capture modes

| Mode | A record carries |
|------|------------------|
| `new_values` (default) | Asserts carry `value`; a retract identifies `(entity, attribute)` but withholds the ended value. |
| `old_and_new_values` | A retract also carries the value it ended, as `old_value`. |
| `new_row` | `old_and_new_values`, plus the entity's full state **as of that record's transaction** under `row` — so consumers skip the read-back that a bare notification would force. |

The `row` snapshot uses the same as-of-transaction predicate the fork
snapshot uses, so the two surfaces cannot disagree about what "live at tx N"
means.

## Reading it

```bash
quipu changes --db homelab.db                        # from genesis, new_values
quipu changes --from 42 --capture new_row --limit 50 # page after tx 42
quipu changes --graph "http://example.org/graph/tenant-a"
```

```text
GET /changes?since=42&capture=old_and_new_values&limit=100
GET /changes?graph=http://example.org/graph/tenant-a
```

Both return one page:

```json
{
  "records": [
    {
      "tx": 43, "sequence": 0, "timestamp": "2026-04-03", "actor": "ingest",
      "source": "episode:...", "op": "assert", "graph": "ROOT",
      "entity": "http://example.org/korimako",
      "attribute": "http://example.org/cpuCores", "value": 8,
      "valid_from": "2026-04-03", "valid_to": null
    }
  ],
  "since": 42,
  "next_tx": 43,
  "watermark_tx": 43,
  "watermark_timestamp": "2026-04-03",
  "capture": "new_values",
  "source": {"store_id": "urn:uuid:…", "log_epoch": "urn:uuid:…"}
}
```

Pass `next_tx` back as the cursor (`--from` / `since`). On an empty page it
stays put, so polling is a fixpoint, not a rewind. `since` echoes the cursor
the page answers: the page holds every transaction in scope in
`(since, next_tx]`. A follower whose own cursor is below `since` has missed
the transactions between them and must resume from its cursor rather than
apply the page. The chunked `begin` and `commit` frames carry the same
`since`. `Ref` values resolve to
`{"ref": "<iri>"}` so a consumer can tell an edge from a string that happens
to look like one. Bytes arrive as `{"bytes_len": n, "base64": "..."}` — the
whole body, so a replica built from the feed holds the same value.

## First consumers

### Retraction compatibility

`retracted_valid_at` is an additive record field, emitted in every capture
mode for retractions only. Its supported representation is an ISO date or an
RFC 3339 timestamp string, preserved without normalization. Explicit null,
malformed values, unsupported representations and use on assertions are
refused by the shared applier without advancing that transaction's cursor.
Assertion `valid_from`/`valid_to` semantics are unchanged.

An incoming legacy retraction without the field is refused: its actual close
cannot be inferred safely from transaction time. Upgrade the source; legacy
assertions remain supported. For retained transaction identities, a close equal
to the transaction timestamp keeps the legacy digest, so an ordinary historical
retraction can still be recognized as a duplicate without rewriting it. A
different close on redelivery stops the replica rather than silently accepting
changed history. Page and chunked delivery share these checks.

Upgrade consumers before relying on the extension; older consumers ignore it.
Reading existing source history with the new producer recovers available close
bounds, but does not repair replicas that already applied the wrong interval.
Any rebuild, replay or stored-data repair requires separate operational
admission. This source change performs none of those actions.

The intended first consumer is incremental index/embedding maintenance
(bobbin): re-embed exactly the entities whose facts changed rather than
rescanning, using per-entity ordering to apply changes safely. The event-push
delivery worker's semantic events remain the right surface for workflow
triggers; the change feed is for mirrors.
