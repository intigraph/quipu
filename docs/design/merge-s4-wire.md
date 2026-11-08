# S4 decision and presentation wire changes

Status: source checkpoint, not qualified or activated. Rust compilation,
focused tests, pendant byte reproducibility and the normal quality gate remain
pending. The frozen pure `merge_graphs` operator is unchanged.

All three merge surfaces report changed slots where ours equals theirs as
`convergent`, including agreeing deletions. They default to `continue`; that
choice preserves the current claim without writing a receive/decide operation.
Explicit `base` and supported explicit values remain subject to the core's
decision and cardinality checks. Ordinary conflicts retain their existing
choices. A report does not turn a mechanical suggestion into a review receipt.

The source emits share decisions v3 (and v3-whole-root), fork decisions v2 and
Git decisions v2. Readers retain the previous share v1/v2, fork v1 and Git v1
paths. Legacy files do not acquire new convergent rows during their stale-input
comparison. New files bind their row kinds to the recomputed inputs.

The presentation schema is `https://github.com/scbrown/quipu/merge-view/v2`.
It identifies the new convergent kind, `continue` choice and nullable bound.
Finite `max_count` values remain JSON integers. An unbounded convergent slot is
JSON null in both its decision record and its presentation rule. It must never
become a platform-sized integer in only one surface. The current internal
legacy record uses `usize::MAX` as its unbounded review marker; that is not a
wire value or a promise that an unknown shape profile is unbounded.

Qualification must compare exact canonical pendant graph and decisions bytes
against the frozen operator on ordinary conflicting inputs, as well as check
the intentionally new convergent rows. The retained small-set/cardinality
vectors and new nullable-bound presentation controls are test source only;
none has been run for this restored checkpoint. Historical green results from
other branches do not qualify it.
