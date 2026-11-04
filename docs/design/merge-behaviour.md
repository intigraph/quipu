# Design: merge behaviour, use cases -> expected outcomes (the merge oracle)

**Status:** design for aegis-karb.1 (2026-11-04), epic aegis-karb. Extends
[merge-reconcile.md](merge-reconcile.md) (aegis-k7l): that document defines how a
person or an agent *decides* a conflict. This one defines what the graph must
look like *after* every kind of merge, decided or not. Measured against quipu
main `dafe8bfe`.

Implementation is aegis-karb.2 (one write core). The paper re-run is aegis-karb.4.
Both use this document as their oracle.

## 1. Why an oracle that does not read the shapes

Today the merge decides value-by-value behaviour from the SHACL shapes
(`merge3::ShapeBounds`, `sh:maxCount`). The paper then judges the merge with the
same shapes: a merge counts as "correct" when it does not violate them.
Merge and judge then read one source, so they cannot disagree, and a missing
or wrong shape is invisible to both. Stiwi, 2026-11-03: *the use cases, not
SHACL, define correctness.*

So every fixture here **states the intended cardinality of each slot itself**
(`card: one | many(n) | list`). The system under test still derives behaviour
from its shapes. When the two disagree, the test fails, and the failure points
at a shape gap or a merge bug. Either way it is a real defect. An assertion
in this document **never reads the shapes graph**. It reads only current
state, valid-time and transaction-time reads, the transaction log and the merge
report.

## 2. Vocabulary

- **Slot**: `(subject, predicate)` and its value set, as in `src/merge3.rs`.
- **Sides**: `base` (the common ancestor), `ours` (ROOT, or Git `HEAD`),
  `theirs` (the share, the fork, or the merged branch).
- **Change kinds per slot**: *unchanged*; *one-side* (exactly one side differs
  from base); *convergent* (both differ from base, identically); *divergent*
  (both differ from base, differently).
- **Merge id**: the SHA-256 of one canonical record of the merge's inputs: the
  path, the incoming identity (share id, `fork name@fork_tx`, or the Git
  `MERGE_HEAD` commit), the base identity (parent share id, the fork's
  `fork_tx`, or the Git merge base), and the SHA-256 of the decisions file
  (the empty decision set for a clean merge). It ties every transaction of
  one merge together and names one merge of one input. Two different merges
  never share an id, including two clean ones; the same input merged again
  has the same id, which is what makes R7 work. ROOT's state is deliberately
  not part of the id: staleness is checked separately (C8b).
- **Fixture times** (UTC, fixed so assertions are exact):

  | Name | Value | Meaning |
  |---|---|---|
  | `T0` | `2026-01-01T00:00:00Z` | base value valid from |
  | `To` | `2026-01-02T00:00:00Z` | ours changed the slot (ROOT write time) |
  | `Tt` | `2026-01-03T00:00:00Z` | theirs changed the slot (its *authored time*, §3.1) |
  | `Tf` | `2026-01-04T00:00:00Z` | the fork closed a value (case C7) |
  | `Tr` | `2026-01-05T00:00:00Z` | ROOT re-claimed a value after the fork (case C7) |
  | `Td` | `2026-01-10T00:00:00Z` | the apply call: the receive and decision time |

Valid time is half-open, `[from, to)`. That is the store's visibility predicate
(`valid_from <= t AND (valid_to IS NULL OR valid_to > t)`, `src/store/reads.rs`).
A row closed at its own start, `[t, t)`, is therefore **visible at no valid
time** but stays in transaction history. Directive 2 relies on this.

## 3. Rules every use case shares

Stiwi's directives of 2026-11-03 are restated here as rules R1-R8. The cases in §5
apply them.

- **R1 One operator.** The value set of every slot is the three-way rule of
  `merge3::merge_slots`. An unchanged side defers to the other side. Agreeing
  sides need no merge. A divergent slot is merged value by value, each side's
  removals removed and each side's additions added. It conflicts only by
  cardinality (R4). The only per-path differences are inputs (§3.1) and the
  output target (store or pendant file). Merge logic never differs by path.
