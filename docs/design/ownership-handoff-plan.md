# Ownership handoff implementation and acceptance plan

Status: proposed plan for [explicit source retirement](ownership-handoff.md).
Controller disposition selected Option A; no distributed lease implementation,
new trust root, coordinator provisioning or production action is authorized.
Each phase below needs its own concrete source scope and normal review.

## Entry evidence and first source step

Retain the original two-host native fixture and source/asset manifests. The
unchanged original A writer committed after B adopted its copy; B host/path
checks and fresh epochs passed. This is the required counterexample, not a
reason to rerun production adoption or manufacture another identity.

Source seams at design base `11f11593759af43c6ca6f0dffbb7f18e2c182a3e`:

| Owner role | Code and responsibility |
| --- | --- |
| Store/transaction maintainer | `store/ownership.rs`, `ownership_fence.rs`, `release.rs`, constructors and direct writers: role, journal, latch and commit refusal |
| Replication maintainer | `replicate_source.rs`, relay/poll paths, applier and bootstrap: retirement control, generation pin and no splicing |
| Signing/authority maintainer | Existing admitted signing/verifier registry: request, certificate, chain-head admission and nonce/idempotency verification |
| Client/merge maintainer | Local-sync client, overlay/base journal and changeset admission: preserve and merge non-owner intents |
| Controller/deployment owner | Exact source identity, qualified authority host/profile, rollout, process/version floor and rollback admission |

The first implementation proposal should be **P0 below only**: an executable
authority-state model, durable-record schema/serialization and discriminating
private tests, with no production writes, owner enrollment or wire activation.
Do not start store enforcement before the journal, rollback and lock-order
contracts are reviewed. File the concrete source carrier after design acceptance;
this plan does not dispatch an unbounded implementation or select a new principal.

The [executable model and candidate record contract](ownership-handoff-model.md)
describe that source-only P0 implementation and its remaining assumptions.
They do not activate store enforcement or satisfy native/runtime acceptance.

## Phases and proof gates

| Phase | Deliverable | Required proof before advancing |
| --- | --- | --- |
| P0: model and authority contract | Store + signing owners specify canonical tuples, ACTIVE/RETIRING/RETIRED/PENDING transitions, receipt and witness prerequisites | State-machine enumeration rejects two active owners, stale predecessor, different retry payload, skipped generation, nonce reuse and ambiguous recovery. Removing the retirement prerequisite must reproduce the retained split-owner case. No runtime enablement |
| P1: local role and commit boundary | Protected journal, generation pin, per-lineage latch and narrow internal transition token | Real SQLite outer COMMIT, autocommit and outer RELEASE; stale handles, direct bookkeeping, batch/registry/import/fork/share writers; rollback leaves no fact, cursor, audit or success receipt. Race commits against retirement with deterministic barriers |
| P2: source retirement and durable outbox | Seal cut, fence A, end sessions and issue one destination-bound certificate only after durability | Crash at every journal/data/outbox/fsync boundary; held A writer after barrier fails; source/relay emits terminal signed control and cannot answer fresh challenges. Losing the response does not unretire A or mint another receipt |
| P3: destination and restart admission | Verify full artifact/cut, chain head, existing trust/profile and native binding; persist pending then activate once with a fresh epoch | Tamper, wrong destination/path/host/scope/profile, stale/reordered proof, partial restore, duplicate activation and concurrent B/C requests; old database/sidecar with current journal refuses; offline or stale-witness startup fails closed |
| P4: non-owner intents and consumers | Versioned authority profile, s24.4 merge-back, explicit bootstrap/generation boundary and diagnostics | Valid offline edits survive and merge through current-owner validation; stale epoch/base, revoked authority and conflicts refuse without losing overlays. Late A canonical writes remain attributed evidence, never silently converted or replayed |
| P5: separate native qualification | Controller-admitted real two-host transfer, restart/crash matrix and same-source rollback rehearsal | Original retained-writer case now refuses A after retirement and admits exactly one B; fresh follower bootstraps and tails correctly; old follower remains unchanged; all failure and compatibility controls below pass |
| P6: scoped rollout and acceptance | Explicit enrollment of inventoried lineages/writers/consumers under current deployment gates | One same-source smoke and natural acceptance from the actual serving artifacts; full operator recap and protected-journal evidence. Source/review success alone is insufficient |

P0-P4 may be authored and exercised only in admitted private fixtures. P5-P6
require a later exact runtime packet; their inclusion here grants no window,
production store access, restart, fence, identity, credential or policy mutation.
Required signing distribution and real-client gates cannot be replaced by test
signers or renamed as complete. OCI/browser ownership remains unsupported.

## Discriminating validation matrix

Pair every refusal with an ordinary positive control and an actual source
counterfactual. Record which mutants were run; a proposed mutant is not a result.

