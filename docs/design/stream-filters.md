# Change-stream filters by subject, predicate and membership

Status: design, aegis-n0wc.3 (child of aegis-xdvg). Implementation follows this
document; fluent-value subscriptions are a separate, later slice (section 6).

## 1. Problem

A push consumer that receives every change and then re-runs its board query has
only moved the busy read (Stiwi, 2026-11-11). Today the transaction stream
(`GET /changes/stream`, and `{"feed":"changes"}` on `GET /feed/ws`) filters by
one `graph` only, and the event stream by `types`, `group` and `graph`. Nobody
can subscribe to "status changes on the items assigned to me". An unknown
parameter is refused with 400 (`Params` is `deny_unknown_fields`).

Acceptance (aegis-n0wc.3): a subscriber for status changes on one assignee's
items receives only those transactions, shown with a positive and a negative
control.

## 2. What the stream already carries

Each `quipu.changes` frame holds every record of one transaction:
`entity` (subject IRI), `attribute` (predicate IRI), `value` or `old_value`,
`graph`, `op`. The hub (`src/server/feed_hub.rs`) reads each commit once and
filters it per subscriber in memory; a subscriber behind the ring floor catches
up from the store with the same filter in SQL. `a_ring_page_equals_the_filtered_store_page`
holds the two paths equal. Every new filter keeps that parity.

## 3. Filters

Three new parameters on the change stream, on SSE (query string) and on the
WebSocket `subscribe` message. All are optional.

| Parameter | Matches a record when | Cost |
|---|---|---|
| `predicate` | its `attribute` is one of the listed IRIs | pure, per record |
| `subject` | its `entity` is one of the listed IRIs | pure, per record |
| `member` | its `entity` satisfies a membership condition before OR after the transaction | one batched store read per commit, shared by all subscribers |

- Lists are comma-separated on SSE and JSON arrays on the WebSocket. At most 64
  IRIs per list, and the existing 2,048-byte cap per filter string still holds
  on SSE.
- Within one parameter the values are OR. Across parameters (and with `graph`)
  they are AND. A frame carries only the matching records of a transaction; a
  transaction with no matching record sends no frame, but it still moves the
  subscriber's scanned watermark (as `graph` does today), so a quiet filter never
  falls into catch-up.
- Filter kinds stay separated: these three are change-stream filters. On the
  event stream they are refused (as `graph`-only rules are today).

### 3.1 `member`: class and property membership

`member` names a condition on the subject: `<predicate> <value>`, where value is
an IRI (`{"ref": ...}`) or a literal. A class filter is the special case
`rdf:type <Class>`; "items assigned to alice" is
`<assignee> "alice"`. One condition per subscription in the first slice.

**Before OR after.** A subject matches if the condition holds in the state
before the transaction or in the state after it. That is the deletion signal the
local-sync design asked for (`local-sync-mode.md` section 4): when an item is
reassigned away from alice, the transaction that moves it out still reaches
alice's subscriber, carrying the `assignee` retract with its `old_value`, so a
local view can drop the item. The next transaction on that item does not match.

**How it is evaluated.** Membership is not in the frame for a subject whose
type or assignee was set earlier, so it needs a store read. The hub computes it
once per commit, not per subscriber:

