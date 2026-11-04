# Ownership handoff: executable model and candidate records

This implements only P0 of the [accepted plan](ownership-handoff-plan.md):
an executable authority model and candidate record serialization. The module is
`quipu::ownership_handoff_model`. It performs no filesystem, database, signing,
IPC or network operations. Nothing in `Store`, adoption, restore, commit hooks,
replication, startup or the CLI calls it. It does not fix the retained physical
old-A writer counterexample or enroll any store.

## Assumptions and observations

`Model::new` assumes one already admitted generation-zero owner and consistent
durable state. It rejects mid-chain reseeding without retained authority history.
It is not an enrollment or recovery API. `Observations` are explicit model
oracles: authentication of the exact body under an existing profile, current
operator/source/destination transition grants, and availability of an external
current-head witness. These booleans are **not** production verification results
or credentials. Test values do not supply actual signing profiles or principals.
Unknown observations default to false and refuse admission.

The authority tuple binds full-store lineage/unit, generation, epoch, existing
principal, native-placement digest, policy revision and predecessor retirement.
Scoped graph ownership is not modeled. Each operation additionally binds a
destination, existing operator, exact successor generation and existing profile
revision. Requests require distinct source/destination placements; returning to
an old placement requires a later handoff with a new generation and epoch.

The witness and nonce ledger are assumed durable and outside a local restore.
The witness retains ACTIVE, RESERVED and RETIRED phases, not just an owner tuple.
An unresolved reservation blocks ordinary source restart; an existing running A
can still commit until its actual local barrier. The exact internal retirement
can resume without admitting unrelated source writes. The witness cannot remotely
supersede a still-active source. Whole-witness rollback,
cloned running processes/native identities, hostile privileged writes and
pre-fencing binaries remain outside the supported model. There is no clock or
timeout transition, lease, profile provisioning or network coordinator here.

## Durable boundaries, rejection and recovery

The actions distinguish these steps:

1. Externally reserve one immutable operation per predecessor; running A can
   still commit, but ambiguous ordinary source restart refuses.
2. Persist A's RETIRING barrier; every held A handle now refuses.
3. Seal the source data retirement/cut/outbox under that barrier.
4. Complete the agreeing RETIRED protected-journal record.
5. Durably record source retirement at the witness, still naming no active B.
6. Publish the usable retirement body only after both local records agree and
   the external retirement is durable.
7. Persist B PENDING against the exact published retirement and full artifact.
8. Install B's proposed data/placement tuple without admitting writes.
9. Advance the external predecessor CAS to that exact successor.
10. Consume the receipt and persist B ACTIVE/result, with a fresh nonce-bound
   current-head admission. A stays retired.

Each action is one **abstract atomic durable step**, not an implementation of
fsync or distributed transactions. Data, journal and witness boundaries are
separate actions so a crash can occur between them. Receipt consumption and its
ACTIVE/result journal change are one modeled durable group; a future adapter
must actually persist them atomically. The model's internal maps are not a
filesystem layout or a recoverable production database.

`apply` refuses atomically: a failed action leaves all model state unchanged.
`Crash` drops process admission without erasing durable records. `Restart`
requires matching journal/data/placement and a fresh current external head;
returning a historical activation receipt never restarts a writer. An already
admitted running A can commit during witness outage using its local records.
After retirement it cannot. Missing journal or data is UNKNOWN for this enrolled
unit, never a downgrade to legacy INACTIVE.

Fixture restoration actions operate only on in-memory model values. Local
host/journal rollback requires a stopped process, and does not roll back the
external witness or nonce ledger. An old ACTIVE journal therefore cannot
restart A after B owns. Conflicting data/journal states remain non-writable.
This does not establish real host rollback resistance or physical lock safety.

## Candidate record schema

`Record` is a strict `{version, payload}` envelope. Version 1 is experimental,
with these tagged payloads:

| Payload | Binding |
| --- | --- |
| JOURNAL | ACTIVE tuple, RETIRING request, RETIRED certificate, or PENDING certificate plus exact successor |
| OPERATION | Immutable request and globally retained operation ID |
| RETIREMENT | Full request plus sealed transaction coordinate, fact-cut digest and full-artifact digest |
| ACTIVATION | Full retirement body plus exact successor tuple/result |
| WITNESS | Explicit ACTIVE tuple, RESERVED operation, or RETIRED certificate |
| STARTUP | Full externally current witness phase/head plus challenge nonce |

`Record::decode` bounds input at 64 KiB, rejects unknown versions, unknown or
duplicate fields, malformed digests, empty/control-containing identifiers,
unknown roles/units, generation wrap/skips and inconsistent nested tuples.
`Record::encode` validates first and emits deterministic typed JSON. Request and
certificate identities use SHA-256 with distinct model-v1 domains and a byte
length prefix. Input JSON object ordering is not identity: decode and re-encode
the typed structure first. Identifiers are byte-sensitive, not normalized aliases.

Record validation establishes **internal shape only**. A decoded RETIRED record
is not authenticated retirement, and a STARTUP payload is not a signed witness
proof. These records are not the versioned authority wire profile; no network
endpoint, storage migration, actual signature format or trust distribution is
added. Future adapters must authenticate exact bodies, inspect actual cut and
artifact bytes, enforce current grants and supply protected durable history.

## Controls and remaining work

The test suite includes bounded action-order enumeration across competing B/C
destinations, generated action sequences, typed-record round trips and JSON
ordering, held-handle refusal, every modeled crash boundary, exact retries,
nonce reuse, generation exhaustion, revoked/unknown observations, stale local
restore and returning to A through a new generation/epoch. A source mutant that
removes `Model::require_retired` must expose a usable receipt while A remains
active and make the exclusivity control fail. Record actual mutant execution
separately from this specification.

P1-P6 remain unimplemented: real protected journals, commit/RELEASE latches,
cross-resource fsync/crash recovery, real signatures/current-head witness,
authority-aware producer/consumer wire profiles, intent merge-back and native
retained-writer qualification. No successful model run is production fencing,
physical durability, signature acceptance or installed consumer evidence.