- **R2 Convergent is flagged.** A convergent slot (ours == theirs != base,
  including both sides deleting) produces a report row of kind `convergent` with
  default decision `continue`. `continue` writes nothing, because ROOT already
  holds the value. It never blocks an apply. The operator may override with
  `base` or `value`, which are then ordinary decisions (R5).
- **R3 Sequenced decisions.** A decided conflict is written as up to two
  transactions in this order, all or nothing (§3.2):
  1. `receive`: the clean part of the merge, plus every incoming value of a
     decided slot recorded as a **receipt**: an assertion with the empty
     interval `[Td, Td)`. It stays in transaction history and is current at
     no valid time, so a loser never holds valid time;
  2. `decide`: the decisions. A losing ours value is closed at `Td`. Every
     chosen value ROOT does not hold, a received winner included, is asserted
     as a live row from `Td`.

  (Ruling B, malcolm on aegis-karb.2, 2026-11-04. An earlier draft received
  incoming values as open assertions and retracted the losers in `decide`.
  The per-transaction write gate refuses that, and exempting the receive step
  from the gate was rejected.)
  An empty step is not written. A clean merge with no decided slot is one
  `receive` transaction.
- **R4 Cardinality from the shape.** `card: one` (maxCount 1) is decided as a
  whole slot. `card: many(n)` is a set: union, duplicates impossible, and a
  conflict only when the merged set exceeds `n`. `card: many` with no bound
  never conflicts. `card: list` merges as one unit or as a sequence, never link
  by link (case C6).
- **R5 No rewriting of ours.** A losing ours value is closed at the decision
  time `Td`. Its earlier valid history stands. A merge never makes a ROOT row's
  interval empty or reversed. Only a received-and-rejected incoming row may be
  empty.
- **R6 Interval sanity.** No row ever has `valid_to < valid_from`. After a
  merge, a `card: one` slot never holds two values whose intervals overlap, read
  at the latest transaction, and at no transaction-time pin either: receipts
  are never open (R3).
- **R7 Idempotent.** Re-applying the same decisions writes nothing and reports
  `already-applied` with the original transaction ids.
- **R8 Counts.** Every apply reports `applied N` (slots written) and `held M`
  (slots left undecided or held by a partial mode, §6). Atomic mode reports
  `held M` with `applied 0` when anything is held.

### 3.1 Inputs per path: the authored time of an incoming value

The per-path difference the epic allows is how much time and provenance the
incoming side carries:

| Path | Authored time `Tt` of an incoming assertion | Close time of an incoming removal | Provenance carried |
|---|---|---|---|
| Fork promote | the fork fact's own `valid_from` | the fork's latest closed row for that value | fork name, `fork_tx`, per-fact tx/actor/source |
| Share reconnect | the incoming share's `created_at` (its anchor tx time: "known by", `src/share.rs`) | the incoming share's `created_at` | share id, parent share, store id, attestation |
| Pendant merge | none (the output is a file without valid time) | none | Git commit ids |

**Where the incoming time applies.** Use `Tt` when nothing ROOT holds is
displaced over `[Tt, Td)`, other than a base-identical value that this
incoming change supersedes. That covers a one-side change (ours equals base,
so theirs authored the only change, at `Tt`) and a set-union addition. In a decided conflict an incoming value is received from `Td`
instead. Ours held the slot until the decision, so an earlier start would
overlap it (R6). `Tt` is still recorded, in the decision row and the `receive`
source.

**Clamp (case C7).** A close taken from the incoming side is applied only if
it falls inside the ROOT row it closes. If it is earlier than that row's
`valid_from`, the row closes at `Td`. A close **later** than `Td` (a
future-dated close on the incoming side) is applied as is: it reverses no
interval and empties no row, and it is what the incoming side authored (case
C7b). A merge never produces `valid_to < valid_from` (R6), and never empties
a ROOT row (R5).

### 3.2 Atomicity and the write gate

The `receive` and `decide` transactions commit under one writer reservation
(one savepoint), together or not at all. Each step passes the ordinary
per-transaction write gate unchanged: because receipts are never open, no step
holds two open values on a `card: one` slot, and no gate exemption exists.
Measured on main `af460814`: an empty-interval receipt passes a maxCount-1
gate, and an open second value is still refused (fixture C4-gate).

