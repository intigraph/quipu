# Projection aggregates: time windows, day buckets and a row cursor

**Status:** proposed (2026-10-24). Extends the write-maintained projections
(`src/store/projections.rs`, `projections_aggregate.rs`, `POST /projections`).
It adds no new storage and no new maintenance path: three read-side
parameters and one kind of declared column.

## 1. Problem, measured

A daily work brief asks, for one month: active items by status, items created
per day, items closed per day by priority, plus a validity count. Asked as
SPARQL over a copy of a 1.71M-fact board, each query took 5-13 s. Every one
of them visits every item through several OPTIONALs, and an upper bound of
5 s was the requirement.

The same numbers read from a declared projection (`key` = status, columns
priority, created, closed, identifier, title, keywords) took 22-40 ms per
aggregate on that copy. The projection built in 22.7 s (73,658 rows), and its
triggers added 2.9 ms to the median single-datum board write (14.1 to 17.0 ms;
p99 26.3 to 27.9 ms). The closed-since count matched SPARQL exactly (43,796).

Three things the brief needs cannot be expressed today:

- **No upper bound.** `since` keeps rows at or after an instant; a month
  needs `[start, end)`.
- **No day buckets, one dimension.** `group_by` takes one column. "Closed per
  day by priority" needs a bucketed time column and a second dimension.
- **Rows stop at 1,000 with no cursor.** A month of closures on that board is
  about 44,000 rows (3,000-5,300 on the busiest days), so a client cannot list
  rows and group them itself, and it cannot page either.

## 2. Extension

All three are optional; an existing request means exactly what it means
today.

- **`until: {"column": "<time column>", "at": "<RFC 3339>"}`**: keep rows
  whose column is strictly before the instant. It uses the same epoch-seconds
  column and index as `since`, so `[since, until)` is one index range.
- **Whole-second bounds.** A time column is indexed by whole epoch seconds,
  with a stored fractional second truncated. For a whole-second bound B that
  is exact: x < B exactly when floor(x) < B, and x >= B exactly when
  floor(x) >= B, so a closure at 23:59:59.9 falls before midnight and on the
  right day. A bound with a fractional part cannot be compared that way
  (floor(x) < floor(B) would exclude 00:00:00.2 from "before 00:00:00.5"). So
  `since` and `until` must be whole seconds, and a non-zero fraction is
  refused rather than silently floored. The brief's windows are calendar days,
  so this costs it nothing.
- **Day buckets in `group_by`.** `group_by` may be a list of at most two
  dimensions. Each dimension is a column name, `"key"`, or
  `{"column": "<time column>", "bucket": "day"}`, which groups by the UTC
  calendar day of the column (from its epoch seconds, never by string
  slicing, so offsets and fractional seconds bucket correctly). Groups carry
  `values: [...]` in dimension order; a one-dimension request still returns
  `value` as today.
- **`after` on `rows`**: rows are already returned in a stable entity order.
  A response that is `truncated` also returns `next_after`, the last row
  returned as `{entity, graph}`, and a request with `after` resumes strictly
  after it. The graph is part of the cursor because rows are ordered by
  (entity, graph): with `graphs`, one entity can be a row in each graph, and
  an entity-only cursor would skip the rest of them. Every page
  carries `tx`. A page whose `tx` differs from the first page's is a different
  snapshot, and the client restarts rather than splicing.

- **Presence columns in the declaration**:
  `{"name": "inbox", "predicate": "<IRI>", "equals": <value>}` projects 1
  when the entity currently has that exact value for the predicate and NULL
  otherwise, computed as an `EXISTS` over current facts by the same
  recompute and triggers as every other column. It is needed because an
  ordinary column holds only the LATEST value of its predicate (plus a
  row-level `multi` flag), so for a multi-valued predicate such as keywords
  no filter on that column can answer "has this value". Exact equality only:
  no regular expressions and no case folding.

Validation stays where it is: every name is checked against the declaration
before it reaches SQL, and values are bound.

## 3. The brief's metrics in this API

| metric | request |
|---|---|
| active by status | `aggregate`, `where: {key: [open, hooked, in_progress, blocked, deferred]}`, `group_by: "key"` |
| created per day in the month | `aggregate`, `since`/`until` on `created`, `group_by: [{column: created, bucket: day}]`, inbox excluded by `where` |
| closed per day by priority | `aggregate`, `since`/`until` on `closed`, `group_by: [{column: closed, bucket: day}, "priority"]` |
| validity | `rows` paged with `after`, columns identifier, title, priority, created, closed; the client applies the validity rule |

**Inbox exclusion.** The SPARQL treats an item as inbox when it carries the
`inbox` keyword (compared case-insensitively) or its title starts with
`inbox:` or `mail:`. On the measured board, every case-insensitive match is
exactly the plain string `"inbox"` (68,158 facts), and only 3 items match the
title rule without the keyword. A projection cannot evaluate a regular
expression, so the brief defines inbox by the keyword, through a presence
column: `{name: "inbox", predicate: schema:keywords, equals: "inbox"}`.
Non-inbox items are `where: {inbox: [null]}`. A `keywords` column filtered by
`null` would be wrong: it keeps only items with no keyword at all, and a
latest-value column cannot see one keyword among several. Writers keep the
keyword authoritative, and the 3 items get it.

## 4. Tests

- `[since, until)` is inclusive at the start and exclusive at the end, on
  offset and fractional-second STORED timestamps. A bound with a non-zero
  fractional second is refused; `.000` is accepted.
- Day buckets equal the UTC calendar day computed independently.
- A two-dimension aggregate equals the sum over one-dimension aggregates
  filtered by each value of the other dimension.
- Paging with `after` returns every row exactly once at an unchanged `tx`.
  After an interleaved write, `tx` changes and the client contract says
  restart.
- Differential: each brief metric through the projection equals the
  corresponding SPARQL on the same store (fixture, and the measured copy
  before rollout).

## 5. Rollout and rollback

Source first, and inert: no projection is declared in served configuration
today. Declaring the board projection is a separate configuration change,
with its measured write cost (about +3 ms on the median board write) stated
for the deploy decision. Rollback is to revert the source; the configuration
removal drops the projection's table and triggers on the next start.
