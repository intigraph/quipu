# Pending observable configuration

This describes the source-stage implementation alongside the
[fluent contract](fluents-v1-contract.md). The schema activation and runtime
acceptance gates in that contract remain open. The APIs below are internal
implementation paths; ordinary CLI and HTTP sharing do not select pending
configuration automatically.

## Request and identity

A pending request is a strict JSON descriptor with contract
`quipu-pending-observable-config/v1` and these fields:

| Field | Meaning |
| --- | --- |
| `graph` | Exact logical graph IRI |
| `name` | Observable identity |
| `subject`, `predicate`, `property_shape` | Native registration binding |
| `policy` | One of the existing four named computation policies |
| `observes` | Sorted, unique, nonempty dependency IRIs |

Unknown fields refuse. Every identity is an IRI of at most 4096 bytes; there
are at most 128 dependencies and 65,536 bytes in the complete request. Decode
checks syntax and bounds without registering an observable or interning its
identities in the receiver.

`Pending` retains the exact received bytes, including whitespace. Its identity
is `urn:quipu:pending-observable-config:v1:` followed by the lowercase SHA-256
hex digest of `quipu-pending-observable-config/v1`, a zero byte, and those
original bytes. Equivalent JSON encodings can therefore have different request
identities. Signing, transport and receipts must use the same original request.

`fluent_pending_configs` stores the request identity and original request BLOB.
Retention is idempotent and checks the retained bytes after insertion; loading
checks the size, descriptor and identity again. This table is transferable
content. A pending row does not establish that its dependencies exist or that
registration would pass the native writer's gates.

## Explicit carriage

`ShareOptions.observable_configs` explicitly selects request strings. Its
default is empty. The matching manifest field is covered by the share identity;
an empty field is omitted to preserve existing empty-manifest encodings. There
are at most 64 requests per manifest, each subject to the descriptor bounds.

Share creation, verification, import and merge validate the requests. Existing
publication checks inspect the raw manifest, original request strings and a
decoded inspection view. Inspection does not rewrite the retained bytes.
Ordinary import and merge retain pending requests without installing their
configuration or a verifier grant. A three-way merge still requires a real
parent share and its normal merge admission checks.

The explicit pendant carrier path carries complete manifests, including
configuration-only manifests, under its existing count and byte limits. Their
complete bytes contribute to the pendant content hash. Ordinary unpack refuses
pending carriers before creating its output. `materialize_pending_carriers`
verifies the complete pendant and receiver publication policy, then atomically
retains the exact manifests and their pending requests. A conflicting complete
manifest refuses the batch. It does not install facts, shapes, queries,
configuration, authentication records or grants.

Ordinary logical snapshots and journals classify pending requests as content.
Native configuration acceptance receipts are excluded from both those transfer
catalogues and ordinary sharing.

## Receiver-local approval and acceptance

`Approval` uses contract `quipu-observable-config-acceptance/v1`. Its signed
message binds the request identity, receiver store identity, SHA-256 of the
currently combined loaded shapes, authentic importer principal and verifier
identity. Decoding preserves the exact received approval bytes. The complete
approval is bounded to 16,384 bytes.

The normal native writer verifies approval inside its transaction. It requires
the current authentic importer and exact graph, request, receiver and profile.
The verifier registry must yield exactly one current keyed registration for
the approval's exact scope in `Scope::Governed`. The scope is domain-separated
from source-event scopes. A valid event witness, raw registration, named-graph
registration or signature without that current governed registration does not
authorize configuration.

After approval, existing registration checks still resolve the actual property
shape and graph, policy, dependencies and cycles. Normal ownership, authority
and post-state SHACL checks still apply. Generic transport never invokes this
native materializer.

`fluent_config_receipts` records the accepted graph/entity, native transaction
and original request and approval bytes. Insertion requires the native writer
permit for that transaction and graph/entity. SQL triggers refuse changes and
deletion. The receipt is written in the same native transaction as registration;
a failed post-state check rolls both back.

Replay first rechecks current approval and then requires the original receipt's
transaction and exact request and approval bytes. A pre-existing local
registration without that receipt cannot be relabeled as an approved import.
Revocation can refuse a retry without erasing the prior accepted configuration.

An owned respace operation remaps the receipt's graph/entity term IDs and keeps
its transaction and signed bytes unchanged. It reinstalls the receipt guards
before committing. This preserves existing local evidence; it does not grant a
different receiver permission to reuse the approval. Physical database copying
is separate from the ordinary logical-transfer contract above.

## Acceptance evidence and remaining work

The focused native controls exercise request persistence and restart, share
identity and pending-only import, actual disk merge, pendant hash and tamper
refusal, original approval receipts and SQL immutability, post-state rollback,
nonempty receipt exclusion from snapshots/journals, populated respace and
decoded publication checks. The configuration controls also cover scope,
signature, revocation, receiver/profile/importer/graph mismatches and native
registration failures.

Those controls qualify the source path. Full library and feature checks,
Clippy, Wasm/server compatibility, integration with current main, independent
review and installed acceptance remain separate gates. Fork admission,
retention/roll-up, producer admission and governed proposal activation retain
the dispositions required by the fluent contract. Neither this document nor a
successful pending transport test supplies an activation decision.
