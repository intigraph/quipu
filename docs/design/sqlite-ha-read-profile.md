# Inactive SQLite HA read profile

This source profile is an opt-in library API, not an installed HA service.
`ha-read-profile` is absent from both `default` and the shipped `full` feature
bundle. No CLI, server route, configuration loader, timer or packaging path
constructs it. Linux BOOTTIME is required; other targets expose no profile.
Existing `/replica/ready` retains its contract and excludes staleness.

The trusted owner supplies an exact complete-cut manifest and the source stream
key through an independently authenticated channel. Neither is learned from an
arriving frame. The manifest must name exactly the current producer version and
source revision, with no dirty source declaration. This is deliberately stricter
than N/N+1 compatibility: a different build remains unqualified. Declared producer
fields are pins, not remote build attestation; their artifact provenance still
belongs to the owner. Unit fixtures explicitly synthesize this declaration.

The existing shared applier consumes the pinned multipart cut and signed scoped
tail. The profile persists its cut pin after the applier commits the complete
bootstrap. A crash before that marker refuses reopening rather than adopting a
feed-only replica. Reopening discards session proofs and requires another signed
session/challenge. Epoch changes, source mismatch, revocation and stopped state
cannot splice another history into a qualified reader.

Every query, including COUNT, opens one SQLite read transaction. Cursor, verified
coverage, lineage, epoch and the persisted cut marker are read in that transaction;
the existing owner-local read policy authorizes the exact scope before its facts
are copied. The HA-only query view uses the existing lossless record encoder and
shared record-to-datum converter, preserving bytes, references, multivalues and
valid intervals. The ordinary RDF view path is unchanged. No unscoped Store is
exposed, and no readiness token can authorize a later read. Grant/policy rechecks
remain conjunctive with eligibility. BOTH Query and Replicate authority are
required before eligibility/minTx inspection, because a source cursor is global
replication metadata; a Query-only principal must not infer it by probing minTx. A healthy replica never authenticates a
principal. Native raw bytes may participate in queries/counts; attempting to
serialize an unrepresentable raw-byte RDF term still refuses, as on the source.

This first profile requires an explicit finite positive age bound and exactly
zero verified transaction lag. A signed answer covers the source head observed
for that challenge; the profile never claims to know transactions committed
later. BOOTTIME includes suspension. Millisecond observations are rounded
conservatively at publication, and query/copy/policy-read time counts against the
bound. Clock regression invalidates proofs. Caller-provided epoch time must come
from the same trusted authentication adapter as existing grant checks; publication
advances it conservatively rather than extending a grant near expiry.

`min_tx` refers to source numbering, including signed coverage over out-of-scope
transactions. The observed serving snapshot must cover it. Missing state, stale
or absent proof, drain, stop, epoch mismatch, unknown producer compatibility,
attachments, external vector/model providers or frozen cut members refuse.
Custom functions/aggregates and SERVICE are rejected even in skipped branches;
the profile does not claim index/model coverage. No automatic bootstrap network
client or follower process is started. The caller transports frames under the
existing source authentication and scoped grant contract.

Private qualification uses the real cut, shared applier and scoped stream with
two synthetic stores, actual owner policy, and a loopback HTTP test adapter. The
adapter demonstrates authentication/read-only behavior; it is not a new production
endpoint. Runtime acceptance remains open and separate from these source tests.

## Unexecuted deployment and rollback plan

A future admitted rollout must select an exact producer/consumer build pair,
provenance-checked cut/key/policy pins, supported graph/context set, explicit age
bound, measured resource/lag limits and independent host placement. An IaC owner
must define a dedicated read-only follower and its routing eligibility consumer.
There is no routing, placement or migration authority in this source proposal.

Before any installation, retain the prior binary/configuration and a scoped
rollback, current CI, independent source approval and operator window. Qualify a
synthetic staging rollout first. Only a separately authorized live window may
measure one populated read per second across a roll, write refusal, read p95,
lag alert and exact serving revision. Roll followers serially; keep writes on the
primary. Cross-version migration is refused until its exact pair is qualified.
Phase 2 promotion, distributed leases and fencing are excluded.

Rollback of the source profile is to stop its admitted caller and restore the
prior binary/configuration; no existing default caller is changed by this patch.
Do not mutate an old replica into another lineage. Retain the refused artifact
and rebuild from a newly qualified complete cut. Installation alone would not
establish zero failed reads, lag acceptance or HA completion.
