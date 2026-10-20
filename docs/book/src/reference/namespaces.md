# Stable entity references and namespaces

An IRI identifies an entity, rather than its mutable display label or the store
that currently knows it. Link to the role when you mean a job; link to the host
when you mean the hardware. Renaming a label does not change an IRI. Existing
IRIs retain their meaning; this feature does not infer sameAs or migrate data.

## Configure and list

Declare namespaces in `.bobbin/config.toml` or the normal user config:

```toml
[[quipu.namespaces]]
prefix = "public"
# Defaults: public visibility/audience, opaque keys, this public base.
base_iri = "https://w3id.org/intigraph/"
minting = "opaque"

[[quipu.namespaces]]
prefix = "company"
base_iri = "https://knowledge.example/id/"
visibility = "private"
audience = "org:example"
resolver_url = "https://knowledge.example/entity?iri={iri}"
# Optional exact IRI allowlists; empty means unrestricted.
types = ["https://example.org/vocab/Host"]
graphs = ["urn:quipu:graph:root"]
```

No namespace is registered by an empty config, and the w3id example does not
assert that a redirect has been registered. Use your approved namespace and
resolver target. New namespace declarations default to public; a private
namespace is an explicit option. Neither URLs nor credentials are discovered.

```sh
quipu namespaces list --db local.db
quipu namespaces apply --db local.db
```

Listing is read-only. `apply` explicitly persists owner declarations in the
reserved `urn:quipu:graph:namespaces` graph. Config and retained policy must
agree; dropping a config entry does not erase retained private policy. Changing
an existing declaration or overlapping its base refuses and needs a separate
explicit migration. Imported graphs do not configure the owner registry.

Registry records use the new
`https://w3id.org/intigraph/vocab/namespaceDeclaration` predicate. Existing
ontology and entity IRIs are preserved; this does not migrate legacy vocabulary.

REST `GET /namespaces` (also `POST /namespaces` with `{}`) and MCP
`quipu_namespaces` expose the same effective list, without applying config.
There is no remote namespace-policy application endpoint.

## Mint and reference

A mint request selects a registered namespace and a type IRI:

```json
{"namespace":"company","type":"https://example.org/vocab/Host","label":"Compute node"}
```

```sh
quipu entity-ref mint request.json --db local.db
```

Run `namespaces apply` before minting; a declaration alone cannot allocate IDs.
This keeps policy durable if configuration is later removed.

The result contains the committed `iri` and `tx_id`. Opaque keys encode 128 bits
of OS entropy in 26 lower-case base32 characters. Keep the returned IRI;
changing its label does not mint another identity. An uncertain mint response
must be reconciled from retained evidence before another mint: retrying creates
a different entity. Explicit label minting accepts only letters, digits, `-`
and `_`, and never reuses an already allocated key, even after retraction.
Graph/type selectors apply before the normal guarded RDF write.

Authenticated REST `POST /entity_ref` and MCP `quipu_entity_ref` accept the same
fields plus `"action":"mint"`. Existing episode and raw RDF identities remain
compatible; use this registered path when you want new stable identities.

A reference descriptor carries `iri`, optional `source` and optional `asOf`:

```json
{"iri":"https://knowledge.example/id/example-key","source":"pendant-v1","asOf":"2026-10-20T00:00:00Z"}
```

```sh
quipu entity-ref resolve reference.json --db local.db
```

Resolution returns the registered owner resolver and exact reference. It makes
no network requests. Source hints do not establish trust or change identity.
`asOf` is a query parameter, never part of the minted key. The existing entity
page is current-state; use temporal query/history APIs for a historical view.
The descriptor reports this limitation instead of claiming a pinned rendering.

Markdown may link directly to an IRI; frontmatter `entities:` may list IRIs;
code comments may use `quipu: prefix:key` with their registry context. These
references can feed explicit Bobbin/anchor links; no reactor is launched.

## Audience-matched pendants

Public namespace IRIs can go everywhere. Restricted IRIs can go only to a
matching audience: `org:<group-id>` or `personal:<owner-id>`. Protected owner
configuration binds an actual receiver store ID to that audience:

```toml
[[quipu.namespace_destinations]]
store_id = "the-actual-receiver-store-id"
audience = "org:example"
```

Configure that binding on sender and receiver, and matching namespace policy on
the receiver. Apply the namespace registry on both stores before transfer.
A manifest or request cannot grant membership. Use the receiver's
existing stable store identity, not a copied identity or an invented name.

```sh
quipu share --output team.pendant --no-shapes --db local.db \
  --destination '{"audience":"org:example","store_id":"the-actual-receiver-store-id"}'
quipu import team.pendant --db receiver.db \
  --destination '{"audience":"org:example","store_id":"the-actual-receiver-store-id"}'
```

The existing manifest destination binds that declaration into share identity.
Import checks the actual receiving store ID and its own protected policy before
staging, while retaining ordinary integrity, source, attestation and trust gates.
Matching namespaces travel as policy metadata, never as authority to change the
receiver's configuration. Personal transfers require an explicit matching owner
binding; they are not granted by a generic internal flag.

The existing publication seam examines all emitted files, including predicates,
datatypes, shapes, queries, nested manifest keys/values and escaped IRI spellings.
It refuses mismatched audiences and never rewrites identity. Default outward
publication still requires the existing identifier-policy catalogue. Legacy
`--destination internal` remains internal, but does not prove org membership and
cannot override restricted namespace policy. REST share requests cannot set an
internal destination. Use authorized transfer channels; audience metadata is
not authentication or encryption. Org transfers still run the existing
identifier-policy catalogue, exempting only permitted namespace IRIs in a
private inspection copy; unrelated blocked identifiers continue to refuse.
