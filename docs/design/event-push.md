# Event push: one feed read per transaction, WebSocket, named-graph events

**Status:** proposed (2026-10-24). Builds on `docs/book/src/architecture/event-stream.md`
(the SSE feed streams) and `local-sync-mode.md` §3, §5 and §6 (consumer contract,
freshness, bounded transfer). Nothing here changes the feed's frames, cursors or
authorization; it changes how many times the store is read to serve them, adds a
second transport, and widens which graphs emit semantic events.

## 1. Problem, measured

- **Every stream reads the store on its own.** `feed_stream.rs` gives each SSE
  connection its own cursor loop: on every commit wake, each connection pages
  `/events` or `/changes` from its own offset. N live subscribers cost N reads
  per transaction, so push cannot be cheaper than polling at fleet scale.
- **64 streams per process** (one semaphore, 503 when full). A consumer that
  needs several filters (graphs, event types) holds several streams.
- **Semantic events are ROOT-only.** `emit_events` returns early for any
  `graph != 0`. The work board lives in a registered named graph, so its
  consumers cannot subscribe to "an item changed" and fall back to polling
  `/query`. On a production copy, 2026-10-22 00:00–07:20Z: the board graph took
  32,263 datums in 2,784 transactions, while the whole events log took 3,796
  events in 1,567 transactions.
- `/events/stream` refuses `graph`; `/changes/stream` refuses `types` and `group`.

## 2. Shared feed hub

One hub per process and feed kind (`events`, `changes`):

- **One reader.** On each commit wake the hub crosses the writer barrier (as the
  SSE loop does today), reads the new page(s) once from its head cursor, builds
  the frames once (including the F6 chunked parts, using the same
  transport-neutral frame producer the SSE path uses), and appends them to a
  bounded in-memory ring keyed by cursor.
- **Subscribers filter in memory.** A subscription is `(feed, since, filter)`.
  The filter is applied to frames already read (`graph` for changes; `types`,
  `group` and `graph` for events). Matching is a pure function of the frame, so
  a filtered subscriber does no store read.
- **Catch-up, then live.** A subscriber whose `since` is behind the ring's
  oldest cursor pages the store directly with its own cursor (today's code,
  unchanged) until it reaches the ring, then switches to ring delivery. The
  switch happens at an exact cursor: frames are delivered in strictly
  increasing cursor order with no gap and no duplicate (the same at-least-once
  delivery contract, with dedup by cursor on the consumer side).
- **Idle cost is zero.** No commit, no read. Heartbeats come from a timer and
  never touch the store.
- **Slow subscribers do not slow the hub.** Each subscriber has a bounded send
  queue. If it falls behind the ring's tail it is moved back to catch-up paging
  (and leaves the ring) rather than holding frames or blocking the reader. A
  subscriber that cannot keep up even in catch-up is closed with a
  `quipu.error` that names the last delivered cursor.
- **Authorization** is evaluated per subscription at subscribe time and again
  per frame for graph-scoped frames, with the same check `/query` uses (P1
  per-graph ACL when s24.1 lands). The hub never widens what a principal can
  read; it only shares the read.

The hub sits behind both transports. SSE keeps its URL, frames, IDs and
`Last-Event-ID` resume; only its reader moves into the hub.

## 3. WebSocket transport

`GET /feed/ws` upgrades to a WebSocket carrying JSON text messages.

Client to server:

```json
{"op": "subscribe", "sub": "board", "feed": "changes", "since": 713600, "graph": "<iri>"}
{"op": "subscribe", "sub": "items", "feed": "events", "since": 4100, "types": ["entity.updated"], "graph": "<iri>"}
{"op": "unsubscribe", "sub": "board"}
```

Server to client:

```json
{"sub": "board", "event": "quipu.changes", "id": "713601", "data": { ... }}
{"sub": "board", "event": "quipu.changes.part", "id": "713602", "data": { ... }}
{"event": "quipu.heartbeat", "ts": "..."}
{"sub": "items", "event": "quipu.error", "data": "...", "last": 4120}
```

- `event`, `id` and `data` are exactly the SSE frame's fields, so a consumer's
  frame handling is transport-independent; `sub` routes it.
- **Multiplexing.** Many subscriptions share one connection. Connections and
  subscriptions are bounded separately: the existing 64 counts connections
  (SSE streams plus WebSockets); `max_subscriptions_per_connection` (default 32)
  and a process-wide `max_subscriptions` (default 1,024) bound the rest. A
  refused subscribe answers that subscription with `quipu.error`, not the
  connection.
- **Resume** is per subscription: reconnect and re-subscribe with `since` = the
  last applied cursor. There is no server-side session state to lose.
