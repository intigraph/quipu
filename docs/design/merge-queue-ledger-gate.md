# Ledger provenance before merge-queue enablement

The `Merge-group ledger` workflow reports **Merge-group ledger provenance** on every
`merge_group: checks_requested` event. Before enabling a queue, the repository
settings owner must add that exact check name to the required checks, alongside
the existing checks. Queue settings and required-check settings are separate
operator actions; this change does not enable or configure a live queue.

The job checks out the event's combined head with full history and read-only
permissions. It refuses an event/checkout mismatch, missing or ambiguous stamp,
short or unavailable revision, and any conformance-code difference between the
ledger's measured revision and the combined tree. It uses the landing helper's
strict stamp contract, including the union of base and head `CODE_PATHS`, so an
entry cannot narrow the inherited coverage. There is no stale-stamp waiver or
report-only success on the queue path.

This is an admission check of existing evidence. It neither derives new ledgers
nor commits them, and it adds no bot exemption. Derivation inside CI and cron-bot
retirement remain separate work; a stale group is refused until valid evidence
covers its combined code. A correct docs-only group can pass with an older
measured revision when all covered code is identical.

Run `just merge-queue test` for synthetic queue groups with correct, missing,
wrong, ambiguous and stale ledgers, a narrowed path contract, and an event/head
mismatch. `just merge-queue check` runs the same validator as CI and requires
the real Actions event environment. The controls run on PRs as
**Merge-group ledger controls**; they do not claim that a live queue ran.

Before queue enablement, verify a real combined-group check reports under the
required name and rejects stale evidence. Rollback is to keep the queue disabled
and revert this source change through the normal reviewed path. Never waive the
new required check while allowing queue landings.
