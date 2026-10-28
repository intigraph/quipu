# Key-field IRI minting

Settles G9 of `terminusdb-gap-analysis.md` against
`stable-entity-references.md`. Board item aegis-osn.

## Verdict

Compatible, with constraints. The stable-reference rule is that an IRI
identifies the thing *independently of a label, hostname, role, store or
pendant*. Key minting breaks that rule only if the key is one of those things
standing in for the entity. It keeps the rule when the key fields *are* the
entity's identity, the way an ISBN is a book edition and `(host, port)` is a
network endpoint. The rule forbids deriving a *machine's* IRI from its
hostname, because a machine keeps its identity after a rename. It does not
forbid deriving an *endpoint's* IRI from the host and port that define it. So
key minting is a third registry minting mode (`opaque`, `label`, `key`), not a
change to the contract. The rule still holds: a role and its current host are
two entities. The host or endpoint may be keyed; the role stays opaque and
points at it through temporal facts.

## When minting is allowed

All of these must hold; the implementation refuses otherwise.

1. **Owner opt-in per namespace.** `[[quipu.namespaces]]` declares
   `minting = "key"`, and `types` must be non-empty. A keyed shape on a type
   nobody selected never starts minting. The declaration must be applied to
   the registry graph first, as for every mint.
2. **Exactly one keyed shape per type.** A node shape with
   `sh:targetClass T` carries `quipu:keyFields ( p1 p2 ... )`, a non-empty RDF
   list of predicate IRIs without repeats. Zero or two keyed shapes for `T`
   refuse.
3. **Key fields are exactly-one and typed.** The same shape declares each key
   field with `sh:minCount 1`, `sh:maxCount 1` and a kind: `sh:datatype`
   `xsd:string`, `xsd:integer` or `xsd:boolean`, or `sh:nodeKind sh:IRI`. The
   cardinality lets the shape-aware merge raise a divergent key value as a
   conflict instead of a union (below). The kind stops `5432` and `"5432"` from
   minting two IRIs; a mint value of the wrong kind refuses. Any other
   datatype refuses the shape until it has a canonicalizer (see below).
4. **Identity, not description.** This is a review rule for shape authors, not
   a runtime check. A field qualifies only if a change to its value means a
   different thing. Labels, names, titles, current hosts of a role, owners and
   statuses never qualify. If a field's value can change while the thing stays
   the same, the type uses opaque minting.

Labels are never key material. A mint request's `label` is written as
`rdfs:label` and does not affect the IRI.

## Hash vs lexical

`quipu:keyStrategy "hash"` (the default) or `"lexical"` on the keyed shape.

- **Hash:** `base32(sha256(input)[..16])`, which gives 26 lower-case
  characters, the same syntax as opaque keys. The input is
  `quipu-key-v1\n<T>\n` followed by `<p>\n<term>\n` for each key field in list
  order, where `<term>` is the N-Triples spelling (escaped, so a newline in a
  value cannot forge a boundary). Type and datatype are part of the input, so
  the string `"5432"` and the integer `5432` are different keys, and the same
  values under two types give two IRIs. A pinned test vector, reproduced with
  Python's hashlib, fixes the derivation. Any change needs a new version
  string, never an edit. Hash keys hide the values from someone reading the
  IRI, but anyone who can guess the values can recompute the hash. Do not treat
  hashing as protection for low-entropy private values; audience policy on the
  namespace still governs publication.
- **Lexical:** each value's lexical form (an IRI's full spelling),
  percent-encoded outside RFC 3986 unreserved characters and joined by `+`. For
  example, `https://public.example/ep/db.lan+5432`. It is readable, but it
  drops the type and datatypes, so the namespace must select exactly one type.
  The required per-field kind keeps the dropped datatype unambiguous.

Value canonicalization: JSON strings become plain literals, integers become
`xsd:integer` and booleans become `xsd:boolean`. `{"@id"}` gives an IRI and
`{"@value","@type"}` a typed literal of a whitelisted datatype. Key values are
limited to the kinds that have exactly one spelling per value: plain/`xsd:string`,
`xsd:integer` (canonical decimal within i64, so `05432`, `+5` and `-0` refuse),
`xsd:boolean` (`true`/`false`) and IRIs. Everything else refuses, both in the
shape and in the request: `xsd:dateTime` (`Z` vs `+00:00`), `xsd:int` and other
derived integers (leading zeros, and a different datatype IRI for the same
number), decimals, floats, language-tagged strings and empty values. Each can
join the whitelist only with a canonicalizer and a pinned vector. A key must
have one spelling or writers diverge. Quipu does not case-fold or
otherwise normalize strings. If hostnames are case-insensitive in a domain,
the shape adds a lowercase `sh:pattern` and writers send lower case.

## Repeats, retraction and changed key values

