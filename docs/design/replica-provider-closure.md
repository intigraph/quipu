# Portable provider comparison for a future standby

This source-only library component compares two independently obtained native
`quipu-acquisition/composed-v1` payloads. It adds no server route, CLI command,
readiness publisher, service, replication process or automatic caller.

[`replica_provider::compare`](../../src/replica_provider.rs) accepts each payload
with its own caller-supplied acquisition request. It first invokes the existing
native payload verifier on both inputs. It does not implement a second manifest
parser, replace the acquisition schema, fetch an artifact or open a database.
Callers remain responsible for obtaining both artifacts through independently
trusted channels: a receipt hash proves integrity, not origin authentication.

## What is compared

Both requests must name the same exact MAIN transaction and each request must
bind its own observed topology generation. Successful native verification checks
the selected graph scope, canonical graph bytes, receipt hashes, completeness,
applied pins and the receipt's internal provider census.

The additional comparison requires identical MAIN lineage, applied transaction,
selected graph content identities and ownership/policy-dependency digest. Every
provider alias must have the same persistent store ID, complete file digest,
manifest digest, local head, term space, graph inventory and scope exclusion.
Missing, extra or different providers refuse. Provider list order is not identity.

Provider `instance` and composition `incarnation` describe physical handles on
their respective hosts. Each remains covered by its own native receipt and
expected generation. They must not be compared as a shared restore epoch across
copies. Neither a physical path nor a copied persistent store ID establishes a
source restore epoch.

## A match does not make a replica ready

The returned report binds both receipt hashes and always contains:

```json
{
  "schema": "quipu-provider-closure/comparison-v1",
  "provider_closure": "matched",
  "ready": false
}
```

It also records the applied transaction and provider count. The report's fields
are private, and its `ready()` accessor always returns false. There is no HTTP
status or health-check exit code to route traffic on.

The acquisition contract exports **qualified MAIN data** and explicitly excludes
attachment data. A matching provider set therefore does not establish composed
query or search parity, provider-order semantics, complete read-policy parity,
unselected MAIN data parity, transport freshness or model/cache generation.
It also does not prove that a later query checks minTx on the same read snapshot,
or that the source has not been restored into another epoch. Those are separate
required runtime contracts before any standby can publish readiness.

The current follower's generic health response can remain successful while a
missing frozen file makes an ordinary graph query return an empty result. This
library does not change that route or claim to fix it. Native composed
acquisition already refuses an incomplete frozen-provider census. Consumers must
propagate acquisition refusal or absence rather than fabricate an empty payload
and treat it as a complete observation.

## Verification and delivery boundary

The tests begin with a real disposable store, committed MAIN data and a frozen
pack, and obtain pinned payloads through the native composed acquisition path.
They compare an unchanged pair, different physical-handle declarations, and
restamped counterfactual receipts whose portable identities, scope, lineage,
pins, graph content or policy dependencies differ. Restamping is a private
integrity fixture, not authentication or a claim of a real replica deployment.
Native hash, generation, scope and completeness refusals remain load-bearing.

Run the focused component with:

```sh
just test --no-default-features --lib replica_provider::tests -- --test-threads=1
```

Also run the existing composed-acquisition controls. A repeated private
present/missing-file follower probe must retain ROOT and present-pack positives,
write refusal and an unchanged root file. Its existing missing-file result is
expected to remain unchanged by this unwired component.

No source merge grants installation, a restart, routing, a transport credential,
promotion, a failover drill or permission to treat an empty replica read as
authoritative absence. A later reviewed integration must supply the remaining
contracts and observe actual runtime acceptance under its own operator slot.