### 3.3 Provenance on every merge transaction

| Field | `receive` | `decide` |
|---|---|---|
| `actor` | the applier (the reviewer when there are decisions) | the reviewer |
| `source` | `merge:<path>:receive;merge=<merge id>;incoming=<share id or fork name@fork_tx>` | `merge:<path>:decide;merge=<merge id>;decisions=<sha256>;reviewer=<who>` |

`<path>` is `share`, `fork` or `pendant`. Both transactions carry the same merge
id, so an oracle can pick out one merge's transactions from the log. The exact
string format is karb.2's choice. The oracle checks only that these fields are
present and that both transactions share the merge id.

## 4. Assertion vocabulary (what a fixture checks)

All reads are against the store after the merge, or against the pendant file
for the pendant path.

| Assertion | Reads | Meaning |
|---|---|---|
| `CUR(s) = {..}` | current state | values of slot `s` now |
| `VT(s, v) = [a, b) ...` | every row of `(s, v)` at the latest tx | valid intervals of `v`, ordered by start; `[a, a)` is an empty row; `[a, -)` is open |
| `AT(s, tx=k, vt=t) = {..}` | as-of read | values at valid time `t` as known at transaction `k` |
| `TXS = [...]` | transaction log, filtered by merge id | each step's role, asserted and retracted `(s, v)`, `actor`, and the source fields of §3.3 |
| `REPORT = [...]` | the merge result / decisions file | per row: slot, `kind` (`convergent`, `maxcount-exceeded`, `delete-replace`, `list-divergent`), decision, and the `applied`/`held` counts |
| `PACK(s) = {..}` | the merged pendant `export.nt` | pendant path only |
| `INV` | the whole store | R5, R6, no duplicate current `(s, v)` row, R7 on a second apply |

A fixture is one row in a §5 table plus its inputs. In YAML form, ready for
karb.2:

```yaml
case: C4-theirs
path: [fork, share]           # pendant has its own column; see U3
card: one                     # stated here, never read from shapes
slot: [ex:t42, ex:status]
base:   [{v: '"open"',    from: T0}]
ours:   [{v: '"blocked"', from: To}]   # ROOT closed "open" at To
theirs: [{v: '"closed"',  from: Tt}]
decision: theirs
expect:
  CUR: ['"closed"']
  VT:
    '"open"':    ['[T0,To)']
    '"blocked"': ['[To,Td)']
    '"closed"':  ['[Td,-)']
  TXS:
    - {role: receive, assert: ['"closed"@Td']}
    - {role: decide,  retract: ['"blocked"@Td']}
  REPORT: [{kind: maxcount-exceeded, decision: theirs}]
  counts: {applied: 1, held: 0}
```

## 5. Cases

Every case runs on the fork and share paths. The pendant path is checked
time-free in §7 (U3). "Today" is quipu `dafe8bfe`. **DIFF** marks
behaviour karb.2 must change. "INFERRED" means read from code, not yet run.

### C1. One-side change

Slot `card: one`, base `a@T0`.

| # | ours | theirs | CUR | VT | TXS | Today |
|---|---|---|---|---|---|---|
| C1a | `a` (unchanged) | `c@Tt` | `{c}` | `a:[T0,Tt)`, `c:[Tt,-)` | 1 `receive`: assert `c@Tt`, retract `a` closing at `Tt` | fork MATCH. **DIFF** share: asserts `c` and closes `a` at merge time `Td` |
| C1b | `b@To` | `a` (unchanged) | `{b}` | unchanged | none (nothing to write) | MATCH |
| C1c | `a` | `∅`, closed at `Tt` | `{}` | `a:[T0,Tt)` | 1 `receive`: retract `a` closing at `Tt` | fork MATCH. **DIFF** share closes at `Td` |

For share, `Tt` is the incoming share's `created_at` (§3.1).

### C2. Convergent change