- **Repeat mint, same store or another writer:** derives the same IRI. If that
  IRI is live in the target graph with the same type and exactly these key
  values, the mint *converges*: it returns the IRI with `converged: true` and
  `tx_id: null` and writes nothing. This is the one deliberate exception to
  "repeating a mint is a new entity", and it is safe because the key is the
  identity.
- **IRI known but not live** (retracted, another type, another graph, other key
  values, or a hash collision): refuse. Restoring a retracted entity is a
  history operation, never a side effect of a mint. IDs are still never
  recycled for a *different* thing.
- **A key value changes** (the database moves to `db2.lan:5432`): that is a new
  entity by definition. Mint the new key, then record supersession on the new
  node (`dcterms:replaces` / `prov:wasRevisionOf` to the old IRI, with valid
  time). The old entity and its history remain. Things that refer to "the
  database" should refer to an opaque role entity whose temporal facts point
  at the current endpoint, as `stable-entity-references.md` already requires.
  An in-place edit of a key field is never a rename.

## Share, fork, merge and alias repair

- **Convergence across stores.** Two stores, or two forks, that mint the same
  key produce the same IRI and byte-identical type and key triples. A
  set-algebra merge unions them into one node. This is the alias class
  *prevented*, which the triple-level merge cannot *detect* afterwards (0/93
  in the merge paper's replay).
- **Divergent key values on one subject** can only come from a raw write that
  bypassed the mint. Because key fields are `sh:maxCount 1`, the shape-aware
  merge raises that as a decision rather than unioning two identities, and
  post-merge validation catches the rest.
- **Publication.** Key IRIs live in a registered namespace, so the existing
  audience policy and `share_scrub::enforce_destination` apply unchanged.
  Lexical keys put values in the IRI. That is acceptable only when the
  namespace audience may see those values, which it already sees in the key
  triples.
- **What it does not fix.** Key minting prevents *future* aliases for keyed
  types only. It does not repair the 93 recorded aliases: the semantic class
  (47) has no shared key, and the id-form class (46) would only have been
  prevented where a natural key existed. We do not claim a number. Existing
  opaque or legacy IRIs for a thing that is now keyed are not rewritten. They
  are joined to the key IRI with explicit `owl:sameAs` repairs (which the
  merge already preserves) or the alignment workflow. That keeps the
  stable-reference promise that existing IRIs and approved mappings stay
  unchanged.
- **Shape evolution.** Editing a type's `quipu:keyFields` or strategy changes
  the derivation, so it is an identity migration (new type or new namespace
  plus supersession links), never an in-place shape edit.

## Implemented here (minimal)

`quipu_entity_ref` / `POST entity_ref` / `quipu entity-ref mint` accept
`keys: {predicateIRI: value}` for `minting = "key"` namespaces only. Keys are
refused elsewhere, and on resolve. The mint path derives the IRI
(`src/entity_key.rs`), converges or refuses as above, and otherwise writes
type, optional label and key triples through the existing guarded RDF write
path.

## Follow-ups (not in this PR)

1. Refuse raw writes that retract or replace a key field on a key-minted
   subject, outside the merge path.
2. Refuse shape loads that change `quipu:keyFields` or the strategy for a type
   that already has key IRIs.
3. An optional `entity_ref verify` that recomputes a received key IRI from its
   key triples on share import.

## Test plan

Implemented in `src/entity_key_tests.rs` (in-memory stores only):

- Two independent stores mint the same type and keys and get one IRI. A label
  on one side does not change it. A repeat converges with no transaction and
  no new facts. The node carries type and key facts.
- The pinned hash vector, reproduced outside Rust.
- A changed key value, a different type, or a string where the other side used
  an integer gives a different IRI.
- Missing, extra, float, empty, non-canonical, non-scalar, wrong-kind,
  language-tagged, `xsd:int` and `xsd:dateTime` key values refuse. Typed-object
  and bare spellings agree. A string port where the shape says integer refuses.
- Shape contract: no keyed shape, two keyed shapes, `sh:maxCount 2` on a key
  field, a key field without a kind, a non-whitelisted datatype, or an unknown
  strategy refuse. An IRI-kind key accepts `{"@id"}` and refuses a string.
- A raw write that puts a live, typed node at the key IRI with a different port
  makes the mint refuse; this test fails if convergence checks only the type.
- Gates: a key namespace without `types` refuses at configure. A key namespace
  without keys, keys on an opaque namespace, and keys on resolve all refuse.
- Mint after retracting the entity refuses rather than reviving it.
- Lexical: readable and percent-encoded, and refused when the namespace selects
  more than one type.

Still to add with the follow-ups: a merge-operator test (two forks mint the
same key and the merge yields one node with zero decisions; a forced divergent
port raises one decision), plus the raw-write and shape-edit refusals.
