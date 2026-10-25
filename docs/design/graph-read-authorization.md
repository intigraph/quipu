# Per-graph read authorization for scoped replicas

Status: proposed model for the P1 ruling in [local sync mode](local-sync-mode.md).
This document changes no route, credential, grant or deployed policy. Existing
`/share/asof`, `/replicate/stream` and `/replicate/challenge` remain write-class
until the implementation and its paired controls are admitted. A design merge
alone is not permission to serve a second principal.

## 1. Decisions proposed

1. Add an explicitly enabled **scoped-read profile** for an entire serving
   instance. In that profile, every data or metadata read requires a verified
   principal and an exact read grant. Unclassified routes refuse. The legacy
   open-read profile remains available for existing single-principal instances;
   it cannot advertise second-principal isolation.
2. Reuse verified named credentials and enrolled session attestations for
   identity. Introduce no new credential format. Caller provenance, an actor in
   JSON, an audience declaration, a graph label and write authority are not read
   grants. The shared writer bearer does not identify a scoped reader.
3. Use separate, protected, versioned **read grants**, with explicit graph IRIs,
   operations and finite lease ceilings. No implicit ROOT, sibling, companion,
   shape, ontology or policy graph. Delegation intersects grants, never unions
   them. No grant means no access.
4. Enforce graph scope **before execution**, through a scoped read view shared
   by SQL, resident-model, search and export paths. Filtering returned rows after
   an unscoped read is insufficient. A path that cannot use the view refuses in
   this profile until implemented.
5. Bind bootstrap, caches, resumptions and offline serving leases to the same
   principal, source, exact graph set and policy version. Authenticate and
   re-authorize every use. A cursor, digest or cache hit grants no authority.
6. A revocation stops new publication on connected streams and invalidates
   resumptions. Offline serving ends at lease expiry or learned revocation,
   whichever comes first. Previously disclosed bytes cannot be recalled.

## 2. Current source inventory

The inventory is pinned to Quipu `cc33128f`; it is not a claim about a deployed
configuration or proof that a proposed mechanism already runs.

| Mechanism | What exists | Missing for scoped reads |
|---|---|---|
| HTTP route classification | `src/http_auth.rs`: reads return `Allow` before bearer lookup; explicit read/write route census | A third distinction: store read with graph authorization. Changing a route from write to read today opens it. |
| Verified identity | `src/server/auth.rs`: named registry principal; task-local owned request identity. `src/server/write_attest.rs`: verified session identity and replay checks | Identity must be established on scoped read routes before graph selection; it is not itself a grant. |
| Signed request transport | `write_attest.rs` binds method/path/content type/body, refuses query strings; signed reads spend nonces before dispatch | Existing query-string GET clients cannot be called signed readers unchanged. Use the named-credential path or an explicitly supported signed JSON request, not an unsigned retry. |
| Graph authority | `src/governance/authority.rs`: intersection, empty-set refusal and write `authorityOver` | Reuse the set/intersection algebra, not the write predicates as read authorization. |
| Bootstrap | `src/asof_share.rs`, `src/asof_share/manifest.rs`: exact scope/context, policy digest, source identity and pinned cut | Its policy is currently `quipu-asof-policy/write-class-principal/v1`, not a principal-specific graph grant or lease. |
| Bootstrap send | `src/server/asof.rs`: one pinned read, then a temporary spool and commit barrier | A second-principal spool needs principal binding and current publication authorization while it is sent. |
| Feeds | `src/server/feed.rs`: a missing change graph spans all graphs; ROOT events and named-graph events have distinct routes | Authenticate scope before graph lookup, filter transaction/event metadata, and re-authorize publication/resume. |
| Shared applier | `src/applier_stream.rs`, `src/replicate_wire.rs`: source-signed coverage/control; source/epoch/scope binding and revoked control vocabulary | Coverage signatures are not reader authentication or grant leases. Enforce leases at the serving facade and propagate revocation through relays. |

## 3. Identity and the authoritative grant

A scoped request carries a server-created `ReadIdentity` obtained only from an
existing verified named credential or enrolled session attestation. It includes
principal IRI, credential identifier, authentication class and credential
validity. Invalid presented evidence refuses; it never retries anonymously or
with a shared credential. Local embedded callers must explicitly supply the
same trusted context; a method accepting arbitrary principal text is not an
external authentication boundary.

A `ReadGrant` is a new governed record in an owner-controlled control graph.
It contains grant ID, exact principal, exact graph IRIs, allowed operations,
valid-from, valid-until, maximum offline lease seconds, issuer and authority.
The MVP has no graph globs, regular expressions, label selectors, public
fallback or wildcard. ROOT requires its own explicit registered identity.
Operations distinguish data query, metadata, export and continuous replication;
a query grant does not automatically authorize making a persistent copy.

