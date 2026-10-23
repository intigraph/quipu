# Stable entity references

## Contract and source boundary

This revision incorporates Stiwi comment 4 (2026-10-20T22:32:35Z). It supersedes
the first item-comment draft’s blanket private refusal; no code used that draft.

An entity's IRI identifies the thing, independently of a label, hostname, role,
store or pendant. A reference carries `iri`, optional `source` (a store/pendant
hint, never an instruction to fetch), and optional `asOf` (an RFC3339 query
parameter, never a component of the minted identity). A role and its current
host have different identities; temporal relationship facts connect them.
Renaming a label does not mint another identity. Neither subclass nor closeMatch
implies sameAs. Existing IRIs, labels and explicitly approved mappings remain
unchanged. This feature does not migrate them or manufacture redirects.

Current source has `QuipuConfig.base_ns`, label-derived episode node names,
`/entity?iri=...` content negotiation and separate temporal query/history APIs.
There is no namespace registry. `share_scrub::enforce_destination` is the shared
outward share/delta/pack boundary; its existing identifier-policy catalogue stays
mandatory. Raw RDF export is a local backup/debug operation, not a publication
attestation. Namespace controls are additional to existing policy, never a
substitute for it.

## Registry

`[[quipu.namespaces]]` declares `prefix`, `base_iri`, `visibility` (`public` or
`private`), `resolver_url` (optional owner-controlled HTTPS URL template with
exactly one `{iri}` placeholder), `minting` (`opaque`, default, or `label`),
`graphs` and `types` (exact IRI allowlists, empty means unrestricted), and
`audience` (`public`, `org:<group-id>` or `personal:<owner-id>`). New entries
default to public visibility/audience, opaque minting and the w3id public base.
Private namespaces are optional, never the default.
Bases are absolute HTTP(S) IRIs ending in `/` or `#`, without query, credentials
or fragment content. Private bases may use HTTP for owner-local services.
Public bases use HTTPS. Prefixes and bases are unique and cannot overlap.
An empty configuration registers nothing; no resolver target is guessed. The intended
public authority is `https://w3id.org/intigraph/`; its final route/resolver
requires the separate target proof and upstream public-send step.

Config is loaded by the normal configured store-open route. An explicit local
`quipu namespaces apply` persists the declarations in the reserved
`urn:quipu:graph:namespaces` graph as versioned configuration records. Merely
listing namespaces never updates that graph. Effective policy is the union of
configured and persisted declarations; disagreement, malformed registry records
or reclassification refuses. Omitting config cannot erase persisted private
policy. Editing existing bases/policies or deleting them requires a future
explicit migration, not a startup side effect. Raw trusted RDF/SQL writers can
change policy and retain their existing administrative authority; untrusted
pendant graphs are not registry authority.

`quipu namespaces list`, REST `GET /namespaces`, and MCP
`quipu_namespaces` list this effective registry. The REST/MCP list never applies
configuration or allocates an identity. Registry application is local CLI only.

## Registered writes and resolution

`quipu entity-ref mint <request.json>` and authenticated REST/MCP
`entity_ref`/`quipu_entity_ref` select a registered prefix, exact type IRI and
optional graph. The selected graph must already exist and satisfy selectors.
Minting requires the selected declaration to have been applied to the registry
graph first, so deleting config cannot erase an allocated identity’s audience.
Minting writes the typed entity and optional label through the existing guarded
RDF write path. It returns the committed IRI and transaction, not a promise of a
future write. Opaque keys use 128 bits of OS entropy encoded as 26 lower-case
RFC4648 base32 characters without padding; entropy failure refuses. Repeating a
mint is a new entity, so an uncertain response must be read back, never blindly
retried. Label minting is explicit, validated and collision-refusing. Legacy
`/episode`, raw RDF and `base_ns` remain compatible; they do not acquire new
identity semantics. Callers migrating to stable identities use the registered
mint path and then reference the returned exact IRI.

Resolve accepts an exact registered IRI plus optional source/asOf and returns a
reference descriptor and configured resolver URL. It performs no network fetch
or identity inference. A configured resolver is a template, not an automatic
browser localhost probe. The existing `/entity` renderer is current-state;
a pinned reference must use the existing temporal query/history API. The
reference response explicitly reports this limit, rather than presenting a
current-state page as historical evidence. Private references resolve on the
owner's Quipu, never through a public redirect. Tombstone/history retention uses
the existing temporal fact log; IDs are never recycled.

## Publication boundary

Outward share, delta and pendant/pack producers call the existing destination
seam. Namespace export is audience-matched, superseding the earlier blanket refusal.
Public identifiers may go everywhere. An org namespace may go only to a
destination bound to that exact org; personal identifiers stay with their owner
unless a destination has an explicit matching personal grant. Reject namespace
IRIs outside the destination audience in all emitted files, including predicates,
datatype IRIs, graph IRIs, shapes, queries, recursively nested manifest values
and keys, and escaped spellings.
Do not rewrite or anonymize private identity into a guessed public identity.
Extend the EXISTING destination value with an audience object
`{"audience":"org:example","store_id":"<actual-receiver-store-id>"}`. This
object is hashed into the existing manifest destination/provenance. Protected
local `[[quipu.namespace_destinations]]` binds that exact store ID and audience;
request declarations, imported graphs and source hints cannot grant membership.
On import the store ID must equal the actual receiving Store::store_id and
match its own protected binding, before staging any data. This permits an
actual second same-org store without inventing remote authentication. Matching namespace declarations travel inside share identity; the receiver
requires exact local owner-policy matches before staging and never installs
policy from the artifact. Transfers
still use existing trust/source verification and operator-authorized channels.
Legacy `internal` remains stamped but is NOT evidence of an org or personal
audience: restricted namespace identifiers still refuse without a bound
audience object. This adds no new bypass or public floor. Config must be available at the
producer, and retained registry policy travels in its owner database. Consumers
must not treat an artifact's own declarations as authority to declassify it.

## File references and anchors

Markdown links use the exact entity IRI. Frontmatter `entities:` contains IRIs;
a code comment may use `quipu: prefix:key` with its registry context. A `source`
hint disambiguates stores with differing knowledge; it does not change identity.
`asOf` pins the view, not the entity. Bobbin can promote explicit references into
strong edges and the existing eno48m anchor design can consume those edges.
This scope does not implement a reactor, copied code text, anchor automation,
CI mapping authority or the separate namespace migration.

## Verification and delivery

Use private in-memory/disposable stores. Prove registry persistence and config
union/conflict refusal, stable opaque syntax and distinct IDs, graph/type
selectors, label collision, legacy identities and no side effects on list.
Exercise CLI/REST/MCP contracts and positive public outward shares with a loaded
identifier catalogue. Reject private IRIs in subject/object/predicate/datatype,
queries, nested manifests/keys and escaped forms through the real native seam;
a real same-org share exports, imports and resolves in a second disposable
store with its actual store ID bound on both sides; the same export to public,
another org, a forged/unbound receiver or plain legacy internal refuses.
Internal shares remain marked internal. No live graph, key, namespace migration,
service deployment, schedule, 5af change or upstream PR is part of these tests.
Normal current CI, one peer source review and exact-main watch govern delivery.

## Exact upstream draft boundary

Prepare one `intigraph/` README and wildcard `.htaccess` redirect PR only after
the actual resolver target is proven. Show Stiwi the exact diff and PR title/body
once before opening. No final target URL is guessed here; the unfilled target
is a real external publication dependency, not a blocker on engine source work.
