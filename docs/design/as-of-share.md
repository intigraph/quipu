# As-of scoped share: the replica bootstrap

Status: design, not implemented. This document fills in the bootstrap half of
`local-sync-mode.md` §5. Per-graph read authorization (that design's P1, second
half) is a separate design and a separate ruling. Nothing here grants a second
principal any read.

## Problem

A follower must start from a state equal to its source at some transaction
`T`, then tail `/changes` from `T`. Today it cannot do either well:

- **Replaying the feed from zero** is the only complete path. It is expensive:
  a single-record `/changes?since=0` page takes seconds on a large store. It
  also cannot reproduce a frozen graph, because a deep freeze removes that
  graph's rows from the feed.
- **`POST /acquire`** (`quipu-acquisition/v1`) cuts a consistent, pinned,
  hashed read, but cannot serve as the bootstrap:
  - Its canonical N-Triples cannot carry a fact's valid interval, so the
    bootstrapped replica cannot equal a source that the feed describes with
    valid time.
  - It has no `log_epoch`, so a follower cannot tie the cut to the history it
    will tail.
  - It has an 8 MiB cap with no chunking.
  - It refuses frozen graphs.

## Design

### The cut reuses acquisition; the payload is change records

The as-of share is produced exactly as an acquisition is:
- one SQLite read transaction;
- the store identity, head and epoch read inside it;
- every selected graph read at one validated transaction `T`;
- complete, or an error with no partial receipt.

What changes is the payload. Each live fact at `T` is emitted as an `assert`
record in **the same JSON record form as `/changes`**:
- `entity`, `attribute`, `graph`;
- the typed `value`, with bytes as base64;
- `valid_from` and `valid_to`.

A follower converts bootstrap records and tail records with **one**
record-to-datum conversion, the shared applier's. A fact is live at `T` when:
- it was asserted at or before `T`;
- it was not retracted at or before `T`;
- its valid interval is whatever the source stores for it at `T`.

The replica proves equality with the same canonical live-fact digest the
feed-replay conformance tests use. A text form that cannot carry valid time
or bytes is refused, not offered as a degraded bootstrap.

### Manifest

```json
{
  "schema": "quipu-asof-share/v1",
  "source": {"store_id": "...", "log_epoch": "..."},
  "watermark_tx": 742611,
  "source_head_tx": 742630,
  "scope": ["<graph IRI>", "..."],
  "context_graphs": ["<graph IRI>", "..."],
  "scope_digest": "sha256:...",
  "policy_digest": "sha256:...",
  "shapes_hash": "sha256:...",
  "completeness": {
    "valid_time": true,
    "bytes": "lossless",
    "graphs": {
      "<graph IRI>": {"records": 123, "digest": "sha256:...", "frozen": null},
      "<frozen graph IRI>": {"records": 45, "digest": "sha256:...",
                             "frozen": {"pack_content_hash": "...", "frozen_at_tx": 700001}}
    }
  },
  "producer": {"version": "...", "git_sha": "...", "git_dirty": false}
}
```

- `watermark_tx` is the last source transaction included. The follower tails
  from it.
- `scope_digest` is SHA-256 over the sorted scope IRIs, then the sorted
  context IRIs, newline-separated.
- `policy_digest` binds the read authorization the cut was served under.
  Until per-graph read authorization exists, it names the single rule in
  force: an authenticated write-class principal is required.
- Each graph's `digest` is SHA-256 over its records' canonical JSON lines,
  sorted. Following the share-manifest encoding, that is compact UTF-8 JSON
  with recursively sorted keys.

The manifest's own hash is computed the same way as the acquisition receipt
hash.

### Size: the feed's chunk frames, not a second scheme

A cut that fits one frame is one `quipu.asof` frame. A larger cut uses the
change stream's oversized-transaction frames:
- `begin` with `source_tx = watermark_tx`, the part count, totals, the digest
  and the source;