The control graph is not ordinary tenant data. Applying/revoking grants is an
owner-local administrative operation using the existing guarded transaction
path. It writes the grant revision and a persistent authorization generation in
one transaction, with provenance. Tenant writes, imported graphs, mapping tools
and untrusted proposed RDF cannot modify effective grants or that generation.
New record classes and their SHACL constraints ship together. A grant without
qualified issuer authority never enters the effective policy.

Do not overload `authorityOver` or infer read permission from write permission.
A registration binds an identity; a read grant binds that identity to permitted
reads. Their checks are conjunctive. A delegated call uses the intersection of
all verified links' operation sets, graph sets and validity intervals. Empty,
unverifiable or cyclic delegation refuses. The first implementation may refuse
all delegated requests rather than silently accept an unverified chain.

The policy snapshot has a canonical digest and generation. Its contents are
obtained from the protected authority, never from a share being imported. A
malformed, unavailable or stale authoritative policy yields 503/UNKNOWN and no
tenant data, not the previous potentially revoked grant. Invalid configured
scoped mode prevents that instance from serving scoped reads. It must not
silently restart in legacy mode.

## 4. One scoped read view

After authentication, authorization builds an immutable view:

```text
ReadView = principal + credential + operations + exact graphs
         + policy generation/digest + source store/epoch + expiry
```

Each response executes against one database snapshot and one qualified policy
snapshot. A policy change before publication is handled by section 7. A view
is request-owned, propagated explicitly across blocking tasks and removed on
success, error, cancellation and panic. It is never a process-global current
principal or an ambient graph union.

**Dataset rules:**

- An explicit graph or dataset must be a subset of the grant for the requested
  operation. If any member is denied, refuse the request rather than quietly
  remove it. Denied and nonexistent graph names have the same public denial.
- Without an explicit dataset, the default is exactly the authorized graph set
  for that operation. A zero-graph grant refuses; it is not an all-graph query.
- `GRAPH ?g` ranges over that view only. `FROM`, `FROM NAMED`, stored-query
  bindings, attached stores, property paths, subqueries, negation, inference and
  composition use the same view. A syntax or physical plan with unqualified
  external access is unsupported/denied, never an unscoped fallback.
- Context graphs, schema, mappings and inference companions require separate
  grants. If a computation needs inaccessible context, return incomplete/UNKNOWN
  without naming hidden graphs or using their data. Do not copy their facts into
  ROOT or derive public answers from denied premises.
- A view constrains fact reads, entity hydration, dictionary-based discovery,
  graph registries, statistics, search candidates, embeddings and caches. Merely
  restricting the final SPARQL projection does not establish isolation.
- Every query executor and early-exit optimization has the same boundary. If
  SQL is scoped but a resident model or vector index is not, disable that path
  for scoped readers until its paired isolation controls pass.

An embedded owner store remains trusted local administration. It is not safe to
hand its unscoped `Store` object to a tenant handler and rely on convention.
Tenant handlers receive a read-only scoped capability; raw connection, generic
query, global term enumeration and unqualified attachment access are absent or
refuse. Structural inventory and adversarial fixtures must establish this for
all allowed routes before the profile is enabled.

## 5. Route coverage, including metadata

The route census must require an explicit `public-static`, `scoped-read`,
`owner-only` or existing `write` classification. There is no default allow for
new paths, methods, aliases, MCP dispatch or nested computations. In scoped mode
only fixed static assets and a minimal health/version response are public.
Version must omit configured source/graph/credential information. Metrics and
global diagnostics are owner-only unless a separate sanitized interface exists.

