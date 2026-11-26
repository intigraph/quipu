# Opt-in change-stream progress

Status: source candidate; native qualification and deployed acceptance pending.

`GET /changes/stream?graph=IRI&predicate=IRI&progress=true` opts into
`quipu.changes.progress` frames. This first version accepts one explicit named
graph and one predicate, without subject or membership filters. Other progress
scopes refuse with HTTP 400. Ordinary streams retain their existing wire contract.
There are no service or client names in the producer.

A progress frame has the `quipu.change-progress/v1` schema, source store/epoch,
`scope: {graphs: [IRI], predicate: IRI}`, the previous `since`, equal `next_tx`
and `watermark_tx`, empty `records`, `complete: true`, and a finite UTC Unix
`observed_at`. Its SSE id is the new transaction cursor. A quiet observation may
repeat that cursor. Delivery does not commit a consumer offset.

The hub crosses the serving writer barrier and reads identity, head and all
intervening transactions within one SQLite read snapshot. The observation time
is taken before the snapshot's first read, so a slow scan cannot relabel an old
snapshot as newly current. Progress is available only after that complete scan,
with the same identity, and after the subscriber has searched the whole ring
interval. Matching data and pending multipart frames precede progress. A history
change, missing observation, incomplete scan or eviction gap cannot attest
freshness. Catch-up still reads the native log before joining the ring.

While at least one progress subscription is attached, one shared reader performs
a consistency observation at most every 60 seconds while quiet. The first lease
wakes that reader immediately; additional leases share it. Commit hints retain
the existing immediate wake. External writers without this connection's commit
hook become visible at the shared observation, never through a per-subscriber
poll. Once the last lease is dropped, ordinary hourly reconciliation resumes.
An opted-in scan is capped at 16 pages of 64 whole transactions; exhaustion
invalidates the observation and subscribers catch up instead of receiving proof.
This is a page bound, not a transaction byte or wall-time guarantee.

Consumers must validate source, scope, cursor continuity and observation age;
keepalive comments never refresh validity. Future valid-time transitions, initial
snapshot admission, endpoint authority and installed consumer acceptance remain
separate gates. Gold PR1205's parser remains unwired to progress until this
producer is qualified and served. No production activation is claimed here.