- **Heartbeat** every 15 s per connection, from a timer. Liveness only: it
  proves the connection, never a freshness bound (local-sync §5, amendment 5').
- **Unknown messages and frames.** The server refuses an unknown `op` with an
  error for that message. Consumers must ignore an unknown `event` they do not
  handle, rather than treat it as a fault. (An installed consumer logging
  "unexpected change stream event" on every heartbeat cadence is the failure
  this rule prevents.)
- Same authentication as reads, checked at upgrade; the upgrade route is
  declared like every other route (auth, route inventory, REST docs).

## 4. Named-graph semantic events

- **Which graphs.** ROOT, plus every **registered** named graph
  (`registered_graph_id` is `Some`). Overlay and other unregistered graphs stay
  silent for the reason the module docs give: they are compose-only staging,
  and announcing them would double-fire on promotion.
- **Which events.** For a registered named graph: `entity.added` (the entity's
  first facts *in that graph*) and `entity.updated`, one per entity per
  transaction, plus `episode.ingested` when applicable. No `edge.*` events:
  these consumers need "which items changed", and the facts themselves are on
  `/changes`. This bounds volume by entities touched, not datums written.
  `type.new` and `predicate.new` stay store-wide and are emitted only from ROOT,
  unchanged.
- **Separate tables, so no older binary can see them.** Named-graph events
  go in their own table, `graph_events`: the `events` columns plus `graph
  TEXT NOT NULL`, the graph IRI, never a term id (so respacing never rewrites
  it). Its own `offset` sequence, indexed `(graph, "offset")`. A graph
  consumer's committed cursor goes in `graph_consumers (consumer, graph,
  committed_offset)`, never in `consumers`. `events` and `consumers` are
  untouched: a ROOT event's offset, wire form and pruning are byte-for-byte
  what they were. Additive migration; no backfill.
  A nullable `graph` column on `events` was rejected (review of this design):
  quipu has no schema-version refusal, so an older binary opens a newer store,
  and its `events_after`, which has no graph predicate, would deliver board
  events to every ROOT consumer after a rollback.
- **Committed graphs only.** "Registered" means a `committed`-class row in
  `graphs`; overlay-class rows are registered too, and stay silent.
- **Lag.** A reader's `lag` counts only its own graph's events (ROOT's when
  `graph` is absent), or an unfiltered ROOT consumer's lag would climb with
  board writes it never receives.
- **Filter rule.** On `/events`, `/events/stream` and WebSocket `events`
  subscriptions, `graph` selects exactly one graph's events. Absent `graph`
  means ROOT only, which is today's behaviour, so no existing consumer starts
  receiving board events unasked. `types` and `group` combine with `graph` by
  AND. There is no "all graphs" wildcard: a consumer names each graph, and
  each name is authorized.
- **Retention** is the existing `prune_events` cutoff, applied to each table
  against its own consumers: `consumers` holds back `events`, and
  `graph_consumers` holds back `graph_events` for that graph. Volume is re-measured
  on a production copy before rollout, and the measurement is recorded beside
  this design.

## 5. Delivery order

1. **Named-graph events and graph filter** (§4): emitter, migration, filter on
   `/events` and `/events/stream`. Independent of the stream loop.
2. **Hub** (§2) behind the existing SSE endpoints, after the chunked-frame and
   epoch work on the change feed has landed, reusing its frame producer
   unchanged.
3. **WebSocket** (§3) over the hub.

## 6. Acceptance

- N subscribers on one feed cost one store read per transaction: a counter of
  hub page reads per committed transaction stays at 1 for N = 1, 10 and 100
  live subscribers. Idle subscribers cost zero reads, with heartbeats only.
- Resume after disconnect loses no transaction: a fault-injection test drops
  the connection at every frame boundary, including mid-chunk, and the consumer
  converges to the source.
- A slow subscriber never delays others: with one subscriber stalled, the
  others' delivery latency is unchanged, and the stalled one is moved to
  catch-up and converges or is closed with its last cursor.
- A consumer of graph G sees G's events and no other graph's; an absent filter
  sees exactly today's ROOT events; an unauthorized graph is refused.
- More than 64 filters are served over at most 64 connections.
- SSE behaviour, frames, IDs and resume are unchanged for existing consumers.

## 7. Rollback

Each step reverts independently, and every revert is to plain older code.
There is no rollback floor and no deployment order, because none can be
enforced: quipu opens a store whatever its schema, so the guarantee has to
hold for any older binary.

- **Named-graph events.** Older code never reads `graph_events` or
  `graph_consumers`. After a revert, ROOT consumers receive exactly the ROOT
  log, at the same offsets, from their committed cursors in `consumers`, and
  pruning sees only `events`. The graph rows and cursors stay on disk
  untouched, so re-upgrading resumes each graph consumer from its committed
  cursor. Nothing is migrated back and nothing is deleted.
  A graph consumer that reaches older code is the one reader that changes.
  Older code ignores the `graph` parameter, so `/events?graph=G` returns ROOT
  events, and a commit naming `graph` lands in `consumers`. The consumer
  contract covers both:
  - Discard any event whose `graph` is not G. ROOT events carry no `graph`
    key.
  - Commit only when the response echoes `graph`.
- **Hub.** It sits behind the existing SSE handlers, so reverting it restores
  per-connection reads.
- **WebSocket.** The route is additive.

**Acceptance pair (rollback).**

1. On one store: commit ROOT and named-graph writes; commit a ROOT consumer's
   cursor and a graph consumer's cursor.
2. Read with the PRE-CHANGE reader. Use its exact `events_after` SQL and the
   `consumers` resume, against the post-change store. It returns only the
   ROOT events, at their original offsets, resuming from the ROOT cursor.
   Prune with the pre-change `prune_events`: no `graph_events` row is touched.
3. Read again with the new reader: the graph consumer resumes from its own
   cursor and receives only its graph's events.
4. Control: the rejected design, a `graph` column on `events`, fails step 2.
   The pre-change reader returns the named-graph event too.
