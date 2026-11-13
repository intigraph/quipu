# Scoped current reads

The source implementation provides a default-off, dedicated read-only HTTP
profile. It is an R1/R2 implementation increment, not bootstrap, replication,
offline serving or deployment acceptance. Existing single-principal instances
retain their existing profile. Compiling or installing this source issues no
grant and changes no serving configuration.

## Authority and identity

`quipu::read_authorization::OwnerReadPolicy` is trusted owner-local
administration. It owns a separate policy database and never exposes its raw
store to tenant handlers. Tenant RDF, imports, mappings and write grants cannot
modify effective read policy. A version-1 `ReadGrant` names one exact principal,
exact graph IRIs, operations, validity interval, issuer, revision and an offline
ceiling of at most 300 seconds. The ceiling records a bound; this implementation
does not issue offline leases. Revocation is a new, retained grant revision.
The revision and persistent authorization generation commit in one transaction.

The owner must protect the policy file and its directory from tenant processes,
and must never expose that file as tenant data or an attachment. Local code
that can open an arbitrary raw store or rewrite owner files remains trusted
administration. This API does not isolate processes sharing that OS authority.

Named credentials use the existing verifier-only registry. Shared writer bearers
and copied actor/session headers are not scoped read identity. Enrolled signed
requests use the existing verifier, nonce settlement and protected session
binding; the enrolled agent must be a qualified principal IRI. Existing signed
request enrollment restrictions still apply. Identity and an exact read grant
are both required. The serving adapter checks the current registry or session
binding before reading, then checks it again before returning buffered output.
Delegation is unsupported and never widens a grant.

## Explicit profile

An operator may select `[quipu.server.scoped_reads]` with `policy_path` and
`owner` only through a separately admitted rollout. The policy must already
exist. The source profile requires Unix, `read_only = true`, an existing named
credential registry and a physically distinct policy/data file. Invalid policy,
hard-linked policy/data files, attachments, replicas, relays, process workers,
strict-session-only configuration and `--mcp-stdio` refuse startup. There is no
fallback to legacy reads on an error.

Only these capabilities are implemented:

| Method and route | Input | Operation |
|---|---|---|
| POST `/query` | `query`, optional `graphs` array | query |
| POST `/export`, POST `/acquire` | optional `graphs` array | export |
| GET `/graphs` | no query string or body | metadata |
| GET/HEAD `/health`, `/version` | no tenant parameters | fixed minimal public response |

Other routes, methods, aliases and unknown request fields refuse. Native MCP,
stored queries, search, inference options, historical reads, attachments, external
services, graph-store access, global diagnostics and continuous subscriptions
are unsupported. In particular `/share/asof`, `/replicate/stream` and
`/replicate/challenge` retain their existing write classification and refuse in
this read-only profile. Native MCP transport is not published by this profile.
The existing REST dispatch census and a frozen method inventory fail tests when
an unclassified route or method is added.

## View semantics

An omitted dataset selects exactly the operation's granted graphs. An explicit
dataset must be nonempty and wholly permitted; mixed, unknown and denied graph
requests receive the same public denial. ROOT and context graphs need explicit
grants. The preflight checks every `FROM`, `FROM NAMED` and `GRAPH` occurrence,
including nested or skipped branches and expressions. Remote `SERVICE` refuses.

One source read transaction builds a bounded private current-state view, copying
only authorized facts and graph identities. It copies no global dictionary,
history, registry, ontology, labels, search vectors or operational records. SQL
and resident-model execution use that same private store. Tenant computations
cannot obtain a raw source store from the capability. Bounds are 100,000 facts
and 16 MiB serialized RDF; exceeding a bound refuses the whole view. This is a
bounded materialization implementation, with a private fixture cost rather than
a claim about production request rate or large-store performance.

Query responses use W3C results JSON or N-Triples. Export/acquisition returns
`quipu-scoped-current/v1`, an exact graph-to-N-Triples map and `complete: true`.
It is a current export, not an existing as-of or acquisition-v1 source receipt.
Metadata returns authorized graph names and current fact counts only. Buffered
data responses are private/no-store. No ordinary response includes a global
source head, actor, hidden graph name or hidden activity count.

The serving adapter rechecks fresh policy, credential validity and source
lineage/epoch before returning buffered output. A backward clock observation
invalidates the request view. This does not implement R3's serialized publication
permit or revoke bytes already handed off.

## Remaining admission

R3 still owns versioned leases, publication/revocation barriers, cache/resume
binding, feeds, relay propagation and native/Wasm offline serving. R4 still owns
source/config identity, operator admission, allowed/denied production smokes,
rollback and natural acceptance. No runtime transition follows from R1/R2 source
delivery. Rollback of an eventual scoped listener stops that listener; it never
turns a tenant listener into open reads.

### Dedicated HWH memory topology (source-only)

An optional `memory_rpc` object within `server.scoped_reads` selects a dedicated
memory-only topology. It has the existing file-memory fields `enabled`, `graph`,
`deployment`, and `project`. This mode admits only POST `/memory/read` and
`/memory/export`, besides minimal health/version. Generic query/export, metadata,
projection, writes, native MCP, streams, caches/resume and unknown routes remain
denied. Do not expose an unguarded listener against the same private corpus.

The dedicated memory mode accepts verified ordinary named credentials only.
Attested sessions have a separate revocation authority and remain denied here
until that authority participates in the publication barrier. Existing generic
scoped-query behavior is unchanged.

The authenticated principal must match the requested owner crew, or exactly the
configured policy owner (admin). Both still need an existing 44r Export grant
for the configured graph; write credentials and a shared bearer supply no read
authority. Responses are computed from a bounded request-owned graph view and
contain only the selected owner records. Deployment/project mismatches deny.

The final policy recheck holds a shared publication permit on the existing
policy inode. The buffered body retains that permit through frame handoff or
cancellation. Owner `OwnerReadPolicy::apply` needs an exclusive permit and refuses
with UNKNOWN/busy before mutation if a body is still held. A successful revision
ACK therefore cannot precede release of an older response. The finite grant
window is checked again when the body frame is polled; expired/backward-clock
buffers produce a body error without a private frame. This is a buffered HTTP
publication boundary, not a promise to erase bytes already handed to a client.

Use the owner API for policy mutation. File replacement, raw SQL/RDF changes,
registry replacement as an instantaneous revocation API, offline leases and
streaming are unsupported. Missing/symlinked policy authority fails closed;
no serving operation initializes a policy or copies credentials. Unix file-lock
support is required for this barrier. A stalled client can make mutation refuse;
no successful revoke is reported in that case.

Selecting this profile requires synthetic qualification, peer review and managed
topology admission. A source change alone does not activate private uploads.