| # | card | base | ours | theirs | CUR | TXS | REPORT | Today |
|---|---|---|---|---|---|---|---|---|
| C2a | one | `a` | `c@To` | `c@Tt` | `{c}`; `VT(c)=[To,-)` (ROOT's row stands) | none | `convergent`, `continue` | **DIFF**: no row, taken silently (`merge3.rs:85`) |
| C2b | one | `a` | `∅` | `∅` | `{}` | none | `convergent`, `continue` | **DIFF** |
| C2c | one | `a` | `c@To` | `c@Tt`, override `value d` | `{d}`; `c:[To,Td)`, `d:[Td,-)` | 1 `decide`: retract `c@Td`, assert `d@Td` | `convergent`, `value` | **DIFF** (new) |

A convergent row never blocks an apply, in atomic mode too. Counts:
`applied 0, held 0` for C2a/C2b. A `continue` is not an applied slot.

### C3. Set-valued divergent change

Slot `card: many` (unbounded, or a bound not exceeded).

| # | base | ours | theirs | CUR | VT of the incoming change | TXS | Today |
|---|---|---|---|---|---|---|---|
| C3a | `{a}` | `{a,b}` | `{a,c}` | `{a,b,c}` | `c:[Tt,-)` | 1 `receive`: assert `c@Tt` | fork MATCH; **DIFF** share time |
| C3b | `{a}` | `{b}` | `{c}` | `{b,c}` | `a` already closed by ROOT at `To`; `c:[Tt,-)` | 1 `receive`: assert `c@Tt` | as C3a |
| C3c | `{a}` | `{a,b}` | `{}` (closed `a` at `Tt`) | `{b}` | `a:[T0,Tt)` | 1 `receive`: retract `a` at `Tt` | as C3a |
| C3d | `{}` | `{d}` | `{d,e}` | `{d,e}` | `d`: ROOT's single row stands, no duplicate; `e:[Tt,-)` | 1 `receive`: assert `e@Tt` only | MATCH |
| C3e | `{a}` | `{a,b}` | `{a,c}`, `many(2)` | conflict: union `{a,b,c}` exceeds 2 | see C4 | | MATCH (detection) |

`INV`: no two current rows of the same `(s, v)`. That is "duplicates are
impossible" in checkable form.

### C4. Divergent `card: one` conflict, by decision

Base `a@T0`, ours `b@To` (ROOT closed `a` at `To`), theirs `c@Tt`. Kind
`maxcount-exceeded`. In every decided row, `VT(a) = [T0, To)` stands.

| Decision | CUR | VT `b` | VT `c` | VT other | TXS | Today |
|---|---|---|---|---|---|---|
| `ours` | `{b}` | `[To,-)` | `[Td,Td)` (receipt) | | `receive`: receipt `c@Td`; no `decide` | **DIFF**: `c` never enters the graph |
| `theirs` | `{c}` | `[To,Td)` | `[Td,Td)` (receipt), `[Td,-)` (live) | | `receive`: receipt `c@Td`; `decide`: retract `b@Td`, assert `c@Td` | share: one tx at `Td`. **DIFF** fork: `c` keeps fork `valid_from` `Tt` while `b` closes at `Td`, so `b` and `c` overlap in `[Tt,Td)`. Measured 2026-11-04 on `dafe8bfe` with a throwaway test through `fork_promote_resolved`: ROOT rows `ours [To,Td)` and `theirs [Tt,-)` (`forks_merge.rs:198`, plus the retime skip for a value the fork never held) |
| `base` | `{a}` | `[To,Td)` | `[Td,Td)` (receipt) | `a: [T0,To), [Td,-)` | `receive`: receipt `c@Td`; `decide`: retract `b@Td`, assert `a@Td` | **DIFF**: one tx, no receipt |
| `both` | refused: 2 values > maxCount 1; nothing written | | | | none | MATCH (refusal) |
| `value d` | `{d}` | `[To,Td)` | `[Td,Td)` (receipt) | `d:[Td,-)` | `receive`: receipt `c@Td`; `decide`: retract `b@Td`, assert `d@Td` | **DIFF**: one tx |
| `defer` | atomic: nothing written, `held 1`. Partial modes: §6 | | | | none | MATCH (atomic) |

For `card: many(n)` exceeded (C3e), the same table applies. `both` is offered
only when `|ours ∪ theirs|` fits `n`. When it fits, `CUR = ours ∪ theirs` and
the incoming additions are received as receipts and asserted live from `Td`.

**C4-gate, the gate still binds.** Run C4 `theirs` with a gating
`sh:maxCount 1` shape loaded. Expect the apply to succeed with the outcome
above, and `AT(s, tx=receive, vt=Td) = {b}`: the receipt is in the `receive`
transaction and current at no valid time. Sabotage control: the same receipt
written WITHOUT its `valid_to` (an open second value) must be refused by the
gate. Further checks for the receipt (malcolm's conditions): the winner's
re-assert after a receipt is a live row (not deduplicated against the
receipt); the changes feed, the replica applier and as-of readers treat an
assertion with `valid_to = valid_from` as never current.

### C5. Delete vs replace

| # | card | base | ours | theirs | Outcome | Today |
|---|---|---|---|---|---|---|
| C5a | one | `a` | `∅` (closed `To`) | `c@Tt` | conflict, kind `delete-replace`. `ours`: `CUR {}`, receipt `c:[Td,Td)`. `theirs`: `CUR {c}`, `c: [Td,Td), [Td,-)`. `base`: `CUR {a}`, `a:[T0,To),[Td,-)`, receipt `c:[Td,Td)` | MATCH (detection); **DIFF** sequencing |
| C5b | one | `a` | `c@To` | `∅` (closed `Tt`) | conflict, `delete-replace`. `ours`: `CUR {c}`, no `receive` (nothing incoming). `theirs`: `CUR {}`, `c:[To,Td)` (R5: closed at `Td`, not `Tt`) | MATCH (detection); **DIFF** sequencing |
| C5c | many | `{a}` | `{}` | `{c}` | no conflict: `CUR {c}`, `c:[Tt,-)` | MATCH (value set) |

### C6. List-shaped property

A `card: list` slot holds the head of an `rdf:List`. Its value is the
**element sequence**, never the blank-node cells. Lists are compared by
sequence, so cell labels do not matter (the paper notes blank-node alignment
is untested, `ARM-E-F-RESULTS.md`). The shape declares one of two modes. The
fixture declares the same mode itself.

| # | mode | base | ours | theirs | Outcome | Today |
|---|---|---|---|---|---|---|
| C6a | whole | `(x y)` | `(x y)` | `(x y z)` | one-side: `CUR` is the list `(x y z)` | **DIFF**: merged cell by cell |
| C6b | whole | `(x y)` | `(y x)` | `(x y z)` | conflict, kind `list-divergent` (a list is one unit) | **DIFF** |
| C6c | whole | `(x y)` | `(x y z)` | `(x y z)` | convergent (R2), sequence equality | **DIFF** |
| C6d | sequence | `(x y)` | `(w x y)` | `(x y z)` | diff3 on sequences: `(w x y z)` | **DIFF** |
| C6e | sequence | `(x y)` | `(x q)` | `(x r)` | overlapping edit: conflict `list-divergent` on the region | **DIFF** |

`INV` for lists, checked structurally without shapes: every cell has exactly
one `rdf:first` and one `rdf:rest`, the chain ends at `rdf:nil` with no cycle,
and no cell is orphaned (unreachable from a head). A received-and-rejected list
is recorded whole, and its cells are retracted whole.

### C7. ROOT re-claims the same value (aegis-0s2a)

Fork path. Base `a@T0`. The fork closes `a` at `Tf`. After the fork, ROOT
closes `a` and asserts it again at `Tr > Tf`, so ROOT's current row is
`a:[Tr,-)`. By value set, ours equals base and only theirs changed (one-side,
R1).

| Expect | Today |
|---|---|
| `CUR {}`. ROOT's row closes at `Td`: `VT(a) = [T0, ...), [Tr, Td)`. The fork's close `Tf` is earlier than the row's start `Tr`, so the clamp of §3.1 applies | **DIFF** (bug): retime closes the row at `Tf`, giving `valid_to Tf < valid_from Tr`, a row visible at no time (`forks_merge.rs:324`) |

*Alternative considered and rejected:* closing at `Tr`, which empties ROOT's
row. That would erase ROOT's own later claim from valid time and break R5.

**C7b, a future-dated incoming close.** Base `a@T0`, ours unchanged, and the
fork closes `a` at `Tz = 2026-02-01T00:00:00Z`, later than `Td`. Expect:
`VT(a) = [T0, Tz)`, `AT(s, vt=Td) = {a}`, `AT(s, vt=Tz) = {}`, and one
`receive` transaction closing `a` at `Tz`. (Stated as valid-time reads, not
`CUR`, because the fixture times are fixed dates and `CUR` reads the wall
clock.)

### C8. Idempotence and staleness

| # | Action | Expect |
|---|---|---|
| C8a | apply the same decisions twice | the second apply returns `already-applied` with the first apply's tx ids; transaction count unchanged |
| C8b | ROOT changes a reviewed slot between emit and apply | refused as stale, nothing written (merge-reconcile.md §3.2) |
| C8c | apply a clean merge twice (share) | the second apply writes nothing (the graph already equals the merged state) |
| C8d | two different clean merges (two incoming shares, no decisions) | two different merge ids; filtering the log by either returns only its own transactions; the second is not reported `already-applied` |

## 6. Partial merge (Stiwi 2026-11-03, aegis-karb.1 comment 1)

Mode per graph, overridable per call. **Default `atomic`.**

| Mode | Applies | Holds | Writes |
|---|---|---|---|
| `atomic` | everything, or nothing if any slot is undecided | every slot when one is undecided | 0 txs when anything is held |
| `partial-entity` | every entity with no undecided slot | each entity with an undecided slot, whole, at base | `receive` (and `decide`) for the applied part |
| `partial-slot` | every slot that is clean or decided | only the undecided slots | as above |

A held slot is **not received**: its incoming values are written nowhere. It
becomes a pending decision row bound to the partial apply's `receive` tx. A
later apply of the remainder is its own sequenced merge with a new merge id
that names the earlier one. Re-applying either part is a no-op (R7). The
report gives `applied N, held M`.

Fixture P1 (partial-slot): two conflicting `card: one` slots `s1`, `s2` on
entity `e`, one clean slot `s3` on entity `f`, and `s1` decided `theirs`.
Expect `s1` and `s3` written, `s2` at base, `held 1`, a pending row naming the
`receive` tx. In `partial-entity` mode: `s3` written, `s1` and `s2` held
(same entity), `held 2`. In `atomic` mode: nothing written, `held 1`.

**When partial is unsafe: merge units.** Some slots only make sense together:
start and end, a list's members, an `sh:node` value structure. Moving one
without the other leaves an inconsistent entity. A merge unit always moves as
one: all its slots apply, or all are held as one decision row.

1. *Preferred, RDF-native:* model coupled fields as one structured value (a
   node shaped with `sh:node`). The slot then holds a single value, and the
   merge already treats it as a unit. The value node needs stable identity
   (skolemised or content-hashed).
2. *When the data must stay flat:* a NodeShape annotation,
   `quipu:mergeUnit [ quipu:paths ( ex:start ex:end ) ]`.
3. *Not* `sh:group`. It is a presentation grouping in SHACL with no validation
   meaning, and reusing it would change behaviour for shapes that use it for
   forms.

Fixture P2: a `quipu:mergeUnit (start end)`. Theirs changes `start` alone, but
ours changed `end` in a conflicting way. Expect both slots held in one row in
both partial modes. A fixture that changes only `start` on one side applies it
(a one-side change inside a unit is still clean). A constraint over the unit
(`start <= end`, `sh:lessThanOrEquals`) is checked after the merge by the
normal write gate. The oracle checks the same inequality directly on the
values.

## 7. Use cases

Each use case runs C1-C8. These are the use-case-specific expectations and
the competency questions. The questions are parseable by camayoc
`scripts/competency.py` in
[`docs/competency/merge-behaviour.md`](../competency/merge-behaviour.md).

### U1. Share reconnect between peers (`quipu merge`, `/merge/apply`)

- Inputs: base share (the parent), incoming share, ROOT. Authored time =
  incoming `created_at`.
- Output: ROOT transactions per R3, with the share id in the `receive` source.
- Specific: a peer that receives the same share twice gets C8c. Two peers that
  each merge the other's share converge to the same `CUR` for every clean slot.
  Fixture U1-sym checks this by running the merge in both directions.

### U2. Fork promote of an agent scratch fork into ROOT (`quipu fork promote`)

- Inputs: base = ROOT as of `fork_tx`, ours = ROOT now, theirs = the fork.
  Authored time = the fork fact's `valid_from`, and close time = the fork's
  close row.
- Output: ROOT transactions per R3, with `fork name@fork_tx` in the source.
  The fork stays open after a refused or held promote.
- Specific: C7. Fork-promote decisions carry the fork's per-fact provenance
  into the decision row.

### U3. Pendant merge on a Git branch or PR (`quipu git-merge`, `merge-driver`)

- Inputs: Git base, `HEAD` and `MERGE_HEAD` pendants.
- Output: the merged pendant file and `decisions.json`. There is **no
  valid time and no store transaction**. The Git merge commit is the history,
  and its second parent is the received side (the audit that R3 gives the
  store).
- Assertions: `PACK(s)` equals each case's `CUR`, time-free. `decisions.json`
  lists conflicts *and* convergent rows (R2: a `decisions.json` schema bump).
  CI `pendant-check` recomputes it byte-for-byte. A list slot (C6) is
  serialised in one canonical cell labelling, so the bytes reproduce.
- R3 and R5 do not apply (no time). R1, R2, R4, C6 and C8 do.

### U4. Server conditional writes: an open question

Today `/set` and conditional writes (`store/conditional.rs`, compare-and-set)
are two-way. A mismatch is a bare refusal. The question from the epic: should
a mismatch (expected value = base) produce the same decision row?

| Option | Behaviour | Cost |
|---|---|---|
| A (recommended now) | On a mismatch, the refusal body includes a `DecisionRecord`-shaped row: `base` = the expected value, `ours` = the current value, `theirs` = the proposed value, plus the `kind`. Nothing is written. The refusal event is kept | Small, additive. Aligns with aegis-061ugg (the conflict response names the base) |
| B (karb.6) | The single writer runs the three-way merge: each write declares its base tx, a clean slot applies and a conflicting slot becomes a pending row (partial-slot by nature) | The single-writer change, sized by the karb.7 census |
| C | Keep the bare refusal | No change. Agents keep re-reading and retrying blind |

**Recommendation:** A now, B under karb.6. Fixture U4-A: base `a`, current `b`,
conditional write expecting `a` and proposing `c`. Expect: refused, `CUR {b}`,
the response row has `base [a]`, `ours [b]`, `theirs [c]`, and no transaction.

## 8. What karb.2 must change (summary of DIFF rows)

1. Share times: assert and close incoming one-side changes at `created_at`, not
   at merge time (C1, C3).
2. A convergent row with `continue` (C2), in the report, the decisions file and
   the pendant `decisions.json`.
3. Sequenced `receive`/`decide` transactions, with every incoming value of a
   decided slot received as a receipt `[Td, Td)` (C4, C5).
4. A fork that wins a decided conflict starts at `Td`, not at its fork
   `valid_from` (C4 `theirs`, measured overlap). Clamp closes (C7, aegis-0s2a).
5. List slots: whole or sequence (C6).
6. Partial modes and merge units (§6).
7. Both steps in one savepoint under the unchanged write gate (§3.2), proven
   by fixture C4-gate and its sabotage control.
8. A merge id per merge of one input (§2), proven by fixture C8d.

## 9. Not in scope

- How a decision is presented or chosen (merge-reconcile.md).
- Replication of the sequenced transactions: quipu #926 (aegis-fhyw, merged
  `11f11593`) carries a retraction's valid time to replicas. Every C-case `VT`
  assertion must hold equally on a replica built from the change feed (a karb.2
  acceptance check).
- Fluent properties (karb.8) and blast-radius gating (karb.11). They build on
  this oracle and do not change it.