| Surface | Required scoped behavior |
|---|---|
| `/query`, `/ask`, stored queries, `/search_query` | Execute the full plan in the view; authorize expanded stored datasets; no hidden-graph fallback. |
| Search/context/entity/history/graph/render/explain and UI data | Candidates, hydration and provenance use the view. Hidden-only entities are indistinguishable from unknown ones. Unsupported computation refuses. |
| `/graphs`, datasets, namespaces, shapes/ontology listings, projections, stats, counts and watermarks | Return only metadata justified by authorized graphs. Global registry methods are owner-only until safely scoped. Aggregate, empty-result and error paths are included. |
| `/export`, `/share`, `/share/asof`, graph-store reads and `/acquire` | Authorize every data/context graph before opening the cut or artifact. Preserve existing destination, scrub, signature and publication rules as additional checks. |
| `/acquire/composed`, eligibility, alignment and merge previews | Check all underlying sources and disclosure metadata before planning. An eligibility observation must not reveal denied source existence. Owner-only until this is proved. |
| `/changes`, `/changes/stream`, `/events`, `/events/graph`, `/events/stream`, `/feed/ws` | Bind every subscription to the view. No unscoped default stream; mixed transactions carry only permitted records and permitted metadata. Each multiplexed subscription is checked separately. |
| `/replicate/stream`, `/replicate/challenge`, relays and bootstrap spools | Require replication permission and a current scoped lease. Signed source coverage must describe the authorized cut; it does not imply global completeness. |
| Durable consumer cursors, receipts and operational records | Private to their principal and scope; knowing an identifier or nonce is not read authorization. Existing mutation authentication still applies. |
| MCP and embedded serving facade | Same operation/view enforcement as HTTP; bypassing the router must not bypass the policy. |

Transaction numbers and global-head movement can themselves disclose activity.
The MVP returns no hidden transaction source strings, actors, counts, reasons or
payloads. A replication cursor may remain the source transaction number required
by the shared applier, but therefore reveals that the source advanced. This is
an explicit **traffic-analysis disclosure**, not full noninterference. The
proposed model permits that narrow cursor/heartbeat disclosure for replication
only. If unacceptable for a deployment, it must deny replication, not claim
that filtering records hides global activity. Ordinary queries need no global
head or per-hidden-graph metadata.

## 6. Bootstrap, cache and offline lease

A read grant is not a bearer token. The source issues a signed finite lease only
after authenticating the reader and validating the exact replication scope.
The lease binds version, principal, credential ID, source store/epoch, exact
data/context graph sets, scope digest, policy digest/generation, issued-at,
expires-at and maximum offline duration. Its expiry cannot exceed either the
grant expiry or any expiry imposed by the credential verifier. Credentials
without an expiry do not remove the finite lease bound. The initial ceiling
is five minutes, configurable
only downward for this profile until a measured requirement changes the ruling.
Issuance, signing-key rotation and lifecycle use the existing source signing
plane; there is no key material in the lease.

This is an explicitly versioned extension. Existing `quipu-asof-share/v1` manifests
and stream messages use strict deserialization; do not append fields and assume
old clients ignore them. Negotiate a new profile/schema, keep the shared applier,
and require the upgraded client to verify the lease and current policy binding.
Legacy artifacts remain valid only in their existing single-principal profile.

Authorize before reading even a cached or spooled artifact. Cache identity
includes principal, credential, operation, source lineage, exact scope/context,
policy generation/digest and pinned data snapshot. Cache content digests remain
about content; authority is never inferred from equality of bytes. A cached old
lease, valid source signature or guessed manifest hash cannot bypass current
policy on an online request. Shared intermediary responses are private/no-store
unless a separately proved cache keys all these dimensions.

The replica's serving facade verifies its lease before every local read. It
uses a monotonic elapsed bound anchored at verified source time, deducting
transport delay and clock uncertainty rather than starting a fresh full lease
on receipt. If a conservative remaining lifetime cannot be established, it
refuses. A backward wall clock, unknown clock state, restart without fresh
authorization, observed source
epoch/policy change or an unverifiable lease fails closed for the second
principal. Heartbeat and signed freshness coverage are not lease renewal.
Renewal requires authenticated current authorization from the source.

After expiry or a learned revocation, no scoped data API serves the held copy.
The local bytes, rejected overlays and previous receipts are preserved for the
owner's recovery; they are not erased and cannot be made unreadable to a
malicious device owner by an API check. The guarantee covers cooperating
serving software, not recall of downloaded data or isolation between processes
sharing its operating-system authority.

## 7. Revocation and publication races

Grant changes and credential revocation invalidate the authorization generation
used by affected views. A source process serializes policy commits with short
publication permits: a completed revocation invalidates queued views before
any subsequent chunk is handed to the transport. It never holds a database
transaction while waiting for a slow client or network write.

For an ordinary buffered response, re-check immediately before response
publication. For a spool or live stream, check before every bounded part or
transaction and before the final commit barrier. The producer's existing
transaction/chunk atomicity remains intact. If authorization fails mid-cut,
withhold the commit barrier and send a bounded terminal authorization control
where the protocol permits; the applier discards uncommitted buffered parts.
Do not resume a partially authorized cut by merely changing its policy digest.

The linearization point is handing a permitted bounded chunk to the transport,
not its eventual arrival at the remote device. Bytes already handed off may
arrive after revocation. New handoffs after the revocation publication barrier
are forbidden. Tests must control that barrier rather than rely on sleeps.