1. The hub keeps the set of distinct `member` conditions of live subscribers.
   With none, nothing below runs (no cost for today's consumers).
2. In `read_new`, for each new transaction and each condition, one indexed read
   finds which of the transaction's distinct subjects satisfy the condition as of
   the transaction (after) and as of the previous transaction (before). The read
   uses `idx_current_aev` / the as-of path, bounded by the transaction's subject
   set, never a scan.
3. The result is stored on the ring entry as `{condition -> matching subjects}`.
   Per-subscriber matching stays a pure function of the ring entry, as
   `event-push.md` requires.
4. Catch-up from the store computes the same set for the page with the same
   helper, so ring and store pages stay equal (extend the parity test).

A condition is identified by its normalized `(graph, predicate, value)`, and a
subscription refused if the hub already tracks 64 distinct conditions (a
bounded per-commit cost; the refusal names the limit).

**Graph scope.** A condition is evaluated in one graph, never across graphs:

- with `graph=X`, in X only, so a fact in another graph can never admit a
  subject the subscriber would not otherwise see;
- without `graph`, in the graph of the record being matched (ROOT for ROOT
  records): a record in graph G is admitted only by membership facts in G.

**Value matching.** The value is one RDF term in N-Triples syntax, matched as
an exact term: `<iri>`, `"alice"` (an `xsd:string`), `"5"^^<datatype>`, or
`"x"@lang`. `"alice"` does not match `"alice"^^<other>` or `"alice"@en`. No
case folding, trimming or numeric equivalence (`"5"^^xsd:int` is not
`"5.0"^^xsd:decimal`). The WebSocket takes the same string in a `member`
field. A test holds the typed-literal negative control.

**Catch-up cost.** Predicate and subject filters push down into the store
page (the page query and `records_for_tx` add `a IN (...)` / `e IN (...)` on
term ids), so non-matching transactions are skipped in SQL, as `graph` is.
`member` cannot be pushed down. On the store path it runs on the page the
pushed-down filters already narrowed, with the same batched helper as the hub:
per page (one transaction) at most two indexed reads (before and after) over that
transaction's distinct subjects, chunked at 500 ids. So a far-behind `member`
subscriber costs at most one page read plus two bounded reads per transaction
it walks; it never scans the graph.

## 4. Errors and limits

- A malformed IRI, an over-long list, more than one `member`, or a
  `predicate`/`subject`/`member` on the event stream: 400 on SSE, a
  `quipu.error` frame on the WebSocket with the connection left open (existing
  behaviour for refused messages).
- Existing slot limits are unchanged (64 stream slots, 32 subscriptions per
  WebSocket connection, 1,024 per process).

## 5. Tests

- Graph scope: a membership fact in graph Y does not admit a subject for a
  subscriber with `graph=X`, nor for a record in graph X without `graph`.
- Value matching: `member=<assignee> "alice"` does not match an item whose
  assignee is `"alice"^^<other>`.
- Positive and negative control for the acceptance: two items, assignees A and
  B; a subscriber with `predicate=<status>` and `member=<assignee> "A"` gets
  the status change on A's item and nothing for B's. Then reassign A's item to
  B: the subscriber gets that transaction once, and no later change on it.
- `predicate` and `subject` alone, each with a non-matching control.
- Ring/store parity for every filter, including `member` across a catch-up.
- With no `member` subscriber, a commit costs one hub read (the existing
  `ten_subscribers_cost_one_store_read_per_commit` bound still holds); with one
  or more, at most one extra batched read per condition per commit.
- The SSE and WebSocket frames stay byte-identical.

## 6. Fluent-value subscriptions (later slice)

Subscribing to a fluent's current value needs a current-value read, which does
not exist: fluents are a contract (`docs/design/fluents-v1-contract.md`) and a
pending shape file not loaded (`shapes/MANIFEST`), and PR #1136 adds internal
APIs only. The slice: `{"feed":"fluent","fluent":"<IRI>"}` emits
`quipu.fluent` frames `{fluent, value, tx}` whenever the projected current value
changes, computed in the hub from that fluent's observation records. It starts
when a fluent projection with a current-value read is active on the served
store.

## 7. Change to the local-sync non-goal

`local-sync-mode.md` lists dynamic pattern subscriptions as a non-goal. Stream
filtering by subject, predicate and membership is now in scope (this document);
the before-OR-after rule is the membership deletion signal for push consumers.
Pattern *replicas* (a replica whose content is a pattern slice) stay deferred:
they still need a bootstrap and resync contract beyond the stream.