| Boundary | Negative / counterfactual | Positive control |
| --- | --- | --- |
| Original physical-host owner | Keep A handle open, retire A, activate B, release A write; bypassing the role check must let it through | Pre-barrier A commit succeeds and belongs to the sealed cut; admitted B commit succeeds |
| Foreign native placement | Real transferred host identifier with every other binding field valid; remove host equality check | Exact admitted native host/path and unchanged protected authority |
| Retirement serialization | Pause an outer write before commit, race the exclusive barrier; omit latch or use cached ACTIVE | Ordered earlier write commits; bounded contention never deadlocks |
| Nested scopes and direct SQL | Refused outer RELEASE followed by another write; omit rollback helper | Fresh valid transaction persists exactly once after a refusal |
| Crash recovery | Kill at each durable marker/outbox/sidecar boundary; expose a receipt before retirement is durable | Identical-operation recovery returns the same cut and receipt, never another owner |
| Authority and replay | Concurrent destinations, changed operation digest, reordered predecessor, wrong signer/audience, reused nonce; disable one verifier | Admitted existing principal and exact idempotent retry |
| Clock / partition | Offline A, expired wall-clock timer, backwards clock, witness outage; replace retirement with timeout promotion | Already admitted running A stays safe offline; B intents remain available |
| Rollback / ABA | Old DB and sidecar, deleted or unreadable journal, prior consumed receipt and generation reuse | Current journal survives ordinary DB restore; fresh generation/epoch is admitted only through a new handoff |
| Whole-authority recovery | Restored host/journal or absent fresh startup challenge; allow cached boot admission | Fresh external current-head proof admits only the current placement; unsupported recovery stays non-owner |
| Stream / follower | Delayed old frames, epoch_end dropped/reordered, stale generation and chunk interruption | Real producer -> fresh bootstrap -> authenticated tail reaches exact scoped facts; old follower never splices |
| Offline merge | Late A canonical records, stale base/epoch, conflict, revoked key and invalid candidate | Legitimate intent with retained base merges through current authority, full validation and one idempotent receipt |
| Compatibility / downgrade | Enrolled lineage opened through alternate constructors, missing sidecar, old binary or inactive mode | Unenrolled legacy startup/writes/ordinary compatibility remain INACTIVE without minting ownership |

The full host/journal/witness rollback assumption needs explicit admission:
disk fsync is not a monotonic hardware counter. If an externally current head
and enforced startup boundary cannot be supplied, do not weaken the check or
claim recovery safety. Preserve the unavailable capability and operator hold.

## Operational evidence and observability

Capture exact commit/tree, binary and configuration/profile digests, platform,
native-provider outcome, data/authority-journal identities, protected-path
before/after controls, role/generation/cut and receipt digests. Keep raw private
artifacts with the item; public summaries use abstract hosts and no credentials.
Use the normal HOME guard on supported platforms; an unsupported platform needs
an explicit scoped rule, never a manufactured clean verdict.

Proposed structured audit events cover prepare, barrier committed, source
retired, receipt emitted, destination pending/active and owner-commit refusal.
Bind operation/lineage/generation/cut and outcome; do not put signer secrets in
logs or unbounded IDs in metric labels. Metrics distinguish role/phase,
retirement age, pending handoff, refused stale commits, invalid proof, startup
authority UNKNOWN and preserved/quarantined intents. A timeout is a detector,
not authority to promote. An observer has its own completeness/freshness signal;
missing samples cannot prove zero simultaneous owners.

## Rollout, rollback and original acceptance

1. Accept the design and each source phase through current exact-head CI,
   native risk classification and one non-author review. A design-only change
   has no runtime demo; the state diagram, counterexample and test matrix are
   review evidence, not a staged successful handoff.
2. Inventory every writer and consumer for a proposed enrolled lineage. Keep
   all existing production activation, shared-verifier distribution and client
   gates until their actual evidence is accepted. Do not auto-enroll legacy stores.
3. Rehearse the controller's exact-source installation and rollback in disposable
   native stores, including the protected journal and minimum writer version.
   Admit the authority witness/profile and failure behavior separately.
4. In a later controller-authorized deployment packet, check and apply the same
   source identity, inspect the real recap and executing artifacts, run one
   bounded smoke, and retain an explicit rollback owner and stop condition.
   No forced refresh, lease, route, new key or hold release is part of this plan.
5. Rollback never clears retirement, deletes a journal, rewinds generation,
   revives an old epoch or permits a pre-fencing binary to write an enrolled
   lineage. If forward recovery is unavailable, stop authoritative writes and
   retain read-only canonical state plus intents. Returning ownership is another
   explicit handoff, not an automatic rollback side effect.
6. Close original ownership acceptance only with the real old-A writer fenced
   before B owns; a verified durable cut/certificate, new B epoch and binding;
   correct live-session termination and follower rebootstrap/no-splice; restart,
   crash, partition and rollback negatives; and actual qualified consumers.
   The previously unowned compatibility path must still pass without claiming
   ownership. Record unavailable OCI/browser capability rather than counting it
   as a positive. Any missing production/activation proof remains explicitly open.