A connected follower receives a source-signed `revoked` control and stops serving
that lease. A relay cannot renew a child's authority beyond its own lease or
upstream validity; parent expiry/revocation invalidates child publication and
serving. A disconnected device cannot learn revocation immediately: the finite
lease bounds that interval. On reconnect, cached shares, resume cursors and
leases are re-authorized; they never restart the old grant automatically.

Policy persistence, credential checks and cancellation state must agree after
crash/restart. If current authorization cannot be established, return UNKNOWN
without a data frame. No available old snapshot is an authorization fallback.

## 8. Paired acceptance fixtures

Use two principals and two actual stores with exact graphs A, B, explicit ROOT
and context C. P may read/query/replicate A plus C; Q may read B. Distinct
credential verifiers and registered session identities are fixture-only.
Each positive must pass and its paired unauthorized request must refuse.
Disable the enforcing mechanism in a controlled mutant and require that the
negative fixture detects the resulting disclosure. Test native SQL/resident-model
paths and the real HTTP,
MCP and replica serving boundaries; no production credentials or traffic.

1. **Dataset:** P queries A successfully. P's explicit B, ROOT, A+B or unknown
   graph requests refuse uniformly. P's default and `GRAPH ?g` enumerate A/C
   only. Include paths, negation, subqueries, stored datasets and attachments.
2. **Cross-route:** repeat on export, acquire, as-of share, changes and both
   event routes; include WebSocket subscriptions and a relayed grandchild.
   An allowed scoped bootstrap reproduces its data digests exactly.
3. **Metadata:** compare stores differing only in denied B facts, labels,
   graph/schema names, search vectors and operation records. Query/search,
   discovery, counts, hydration and errors are identical, except the explicitly
   permitted replication cursor/heartbeat disclosure in section 5.
4. **Identity:** copied actor/session headers, invalid evidence, legacy shared
   bearer, an unbound principal and a write-only grant cannot read A. Valid P
   evidence can. A bad signature never falls back to another credential.
5. **Revocation:** cache a share and resume cursor, revoke P, then reuse each.
   Both refuse before artifact bytes; Q's allowed request remains positive.
   Control an in-flight publication barrier and show no new handoff after the
   revocation commits. Check multi-part commit withholding and buffer discard.
6. **Lease:** native and wasm clients serve before expiry, refuse at expiry,
   after learned revocation, after clock rollback and after unqualified restart.
   An idle heartbeat cannot extend the lease. A relay cannot issue a longer
   child lease. Include partial transaction and disconnect/reconnect cases.
7. **Isolation:** concurrent P/Q requests, blocking tasks, nested calls, error,
   cancellation and panic never reuse another view. Alternate SQL/model/cache
   paths give identical authorized bindings and no larger disclosure.
8. **Completeness:** enumerate every route/method and MCP operation from the
   actual dispatch tables. Adding an unclassified route makes the build fail;
   a supported route that accesses an unscoped store fails a controlled fixture.
9. **Cost:** record source grant checks, lease renewal rate, local read rate,
   memory and p95 before/after on a private fixture. No per-local-read source
   polling or speculative broader production benchmark is authorized here.

## 9. Delivery and rollback

Implement in reviewable increments without enabling a partial security claim:

- R1: protected grant model, verified identity adapter, view type and exhaustive
  route census; scoped mode refuses every unsupported data path.
- R2: query, export/acquire and metadata views with paired SQL/model controls;
  an instance may advertise only its implemented capabilities.
- R3: versioned bootstrap/lease, cache binding, live feed/relay revocation and
  local native/wasm serving checks; prove the complete P1 fixture above.
- R4: operator-approved deployment with exact source/config identity, one allowed
  and one denied smoke, current rollback and owned natural acceptance.

Each increment follows normal current CI, one non-author source review and
exact-main watch. Main deploys source; it does not select scoped mode or issue
a grant. The route move from temporary write-class protection occurs only with
its complete read authorization path and accepted R3 controls. Write-mode and
read-only-store semantics remain independent: a read-only server can serve an
authorized read, while a read grant can never authorize a mutation.

Rollback a scoped deployment by stopping its tenant-facing scoped listener,
revoking its leases and restoring the previous source/config through the normal
lane. Do not roll a scoped listener back to open reads, reuse expired grants,
rewind accepted history or remove revocation records. Existing single-principal
readers remain the separately operated fallback. The parent local-sync work
stays open until its implementation and consumer acceptance, not this document,
are delivered.
