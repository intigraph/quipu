# Title selection and graph membership

A retained, approved cold title-search trial returned the exact requested page
in ten requests, but increased the engine's sampled cgroup memory by
190,246,912 bytes against a 50,000,000-byte target. The positive item-show
control used nine requests and increased memory by 3,162,112 bytes.

The first project title-selection request took 1.316 seconds and returned
2,144 bytes. Fourteen samples entirely within that request show an increase
of 175,325,184 bytes in cgroup memory and 104,804,352 bytes in process RSS.
Later requests fetch the selected items' metadata and details. The trace
identifies the expensive phase, but contains no allocator stacks or cgroup
memory category breakdown. **The allocation mechanism remains unknown.**

Parsing the retained query confirms that its string filter directly wraps a
basic graph pattern, so existing title narrowing applies. Its constant type
pattern excludes the resident graph model. Neither a missing string pushdown
nor a whole-board Rust model is established as the cause.

The ordinary current title leaf reads values from the predicate index, then
checks graph membership through the fact table's `g` column. The existing
partial graph index contains the same physical rowid. For an eligible narrowed
leaf, checking that rowid in the graph index avoids accessing fact payload
columns. This is a possible contributor to the observed memory growth;
it is **inferred**, not an allocation profile result.

The optimization applies only to a retained, unseeded, current, single-file, single
named graph leaf with a constant predicate and narrowed variable object.
It requires the exact complete current graph-index definition. History,
attachments, other shapes and missing or unproved indexes retain the ordinary
graph predicate. Scalar/streamed counts and bounded SQL-star relations also keep
their existing covering plans. No schema, persistent cache or pooled-reader
policy changes.

Membership must refer to the **physical row**, not only its subject: a subject
can have different titles in different graphs. A differential control preserves
the exact ordered title rows, including a language literal, while excluding
foreign titles for both shared and distinct subjects. Replacing the rowid
correlation with a subject correlation admits a foreign title and fails that
control. Another control checks fallback with a same-named incomplete partial
index, absent index, history and unsupported graph scopes.

The [native execution-plan comparison](title-graph-membership-plan.txt) is an
actual SQLite `EXPLAIN` result on the retained fixture, without executing its
fact-data query. The baseline reads the fact table's graph column. The
candidate uses `(g, rowid)` membership and has no fact-table `Column` operation.
Private source tests repeat that property using the implemented predicate.

There is no measured before/after memory improvement yet. The target remains
50,000,000 bytes and at most twenty requests with exact page parity. An
allocation profile on an owned offline copy and a separately authorized bounded
cold trial are still needed to establish causality and runtime acceptance.
The retained driver, its consumed attempt and its lease remain untouched.

Reproduce the private differential and plan controls with:

```sh
just test --lib graph_membership
```