- numbered `part` frames with a running `digest_before`;
- a `commit` barrier repeating the digest and the source.

These are the same transport-neutral `FeedFrame`s, so the hub and any
transport carry them unchanged. A follower stages every part and applies
nothing until the barrier digest matches. A disconnect mid-cut discards the
stage and restarts the cut; it never resumes half-way.

### Frozen graphs (the bootstrap must equal the source)

A frozen graph's rows live in its mounted pack. Freezing respaces term ids
but not transaction ids: `facts.tx`, `facts.retracted_tx` and
`transactions.id` are not remapped. So the pack can be read as of `T` exactly
like main:
- The cut reads the frozen graph's live facts at `T` from the pack.
- It emits them as ordinary records.
- It names the pack's `content_hash` and freeze transaction in `completeness`.

A follower rebuilt from the share therefore equals the source, including
frozen graphs. Its feed tail then correctly carries nothing more for them.

If a selected frozen graph's pack is not mounted, or fails its hash
verification, the cut is refused. It is never emitted without that graph.

### Handoff to the tail, and epoch changes

The follower records `(store_id, log_epoch, watermark_tx)` and tails
`/changes?since=watermark_tx`. Every page and every chunk frame names its
source. On any page whose `log_epoch` differs from the recorded one, the
follower:
1. stops;
2. refuses to splice;
3. discards its state;
4. bootstraps again from a fresh as-of share.

A freeze or thaw mints a new epoch; a freeze or thaw that moves no rows does
not.

### Route

`POST /share/asof` with `{graphs, context_graphs?, tx?, expected_store_id}`.
- `tx` absent means "now": the cut is taken at the head read inside the
  transaction.
- The same validation as `/acquire` applies: 1–64 registered committed graphs
  (frozen allowed), no dataset expansion, no implicit ROOT, unknown fields
  refused.

**Authorization.** Until per-graph read authorization lands, the route
requires an authenticated **write-class** principal: it is listed with the
write endpoints for bearer authorization and read-only mode. That is
fail-closed. No unauthenticated or read-only caller can pull a cut, so no
second principal is served before the read model's ruling exists. Making the
route read-class is a change for the read-authorization work, not for this
one.

## Verification plan

- **Equality:**
  1. Bootstrap at `T` under concurrent writes, then tail from `T`.
  2. Compare: the follower's live digest equals the source's live digest at
     the tail's end, and also at `T` itself.
  3. Control: a replica built by full feed replay from zero equals it too.
- **No gap or overlap:** a write committed during the cut appears exactly
  once, in the tail, never in the cut.
- **Valid time and bytes:** fixtures with bounded and open intervals and a
  byte value replay exactly. Sabotage: dropping `valid_to`, or emitting
  `bytes_len`, turns them red.
- **Frozen graph (s24.1.1):**
  1. Freeze a graph and take a cut.
  2. The follower's digest equals the source's, including the frozen graph.
  3. Control: the same flow without the frozen-pack read fails equality.
  4. An unmounted pack refuses the cut.
- **Epoch conformance (s24.1.1):**
  1. The follower is at `watermark_tx`; the source freezes another graph,
     which bumps the epoch.
  2. The follower's next page names the new epoch.
  3. It refuses to splice, re-bootstraps, and equals the source again.
  4. Control: with no epoch check, splicing would leave the follower unequal.
  5. A no-op freeze does not bump, and the follower keeps tailing.
- **Chunking:** a cut over the frame budget arrives as begin, parts and
  commit. A dropped part refuses the commit and records nothing. A
  disconnect restarts the cut.
- **Authorization:** the route refuses no bearer, refuses read-only mode, and
  refuses an unknown or dataset IRI. Control: an authenticated write-class
  principal succeeds.

## Rollback

- Remove the route. Nothing else changes, and no stored state is added.
- Followers fall back to full feed replay, which still cannot reproduce frozen
  graphs.
