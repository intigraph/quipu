# Design: Structured write provenance, persisted per transaction

> **Status (2026-11-26):** design only (aegis-931w). No runtime change in this
> note. It answers slice M4's generic question from
> the mini-Quipu design (`docs/design/mini-quipu.md`, quipu#1312) §4 rule 4: are the `X-Quipu-*` structured
> provenance fields persisted per transaction, for every store?

## 0. The premise, measured first

The work item says the five fields are only *classified* at commit and not
stored. That is half true, and the half that is false changes the design.

What the code does today (checked on `main` at `dc440180`):

| Thing | Where | Persisted? |
|---|---|---|
| Declared `agent`, `harness`, `host`, `session`, `model` values | `transaction_declarations(tx, declarations)`, JSON, written by `write_provenance::record` inside the transaction's savepoint (#1027) | **Yes**, since #1027 |
| Completeness class (`complete` / `partial` / `absent`) | `RequestProvenance::completeness`, counted as `quipu_write_provenance_total` | No, metric only |
| Authenticated principal, credential id, auth class | `transaction_auth` | Yes |
| The verified attestation's **session** (key binding) | `PendingAttestation::session`, used by `with_verified_identity` | **No** |
| Read surface | `GET /transactions` and `/entity_history` return `authenticated` and `declared` per transaction | Built |
| Logical replication | `store_replica::catalog::DATA` lists `transaction_declarations` | Carried verbatim |
| `pack --full` | `share_completeness::DECLARED` marks it `Content`; the pack refuses any non-`Internal` destination | Carried, internal only |
| As-of share | carries `transactions(timestamp, actor, source)`, not declarations | Not carried |
| Published `pack` / current share | no transaction rows at all | Not carried |
| Merge (`merge_core` receive/decide) | the applier's own request declarations land on the home transaction; the incoming writer's declarations are not read | Not carried |

So the question is not "add a table". The table exists. The open questions are
precedence on read, the missing verified session, merge carriage, and the
outward scrub. Those are what this note decides.

## 1. Decision: persist, and keep the existing table

**Persist.** Keep `transaction_declarations` as the one place declared
provenance is stored. Do **not** persist the completeness class.

Reasons:

1. **Audit needs the values, not the class.** "Which session wrote this?" is
   answerable only from the value. The class is a function of the values plus
   `transaction_auth`, so storing it would store a derived fact that drifts the
   day the class definition changes (for example a new agent harness joins
   `AGENT_HARNESSES`). Store the inputs; derive the class on read with the same
   `RequestProvenance::classify` code that counts the metric.
2. **It must survive a merge, an export and a later audit.** A metric does not
   survive any of them. A row committed in the same savepoint as the facts
   does, and it already rolls back with them.
3. **Generic.** Every store gets the table from `INIT_SQL`. Nothing here is
   mini-Quipu specific.

## 2. Schema, migration and size

### 2.1 Schema (unchanged table, one tightened contract)

```sql
-- Caller declarations, separate from authenticated credential evidence.
CREATE TABLE IF NOT EXISTS transaction_declarations (
    tx INTEGER PRIMARY KEY REFERENCES transactions(id),
    declarations TEXT NOT NULL   -- JSON object, keys ⊆ write_provenance::FIELDS
);
```

Contract on the JSON object, stated so a reader can rely on it:

- Keys are a subset of `write_provenance::FIELDS`: `agent`, `harness`, `host`,
  `session`, `model`. No other key is ever written.
- A value is present only when `declared()` held (a printable character).
- Value cap: the header contract (aegis-7zp4rc) is printable ASCII, at most
  128 characters. `classify` currently keeps values up to 1024 bytes. Slice
  P1 lowers the write cap to the header contract; readers keep accepting up to
  1024 for rows already written.

**Rejected: five typed columns.** They would make a key added to `FIELDS` a
schema migration, and the JSON is never queried by value inside SQL. If a
by-value query is needed later (for example "all writes from host X"), add an
expression index on `json_extract`, not columns.

**One new column, on the evidence table, not the declaration table:**

```sql
ALTER TABLE transaction_auth ADD COLUMN attested_session TEXT;  -- NULL unless auth_class = 'attested_session'
```

The verified binding supplies agent *and* session, but today only the
principal and key id (`credential_id`) reach `transaction_auth`. The session
is lost at commit, so precedence (§3) cannot be applied on read for session.
It belongs in `transaction_auth` because it is local credential evidence:
excluded from shares (`share_completeness`), `LOCAL` in replication, exactly
like the principal.

### 2.2 The three states, and migration

| Observation | Meaning |
|---|---|
| Table absent (read-only open of a store older than #1027) | `UNKNOWN`: the store never recorded declarations |
| Table present, no row for `tx` | `UNKNOWN`: either the transaction predates #1027, or it was written in-process with no request scope (CLI, background job) |
| Row with `{}` | The request was observed and declared **nothing** (class `absent`) |
| Row with keys | Those keys were declared; missing keys were not |

Migration for existing stores is the one already in place: `CREATE TABLE IF
NOT EXISTS` on a read-write open; read-only opens do not migrate and
`Store::transaction_declarations` returns `None`. **Never back-fill.** A
historical row is not inferred from `actor`, `source`, `transaction_auth` or
any log; the code comment "never infer a host from actor" stays the rule.
`attested_session` is likewise added nullable and left NULL for existing rows.

### 2.3 Size, measured on a disposable copy-shaped fixture

Fixture: a fresh SQLite file with the real `transactions`, `transaction_auth`,
`transaction_declarations` and a `facts` table with its two indexes; realistic
declarations (400 ephemeral agents plus services, 4 hosts, 3000 session UUIDs,
3 models; session and model only on the `claude` harness), `VACUUM`ed, built
once with and once without declaration rows, then deleted.

| Transactions | Facts per tx | Without | With | Delta |
|---|---|---|---|---|
| 100,000 | 6 | 86.14 MB | 100.86 MB | 14.72 MB, **147 B/tx**, 17.1% |
| 100,000 | 1 | 26.39 MB | 41.11 MB | 14.71 MB, 147 B/tx, 55.7% |

The cost is per transaction, not per fact: about 147 bytes, roughly the JSON
length plus row overhead. The relative cost depends only on how many facts a
transaction carries, which this fixture did not measure on a live store
(`UNKNOWN`). A dictionary-interned shape would be smaller but was not measured
and is not proposed; revisit only if a live store's measured delta exceeds an
agreed budget.

## 3. Attestation precedence

The rule already in `write_provenance.rs`, applied on **read** as well as at
the metric:

1. When `transaction_auth.auth_class = 'attested_session'`, the verified
   binding is the source of **agent** (the principal) and **session**
   (`attested_session`). A declared `agent` or `session` on the same
   transaction is reported, marked `superseded`, and never shown as the
   effective value. A header never earns that credit.
2. Otherwise all five fields are **declared**: untrusted caller statements,
   reported as such and never as authenticated identity.
3. `harness`, `host` and `model` are always declared, attested or not.
4. Completeness on read is `RequestProvenance::classify` over the stored
   values, then `with_verified_identity` when rule 1 applies. One function,
   so the read and the metric cannot disagree.

Read shape per transaction (additive to today's `authenticated` and
`declared`):

```json
"provenance": {
  "effective": {"agent": {"value": "…", "basis": "attested"},
                "session": {"value": "…", "basis": "attested"},
                "harness": {"value": "claude", "basis": "declared"}},
  "superseded": {"agent": "…"},
  "completeness": "complete",
  "state": "observed"
}
```

`state` is `observed`, `unknown-no-row` or `unknown-no-table` (§2.2).

## 4. Read surface and grants

- **Endpoints.** `GET /transactions` and `/entity_history` (and their MCP
  tools `quipu_transactions`, `quipu_entity_history`) already return
  `declared`. They gain the `provenance` object of §3. No new endpoint.
- **Grant.** Declarations are readable under the same grant as the
  transaction's `actor` and `source`: whoever may read that transaction in
  [graph read authorization](graph-read-authorization.md) may read its
  `agent`, `harness` and `model`.
- **`host` and `session` are operational.** They are returned only to an
  authenticated reader (a named bearer or an attested session); an
  anonymous or legacy shared-bearer read receives `"redacted"` in their place,
  never an omission (an omission would read as "not declared").

## 5. Merge, export and replication

The invariant for every path: **the home principal stays the home
authenticated principal.** Carried provenance is data about the origin, never
identity on the home transaction.

### 5.1 Merge (`merge_core` receive / decide)

- The `receive` and `decide` transactions take their own
  `transaction_declarations` row from the applier's request scope, through the
  normal pipeline, and their `transaction_auth` row from the applier's
  credential. Unchanged from [merge behaviour](merge-behaviour.md) §3.3.
- The incoming writer's provenance is **carried**: for each incoming origin
  transaction, its declarations are read from the *source* (the fork's own
  `transaction_declarations`, the pendant or as-of share's carried rows) and
  recorded with the per-fact origin `fork_tx` / `actor` / `source` in the merge
  decision rows and receipt, under a `carried` key keyed by origin tx.
- Carried values are never written into the home transaction's
  `transaction_declarations` or `transaction_auth`, and the source's
  `transaction_auth` is never carried as evidence (it is local, `Excluded`).
- Request headers on the merge call affect only the applier's own row. This is
  M4's acceptance: a merge request with forged headers changes no carried
  field.
- A published share has no transactions, so its carried provenance is
  `UNKNOWN`, not empty.

### 5.2 Share and pendant export

| Artifact | Declarations carried |
|---|---|
| `pack --full` (internal only, already refuses outward) | verbatim, all keys |
| Logical replication (`DATA` catalog) | verbatim, all keys |
| As-of share, `Internal` destination | proposed: all keys, per carried tx |
| As-of share or pendant, `Outward` or `Audience` destination | proposed: `agent`, `harness`, `model` only; `host` and `session` removed (§6) |
| Published `pack` / current share | none (no transactions travel) |

### 5.3 Replication

Logical replication already copies `transaction_declarations` in `DATA` and
keeps `transaction_auth` in `LOCAL`. That stays. The new
`attested_session` column is part of `transaction_auth`, so it stays local
too. A follower shows replicated declarations as `declared` and its
`authenticated` as absent: replication never manufactures attestation.

## 6. Privacy: the scrub rule

**Rule.** For every destination that is not `ShareDestination::Internal`
(that is, `Outward` and `Audience`), the `host` and `session` keys are removed
from every carried declaration **structurally**, before serialization, by the
producer, and the manifest records `"declarations_scrubbed": ["host",
"session"]` so a receiver can tell scrubbed from undeclared.

- This is a field rule, not a pattern. It does not depend on the
  `InternalIdentifierPattern` catalogue, which may be empty on a store. The
  existing block-tier regex scrub (`share_scrub::enforce_destination`) still
  runs afterwards over the whole payload.
- It is applied in one helper next to `enforce_destination`, so a new
  producer cannot forget it any more than it can forget the regex scrub.
- A receiver that finds `host` or `session` in a non-internal artifact refuses
  the import, the same way `share_attestation` refuses self-registration.

**Residual risk, stated rather than solved.** `transactions.actor` and
`transactions.source` are free text and already travel in as-of shares.
Crew process rules put a session ID into `source` for session captures, so a
session can leak through `source` today. The structural rule cannot parse free
text; the regex catalogue is the guard there, and a deployment that wants
session IDs blocked should register a block-tier pattern for them. Quipu stays
generic and does not hard-code a session-ID shape.

## 7. Proposed implementation slices

| Slice | Change | Falsifiable acceptance |
|---|---|---|
| P1 | Lower the write cap in `classify` to the header contract (printable ASCII, 128); readers still accept 1024 | A 129-character header is not stored; an existing 1024-byte row still reads |
| P2 | `transaction_auth.attested_session` (nullable), written when `auth_class = 'attested_session'` | A signed write's tx reads back its binding session; a bearer write's is NULL; pre-migration rows NULL |
| P3 | Read-side `provenance` object (§3) on `/transactions` and `/entity_history`, via `classify` + `with_verified_identity` | A signed write with a forged `X-Quipu-Session` shows the binding session as effective and the header as superseded; read class equals the metric's class |
| P4 | Grant split (§4): `host`/`session` redacted for anonymous and legacy shared-bearer reads | Anonymous read shows `"redacted"`; named-bearer read shows the value |
| P5 (built, aegis-2ndq.2) | Structural outward scrub helper and receiver refusal (§6), plus as-of share carriage of declarations (§5.2) | An outward as-of share contains no `host`/`session` key and its manifest says so; an internal one contains them; an outward artifact carrying `session` is refused on import |
| P6 | Merge carriage (§5.1) in `merge_core` receive/decide rows and receipt | Fork merge: home tx principal = applier; carried declarations equal the fork tx's; repeating the merge with forged headers changes no carried field (M4) |

Order: P1 and P2 are independent; P3 needs P2; P5 and P6 need P3's shared
composition helper. None of them changes the stored meaning of an existing row.

**P5 as built (aegis-2ndq.2).** The field rule is
`share_scrub::scrub_declarations` / `declarations_scrubbed` beside
`enforce_destination`, and the receiver half is
`share_scrub::refuse_unscrubbed`, called by the bootstrap applier per record.
`POST /share/asof` takes an optional `destination` (default `outward`); each
record carries `declared` per origin tx. Two limits, stated: the as-of cut does
not run the block-tier regex scrub (it never has; it is a write-class replica
bootstrap), and a frozen graph's records carry no declarations (the pack holds
none), which reads as UNKNOWN. An outward manifest now names
`declarations_scrubbed`, so a follower built before P5, whose typed manifest
refuses unknown fields, must be upgraded before it bootstraps from a P5 source.
