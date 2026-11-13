# Durable knot completion fence

Work item: aegis-2cf. Source preparation only; no transport, migration or
production activation is admitted by this document.

## Observed defect

The served `quipu.knot-completion/v1` registry retains 512 process-local
receipts for one hour. A committed request can therefore become UNKNOWN after
an eviction or restart. Neither absent current facts nor an expired deadline
proves that the old worker cannot commit. The producer must keep its frozen
request rather than resend it. Per-effect operator retirement restores unrelated
progress but does not establish the original effect's completion.

## Binding and authority

Persist a receiver-issued attempt ticket, exact write ID, canonical request-body
SHA256, authenticated principal, stable credential identity, authentication class,
store lineage and log epoch. Request attestation nonces are not stable owners.
Never persist bearer material or use declared provenance as authentication.
A status read must satisfy the same authenticated owner and request binding;
foreign owners, body drift, unknown stores and unreadable records remain UNKNOWN.
No receipt lookup may admit a new write.

Use a separately advertised v2 contract. Keep the existing v1 reader semantics
until its consumer is qualified; do not silently return `committed` as v1
`finished`. Anonymous or missing stable credentials cannot mint a v2 owner
binding. The adapter must explicitly reject unsupported authority, not hash a
client label into an identity. Scope consumer adoption and activation separately.

IDs remain consumed after their detailed result expires. An absent receipt is
UNKNOWN, never permission to execute that ID. Capacity exhaustion refuses new
admission before dispatch; it must not evict an unresolved attempt. Retention
maintenance needs a separately governed bounded policy, not an age-based reset.

## Transaction boundary

Durable admission must precede dispatch. Each attempt has a unique receiver
nonce; all later updates compare that nonce and the complete binding. A sealed
attempt cannot regain admission, and a late worker must fail the compare under
the same SQLite writer reservation as its effect.

The ordinary effect and its compact commit witness must commit atomically at
`Store::transact_to_graph_*`'s existing successful RELEASE. Record the transaction
ID, store identity/epoch and the admitted attempt there, after ordinary authority,
SHACL, policy, ownership and event gates. Do not infer commit from an HTTP status.
A failed RELEASE must not expose a committed witness.

`src/store/ops_transaction.rs` already has atomic OWL batch receipts, making it
the candidate integration boundary. `src/server/publication.rs` owns the actual
blocking worker and calls completion only after deferred embedding. The ordinary
`tool_knot` path in `src/mcp/knot.rs` distinguishes snapshot and assertion writes;
both must bind the same original attempt, including no-op and refusal outcomes.

Do not wrap `tool_knot` in a new outer savepoint. Existing
`src/store/ops_post_commit.rs` hooks can embed and call reactive observers that
write their own transactions. Moving their apparent commit boundary changes
refusal and observer semantics. An ambient "next transaction" hook is also
unsafe: an audit/refusal or derived transaction could consume the main receipt.
Pass the binding explicitly to the intended native transaction. The bound tool
entry refuses an existing transaction or savepoint, so its ordinary RELEASE is
the actual commit boundary; ordinary unbound transaction behavior is unchanged.

Persist worker completion only after every synchronous and deferred action has
ended. A compact **committed** witness is distinct from **worker finished**:
a crash after commit but before completion must retain committed-but-unreadable
custody, never a retry recommendation. A crash during admission with no witness
is unresolved until a receiver-side seal also fences any late worker.

## State and crash obligations

| Durable state | Meaning | Allowed producer action |
| --- | --- | --- |
| absent | No retained proof for this exact binding | Keep UNKNOWN; no resend |
| admitted | Exact ID consumed; worker may still start | Retain frozen custody |
| committed | Atomic effect witness exists; worker completion unresolved | Read exact effect; no replay |
| finished | Worker ended; bounded original result retained | Validate result and exact readback |
| stopped | Worker ended in error; a commit witness may exist | Not rollback proof; read back the exact effect |
| sealed | Receiver fenced this attempt at the commit boundary | No replay; explicit disposition only |

Status reads never seal. Sealing requires a separate, explicit authority-bearing
operation; it compares the full attempt, holds the writer reservation, and cannot
invalidate an already committed effect. It returns a negative execution proof
only when the same commit-time check excludes the old ticket thereafter.
An ordinary process restart, ledger absence, wall time or a newer owner claim
cannot manufacture that proof.

The store's existing ownership commit hook still governs the outer commit. A
restored/copy store or changed log epoch must not silently inherit local attempt
execution authority. Bound retained observations to identity and epoch and refuse
mismatches. Do not export protected admission rows as ordinary RDF facts.

## Qualification before wiring

These are required executable controls, not completed validation:

1. Exact committed result survives cache eviction and a real file-store reopen.
2. Commit succeeds but HTTP response is dropped: status and exact effect prove
   one execution; repeating the write ID refuses without another effect.
3. Kill after admission and before commit: no committed witness is invented.
4. Seal while a worker is paused before the actual commit boundary; resume it:
   the worker's entire effect rolls back and it cannot revive the ticket.
5. Hold a worker through HTTP cancellation and cache eviction: it cannot acquire
   a fresh ticket or overwrite a sealed/foreign attempt when it resumes.
6. Crash after native commit and before deferred work completes: committed proof
   remains, finished is not fabricated, and the producer does not resend.
7. Body, principal, credential, auth class, store and epoch mismatches all refuse
   or return UNKNOWN without leaking another owner's receipt.
8. Admission capacity, corrupt/unreadable ledger and failed commit fence refuse
   before any effect; a missing row never becomes not_started.
9. SHACL refusal, policy/audit error, no-op, reactive observer and deferred embed
   controls preserve existing ordering and cannot bind the wrong transaction.
10. Native server route/auth tests retain read-only bearer restrictions and prove
    restart through a new process rather than only clearing an in-memory map.

## Delivery gates

Sattler c3/c5 supersedes the initial no-build scope. Notes Extension 1 adds
server qualification and a full quality gate to the two initial native runs.
Every run uses a private target, two jobs, nice/ionice, an 8 GiB memory limit
and root/fleet free-space floors. Stop on a guard without a retry loop.
This grants source and PR qualification only: no live writes, replay, migration
activation or installation. Native and server controls are being qualified;
these do not establish installed acceptance. Consumer adoption is separate.
Normal independent review, exact-main CI, deployment-window safeguards and
installed acceptance remain separate. The original aged attempts cannot be
retroactively given a durable receipt by installing this change.

## Opt-in wire shape

A new write sends `X-Quipu-Write-Id` and
`X-Quipu-Completion-Contract: quipu.knot-completion/v2`. The adapter requires a
stable authenticated credential. Status uses the existing `/knot/status` route
with `contract: "quipu.knot-completion/v2"`, `write_id` and the exact original
`request` object. All ordinary route authentication and writer scopes still
apply. A consumed v2 ID cannot become a fresh legacy write by omitting the
contract header. Legacy v1 writes are not retroactively given durable receipts.

No HTTP seal route is exposed. The native seal requires the receiver ticket and
its owner. An explicit native owner-bound seal can select its immutable retained binding
after restart without exposing the receiver nonce. The technical design read
still covers this authority and the lifetime capacity policy; no transport may
seal automatically. Consumer adoption and installed qualification remain pending.

Before consumer activation, expose lifetime ledger occupancy and admission-refusal
metrics, establish rate and remaining-capacity runway thresholds, and name the
operational response. The 65,536 entries cover the store's entire lifetime,
including terminal rows and old epochs. This is not an unbounded service limit
or a concurrent-worker quota. Any compaction policy needs separate review and
must retain consumed-ID and late-worker fences; age alone never permits reuse.
