# Per-install instance identity and scoped allocation for non-infra writers

Board item aegis-m5s.3.2, child of aegis-m5s.3. Design only. **This document
grants no runtime switch, enrollment, registry apply, principal grant,
allocation or stored-data change.** It chooses no deployment base: the one
naming choice it needs is the ASK at the end, routed through the lead.

## Governing rulings (inputs, not choices made here)

- Stiwi, relayed by Hammond (m5s.3 c6): instance and private-record identity
  is **install-selected**:
  1. a configured, resolvable, per-install base; or
  2. location-free identifiers such as `urn:uuid`.

  Tools support **both** and have **no hard-coded private host default**.
- This deployment uses **option 1**. The replacement for `gastown.local` has
  **not** been chosen.
- Hammond's "unset base falls back to option 2" is a proposal, not a ruling.
  It is dispositioned below, not adopted.
- No invented namespace, no owner-key policy beyond what this design proposes
  for decision, no general-operator mint. Existing instance IRIs remain until
  this design is decided.
- Sattler (m5s.3 c5): the six rt9f.5 infrastructure classes do not cover
  Reaction/Firing, workflow/gate or memory-record allocation; those need this
  separate design.

## Scope boundary with rt9f.5

rt9f.5 owns six infrastructure allocation types (Host, LXC, Deployment,
Terraform declaration, Address, Document) through the accepted Gold
declaration: namespace `aegis-infra`, base
`https://w3id.org/intigraph/aegis/id/`, `minting = "key"`, graph
`urn:aegis:graph:infrastructure`. **This design does not touch that
declaration, its keyed shapes, its A/B gates, its mint-only writer or its
rollback binding.** Any non-infra scope proposed below must be a *distinct*
`[[quipu.namespaces]]` entry with a distinct prefix and a non-overlapping base;
the registry already refuses overlapping bases
(`docs/design/stable-entity-references.md`). Where a non-infra writer
references an infra entity it reuses the exact infra IRI, never re-derives it.

## 1. Classify every name before changing it

Each IRI a writer emits falls in exactly one class. The class decides what may
change.

| Class | Example today | Rule |
| --- | --- | --- |
| **Vocabulary term** (class, property, shape) | chaski `{NS}ReactionFiring`, shantytown `aegis:aboutGitHubEntity` | Belongs to a w3id vocabulary owned under m5s.1 (`https://w3id.org/intigraph/<x>/ns#`). **Never** derived from the instance base. Out of scope here except to separate it from instances. |
| **New instance allocation** | a new firing, run, gate verdict, memory record | Minted under the install's selected identity mode (section 2), inside the writer's own scope (section 3). |
| **Reference reuse** | a gate verdict naming a PR node; a run naming its workflow | Uses the exact IRI already stored or returned by its owner. Never reconstructed from a label, key or guessed suffix. |
| **Historical deterministic identity** | chaski `firing-<sha256(label, event_id)>` under `aegis.gastown.local` | Stays byte-for-byte. Readers keep recalling it. Never re-minted in place. |
| **Frozen bytes** (signatures, outbox, packs) | shuttle `registration_turtle`, signed knots, outbox payloads, frozen packs | Never rewritten. Signatures verify over the original bytes. A new identity is a new record, not an edit. |
| **Former-name alias** | `FORMER_NAME_LEGACY` in share builders, `DEFAULT_BASE_NS` read alternatives | Kept, labelled, read-only. Not a source for new allocation. |

The defect today is that one constant (`http://aegis.gastown.local/ontology/`)
plays three roles at once in chaski, shuttle signing and shantytown: vocabulary
prefix, instance base and alias. Implementation items must split those roles
into three named values before any value changes.

### Measured writers (read-only survey of current `main`, 2026-11-27)

| Repo | Site | Instance shape today | Allocation kind |
| --- | --- | --- | --- |
| chaski | `graph_sink.py` `firing_iri` | `{NS}firing-<sha256(label\nevent_id)[:32]>` | deterministic |
| chaski | `graph_sink.py` `reaction_iri` | `{NS}reaction-<label>` | label-derived (see 3.4) |
| chaski | focus records | under `NS` | deterministic |
| shuttle | `export.py` `run_iri`/`workflow_iri`/`event_iri` | `urn:shuttle:run:<id>` etc., `SHUTTLE_ENTITY_NS` override | deterministic from run id/seq |
| shuttle | `signing.py` `registration_turtle` | `urn:shuttle:registration:<agent>` with `aegis:` vocabulary | frozen bytes (human-loaded) |
| shantytown | `gate_verdict.py` | `{AEGIS}gate_verdict.<suffix>`, `{AEGIS}gate_approval.<suffix>`, `{AEGIS}github.<node>` | deterministic |
| shantytown | `shuttle_runs.py` | under `_AEGIS` | reference reuse of shuttle runs |
| camayoc | `cy record` | `http://aegis.gastown.local/ontology/mem/…` records and topics | random or keyed per record type |

Share/knowledge builders (`build-repository-share.py`, `build_stable_namespaces.py`,
`build_aegis_compatibility.py`) carry the legacy base as a **former-name alias**
and stay as they are. shuttle's `urn:shuttle:` is location-free but uses an
unregistered URN namespace; it is classified as a **former-name instance
scheme**: existing values remain readable, new allocation moves to section 2.

## 2. Install configuration contract

Every writer reads the same install-wide inputs. Each repo maps them onto its own
config surface (quipu `[quipu.instance_identity]`, environment for Python
tools); the *names* below are proposed for review, the *semantics* are the
contract.

| Input | Option 1 (resolvable base) | Option 2 (location-free) |
| --- | --- | --- |
| `mode` | `"base"` | `"urn-uuid"` |
| `base` | absolute `https://` or `http://` IRI ending in `/`, no query, credentials or fragment content | must be absent |
| `install_uuid` | not used | an RFC 9562 UUID chosen once per install; the UUIDv5 namespace for deterministic classes |

**Visibility and audience are per scope, not per install.** The registry sets
them on each namespace entry, and `Namespace::validate`
(`src/entity_namespaces.rs`) requires `public == (audience == "public")` and
admits `http://` only for a private entry. One install-wide value could not
express a public gate-verdict scope beside a private memory scope under the
same base. Each scope entry of section 3.2 therefore carries its own two
fields, which are inputs to this contract:

| Per-scope input | Values | Rule |
| --- | --- | --- |
| `visibility` | `"public"` or `"private"` | the registry's existing field; `public` requires an `https://` base |
| `audience` | `"public"`, `"org:<id>"` or `"personal:<id>"` | the registry's existing field and syntax; `public` iff `visibility = "public"` |

A private scope under an `https://` base is admitted (the registry's test
forbids only *public* over `http://`), so a public authority can name private
records without publishing them. Section 3.1 fixes which scopes must be
private.

**Base ends in `/` only: a deliberate tightening.** The registry admits bases
ending in `/` or `#`. This contract admits only `/`, because the instance
forms below append `{scope}/{class}/v1/{key}` path segments, and a `#` base
would put the whole instance path in the fragment, which the registry's
one-`#` rule and HTTP resolution both treat as non-addressable. A `#` base
refuses (refusal 2) even though the registry would accept it as a
vocabulary-style namespace.

Proposed environment names for non-quipu writers: `INTIGRAPH_INSTANCE_MODE`,
`INTIGRAPH_INSTANCE_BASE`, `INTIGRAPH_INSTANCE_UUID`. Writers never take
visibility or audience from the environment: they read their own scope
entry's values from the registry, so a writer cannot widen its own audience. A
per-tool override (e.g. `SHUTTLE_ENTITY_NS`) remains accepted only if it
agrees with the shared value; disagreement refuses.

### Instance IRI forms

- **Option 1:** `{base}{scope}/{class}/v1/{key}`, where `scope` is the writer's
  owner scope (section 3), `class` is a short fixed segment
  (`firing`, `reaction`, `run`, `event`, `gate-verdict`, `record`, …) and `key`
  is either the deterministic key (lower-case hex digest or the native opaque
  26-char base32 key) or a fresh opaque key. `v1` is the key-input version
  segment (below); it is present for random classes too, so every option-1
  IRI has the same shape.
- **Option 2:** `urn:uuid:<uuid>`, where deterministic classes use
  `UUIDv5(install_uuid, "<scope>\nv1\n<class>\n<key input>")` (exact form
  below) and random
  classes use UUIDv4 from OS entropy (entropy failure refuses). Scope and class
  are **not** recoverable from the IRI, so the record must carry them as facts
  (section 3.2).

The key input for deterministic classes is the same input each writer hashes
today (e.g. chaski `label\nevent_id`). The **canonical key string** is exactly
the UTF-8 bytes of

```text
<scope> LF v1 LF <class> LF <key input>
```

with `LF` a single U+000A, no trailing newline, `scope` and `class` the fixed
ASCII segments of sections 3.1 and above, and `key input` unchanged from
today's writer. Option 2 hashes this string with UUIDv5 under `install_uuid`;
option 1 uses the writer's existing digest of `key input` as `{key}`, with
scope, class and version carried as path segments instead of hashed. In both
forms the version `v1` is part of the identity (hashed in option 2, the
`/v1/` segment in option 1), so a later change of key input mints under `v2`
rather than colliding with a `v1` identity.

### Validation and refusals (before any write)

A writer that would allocate a new instance IRI refuses, writes nothing, and
exits non-zero with a message naming the missing input when:

1. `mode` is missing or not one of the two values;
2. `mode = "base"` and `base` is missing, relative, not `http://` or
   `https://`, has query/credentials/fragment content, or does not end in `/`
   (a `#` base refuses: the tightening above);
3. `mode = "base"` and `base` **overlaps** a **former-name** HTTP(S) base
   (`http://aegis.gastown.local/ontology/`,
   `https://camayoc.local/ontology/`) under the same two-way `starts_with`
   test refusal 4 uses: `base` equal to, under, **or above** a former name
   refuses. So `http://aegis.gastown.local/` and `https://camayoc.local/`
   refuse as well as `…/ontology/mem/`: a base above a former name would make
   every legacy IRI look like a new-allocation IRI of this install. Former
   names are read aliases, never new-allocation bases. (`urn:shuttle:` needs
   no entry here: refusal 2 already rejects any non-HTTP(S) base);
4. `mode = "base"` and `base` overlaps the base of any registered namespace
   **other than this install's own scope entries** `{base}{scope}/` for the
   configured `base` (section 3.2) — in particular the rt9f.5 `aegis-infra`
   base, and the scope entries of any **superseded** base, which stay
   registered (registry entries are immutable) and so keep a superseded base
   and its successor disjoint. The scope
   entries lie under `{base}` by construction and are exempt; they are checked
   against each other and against every other entry by the registry's
   ordinary two-way overlap rule (`src/entity_namespaces.rs` `insert`);
5. `mode = "urn-uuid"` and `base` is set, or `install_uuid` is missing or not a
   valid UUID (deterministic classes cannot converge without it);
6. the writer's bearer for the guarded write path is unset or empty. The writer
   never guesses identity headers or falls back to an unauthenticated route;
7. the store's **active** install declaration (below) is unreadable or disagrees with the
   configured identity inputs: `mode`, and `base` under `mode = "base"` or
   `install_uuid` under `mode = "urn-uuid"`, each compared byte for byte.
   Without this, an option-2 install whose configured `install_uuid` drifted
   from the recorded one would pass every other refusal and silently mint
   different UUIDs for the same deterministic key, breaking the convergence
   of sections 3.3 and 4; a drifted `base` would do the same under option 1;
8. the store holds **no** recorded install declaration. Allocation never
   creates the declaration implicitly; it is a precondition (below);
9. the writer's scope entry is missing, its `visibility`/`audience` pair fails
   the registry's test (`public` scope over `http://`, or `public` visibility
   with a non-`public` audience or the reverse), or a scope that section 3.1
   marks **private-record** is declared `visibility = "public"` or
   `audience = "public"`. The last case is refused by the new scope-policy
   check (Quipu gap 3), not by the registry, which would accept it.

Reference reuse and reads never require these inputs: a reader with no
configuration still recalls every stored record.

### Recorded install declaration

Before any allocation, an operator records the install declaration
(mode, base or install UUID, timestamp, principal; versioned, below) in the
protected namespace registry graph `urn:quipu:graph:namespaces` through a
local, explicit CLI apply step that **does not exist yet** (Quipu gap below).
The current `quipu namespaces apply` (`src/entity_namespaces.rs` `apply`)
cannot carry it: it serializes only `Namespace` entries, as
`<urn:quipu:namespace:<prefix>>` facts on
`https://w3id.org/intigraph/vocab/namespaceDeclaration`, `Namespace` is
`deny_unknown_fields`, and `persisted()` parses every fact on that predicate
as a `Namespace` and errors on a mismatch. Writing the declaration on that
predicate would therefore break every registry read.

The declaration holds no visibility or audience: those belong to the scope
entries. The declaration is a **distinct record kind** (proposed subjects
`<urn:quipu:instance-identity:v<n>>`), **not** a `[[quipu.namespaces]]` entry with
base `{base}`. It holds no prefix, mints nothing and does not take part in
the namespace overlap test. Were it a namespace entry, the registry's two-way
`starts_with` test would refuse every scope entry `{base}{scope}/` beneath it.
Only the per-scope entries of section 3.2 are namespace entries. Like
`apply`, the new step is local CLI only, and the registry already requires
`apply` before a mint, so the declaration is a
**precondition**, not a side effect of the first write: a writer that finds
no declaration refuses (refusal 8). Neither startup nor a writer ever writes
it. Effective policy is
config ∪ persisted, and any disagreement in mode, base or install UUID
refuses (refusal 7, mirroring the registry's existing rule), so neither
dropping nor editing config can silently switch an install's identity.

### Declaration versions, supersede and the active declaration

The declaration is append-only and versioned, mirroring the registry's
immutable entries (`insert` refuses to reclassify a prefix):

- **Subjects.** Each version is its own subject
  `<urn:quipu:instance-identity:v<n>>` (proposed), `n` a positive decimal
  integer with no leading zero, starting at `1`. A version, once written, is
  never edited or deleted.
- **`declare-install` writes only `v1`.** On a store with no declaration it
  writes `v1` and reads it back. On a store whose `v1` holds byte-identical
  values it is a no-op that still performs the read-back. On a store with any
  declaration whose values differ, or with any version above `v1`, it
  **refuses with no write** and names the existing active version. It never
  replaces, supersedes or deletes a declaration.
- **`supersede-install` is the only way to change identity** (proposed
  `quipu namespaces supersede-install`, local CLI only, like `apply`). Its
  required inputs are the expected active version number and its exact values
  (a compare-and-swap guard), the new values, and the operator principal. It
  refuses with no write when the expected version or values do not match the
  store's active declaration byte for byte (a stale or concurrent supersede),
  when the new values fail refusals 1–5, or when they are byte-identical to
  the active values. Otherwise it writes `v<n+1>` carrying the new values, a
  `supersedes` link to `v<n>`, timestamp and principal, then reads back: the
  active-declaration read must return `v<n+1>` with the new values byte for
  byte, or the step reports failure.
- **Active declaration.** The declaration reader returns the single version
  that no other version supersedes. It refuses (and every allocating writer
  therefore refuses, as refusal 7) when there is no `v1`, when the versions
  are not a gap-free chain `v1 … v<n>` each superseding exactly its
  predecessor, when two versions supersede the same predecessor (a fork), or
  when any version fails the field checks of gap 1. Writers apply refusals 7
  and 8 against the active version only; superseded versions stay readable
  as provenance for the records minted under them.
- **Rollback of a supersede** is another `supersede-install` that re-declares
  the prior values as `v<n+2>`. Nothing is deleted. Records minted under
  `v<n+1>` stay valid identities (section 7). Deterministic classes do not
  depend on the rollback restoring old values: every deterministic writer
  applies **cross-version convergence** (section 4), so an entity keeps the
  IRI it was first minted under — whether that was under `v<n>` or
  `v<n+1>` — across both the supersede and its rollback.
- **Writers pick up a supersede only by re-reading.** A writer reads the
  active declaration before each allocation batch; a writer whose configured
  inputs still match the superseded version then refuses under refusal 7
  until its configuration is updated, so no writer mints under a stale mode.

Quipu gaps to close in the implementation item (new code, not configuration):

1. **Install declaration record.** Versioned subjects
   `<urn:quipu:instance-identity:v<n>>` (proposed, section above) in
   `urn:quipu:graph:namespaces`, on a **distinct predicate**
   (proposed `https://w3id.org/intigraph/vocab/instanceIdentityDeclaration`,
   final name decided in that item, never `namespaceDeclaration`), with its
   own serde type, its own apply steps (proposed
   `quipu namespaces declare-install` and `quipu namespaces supersede-install`)
   and its own read path. `persisted()`
   keeps reading only `namespaceDeclaration`, so registry reads are
   unaffected; the declaration reader refuses a chain that does not resolve
   to exactly one active version, an unknown field, or a value that fails
   refusals 1–5. Read-back is
   exact: after apply, the declaration read returns the same mode and base
   or install UUID byte for byte, or the apply reports failure. Every
   allocating writer reads this declaration before minting and applies
   refusals 7 and 8 against it: absent refuses, and any byte difference in
   mode, base or install UUID from the writer's configuration refuses.
2. **URN-base entry kind.** The registry currently admits only HTTP(S) bases,
   while `namespace::require_base_ns` already admits `urn:`. Option 2 needs a
   registry entry kind with no base prefix (`mode = "urn-uuid"`,
   `install_uuid`, `types` allowlist, and the same per-scope `visibility` and
   `audience` fields and pairing test as a namespace entry) whose owner scope
   is enforced from facts, not from IRI prefix.
3. **Scope-owner write admission and scope policy.** On the guarded write
   path, a write whose subject lies under a scope base `{base}{scope}/`
   (option 1), or carries that scope's allocation fact (option 2), is
   admitted only for that scope's owner principal; any other principal
   refuses with no write. The same check refuses a private-record scope
   (section 3.1) declared public, at apply time and again at write time,
   and refuses at apply any scope named `legacy-alias` (reserved, section 3.1).
   Option-2 admission additionally applies the subject rules of section 3.2
   (no unscoped first write, server-side UUID recomputation, UUID version
   check). Today the registry checks types and graphs only; this admission and
   policy check is new code, and acceptance 4 tests it.

## 3. Owner key and scoped allocation

### 3.1 Scopes

One owner scope per writer. A scope is *(owner principal, allowed types,
visibility, audience)*; a principal allocates only inside its own scope.

| Scope | Owner principal | Allocates | Visibility / audience |
| --- | --- | --- | --- |
| `chaski` | chaski reaction runtime | firing, reaction, focus | install-chosen |
| `shuttle` | shuttle exporter | run, workflow, event | install-chosen |
| `shantytown` | shantytown gate writer | gate verdict, gate approval, GitHub entity mirror | install-chosen |
| `camayoc` | `cy` record writer | memory record, topic | **private-record:** `private`, audience `org:<id>` or `personal:<id>` |

The scope name `legacy-alias` is **reserved** and may never be registered as
a scope: it is the first segment of the migration alias input (section 4),
so reserving it keeps the alias and new-mint UUIDv5 input domains disjoint.
Declaring a scope with that name refuses at apply.

*Install-chosen* means the operator sets the pair on the scope entry, subject
to the registry's test. *Private-record* scopes may never be public on any
install, under either option (refusal 9). The `<id>` is the install's existing
destination-binding audience id, not a new name minted here.

The scope list is the proposed owner-key policy **for decision**; it is not
enacted by this document. There is no general-operator mint: an operator who
needs a record in a scope runs that scope's writer. Verifier registrations
stay human-loaded and keep their current identity (frozen bytes).

### 3.2 Enforcement

- **Option 1:** the scope is a `[[quipu.namespaces]]` entry per writer, base
  `{base}{scope}/`, with a `types` allowlist, opaque or key minting, and the
  scope's own `visibility` and `audience`. The guarded write path already
  refuses a mint outside the declared types and graph. New (Quipu gap 3): a
  write whose subject lies under a scope base is admitted only for that
  scope's owner principal.
- **Option 2:** the IRI carries no scope, so each allocated record carries an
  allocation fact naming its scope and owner. The predicate is taken from the
  loaded w3id vocabulary (m5s.1 ownership ledger); **this design does not mint
  one**. If no suitable term exists, the implementation item files a schema
  change proposal through `quipu_propose_schema_change` first.
  **Option-2 scope enforcement is blocked until that m5s.1 term exists.** No
  writer may allocate option-2 instances before then, because a record
  without an enforceable scope fact cannot satisfy read-back check 2
  (section 3.5). Option-1 enforcement does not depend on it.

**Option-2 subject admission (Quipu gap 3).** Option 1 is protected by path
prefix: a principal cannot name a subject under another scope's base. Option 2
has no prefix, and `install_uuid` is not secret, so any principal can compute
another scope's deterministic UUIDv5. The guarded write path therefore admits
a write that introduces a **new** `urn:uuid:` subject (one with no prior fact
in the store) only when all of the following hold, and otherwise refuses with
no write:

1. **No unscoped first write.** The same transaction carries the subject's
   scope allocation fact, naming a scope the writing principal owns. A first
   write of a `urn:uuid:` subject with no scope fact refuses. Writes to an
   existing subject and references to it as an object are reference reuse
   and are governed by the collision rules of 3.3, not by this rule.
2. **Deterministic subjects are recomputed, not trusted.** For a
   deterministic class the transaction also carries the class segment and the
   key input as facts (predicates from the same m5s.1 vocabulary block as the
   scope fact). The server recomputes
   `UUIDv5(active install_uuid, <canonical key string>)` using the
   **claimed scope** of rule 1, and refuses unless the result equals the
   subject byte for byte. A principal that pre-writes another scope's UUID
   under its own scope fact computes a different UUID, so the squat refuses
   and the owner's later write is never blocked by a 3.3 collision.
3. **UUID version matches the class kind.** A deterministic-class subject
   must be a version-5 UUID and a random-class subject a version-4 UUID (the
   RFC 9562 version nibble). A principal therefore cannot claim another
   scope's computable v5 UUID as a "random" record of its own.

Random v4 subjects are not computable from public inputs, so squatting one
requires guessing 122 random bits; that residual risk is accepted. The key
input fact is stored in the record's own graph under the scope's own
visibility and audience, so recording it discloses nothing beyond the record.

### 3.3 Collision and idempotency

- **Deterministic classes converge.** The same key input under the same
  install gives the same IRI. Re-writing an identical record is a no-op.
- **Collision refuses.** If the computed IRI already exists with a different
  `rdf:type`, a different owner scope, or a different key value on a
  shape-declared key field, the write refuses. It never overwrites or unions.
- **Random classes never converge.** A repeated random mint is a new entity.
  After an uncertain response (timeout, empty body, missing `tx_id`) the
  writer reads back by its natural correlation key (run id, event id, record
  source) before deciding whether to retry. chaski already treats a missing
  `tx_id` as indeterminate; every writer adopts that rule.
- **Cross-mode keys never collide.** An option-1 IRI and an option-2 IRI for
  the same key are different strings by construction. They are linked only by
  an explicit alias (section 4), never by inference.

### 3.4 Label-derived instances

chaski `reaction-<label>` derives identity from a label, which
`stable-entity-references.md` forbids for new allocation. New reactions use a
deterministic key that is the reaction's identity (its definition id), or an
opaque key with the label as `rdfs:label`. Existing `reaction-<label>` IRIs
stay as historical identities.

### 3.5 Exact read-back

HTTP 200 alone is not success. After every allocation the writer verifies, in
the graph it wrote:

1. the subject exists with the exact declared `rdf:type`;
2. the scope fact (option 2) or scope base (option 1) names the writer's own
   scope;
3. the response's `conforms` is not `false` and a `tx_id` is present;
4. the work-item or provenance link the writer is required to attach is
   present.

An error after a committed write is reconciled by read-back, not by retry.

## 4. Migration aliases

Default: **existing instances are not re-minted.** A new identity is only
introduced for new allocations. An alias is written only when an owner
deliberately gives an existing entity a new identity, and then it is an
additive `owl:sameAs` from the new IRI to the old one, carrying provenance with
a distinct source tag so it can be retracted as a unit. Readers accept both
directions of an already-stored former-name link, as the rt9f.5 helper does.

| Direction | Applies to | Alias rule |
| --- | --- | --- |
| legacy `aegis.gastown.local` → option 1 | chaski, shantytown, camayoc existing instances | optional per class; `new owl:sameAs old`; old stays recallable |
| legacy `aegis.gastown.local` → option 2 | same, on an option-2 install | same rule; the new UUID is chosen as below so re-emission converges on it |
| `urn:shuttle:` → option 1 or 2 | shuttle runs/workflows/events | same rule; signed registrations are never aliased in place |
| option 1 base X → option 1 base Y (same mode) | an install whose resolvable base changes, e.g. a renamed domain | a migration, not a config edit: `supersede-install` writes `v<n+1>` with base Y. **No alias facts are required before writers resume**: deterministic classes converge by cross-version lookup (below), so an existing entity keeps its `{X}{scope}/…` IRI and only keys never minted before get `{Y}{scope}/…`. The X scope entries stay registered (immutable) so owner admission still covers writes to X subjects. Aliases from a Y IRI to an X IRI are optional, owner-deliberate, `new owl:sameAs old` |
| option 1 base Y → option 1 base X (rollback of the row above) | the same install | a further `supersede-install` re-declaring X as `v<n+2>`. Entities first minted under Y keep their Y IRI by the same lookup; no alias is required and none is removed |
| option 1 → option 2, option 2 → option 1 | an install that later changes mode | a migration, not a config edit: `supersede-install` writes the next declaration version (section 2); deterministic classes converge by cross-version lookup (below), so no alias is required before writers resume; per-class alias facts are optional; both versions and both IRI sets stay readable; startup never performs it; reversed by a further supersede |
| legacy vocabulary → w3id vocabulary | terms, not instances | owned by m5s.1 (`owl:equivalentClass`/`equivalentProperty` and the published mapping); **never** `owl:sameAs` between instances and terms |

**Cross-version convergence for deterministic classes.** A supersede changes
the active values, so a naive re-emission of an existing key would mint a
second IRI for an entity that already has one, which contradicts the default
above. Every deterministic writer therefore resolves the IRI for a key as
follows, before any write:

1. **Candidates.** Compute the key's IRI under the writer's legacy form `v0`
   (its former-name constant, only for a class that had a deterministic
   legacy form, e.g. chaski firing, shantytown gate verdict) and under the
   values of every declaration version `v1 … v<n>` in the active chain,
   using each version's own mode, base or `install_uuid`. Versions with
   byte-identical values give the same string and are one candidate (so after
   a rollback `v1` and `v3` are one candidate).
2. **Lookup.** Read each candidate by exact IRI in the writer's own graph. A
   candidate **exists** when it has an `rdf:type` fact there. A read that
   fails, times out or is truncated is indeterminate: the writer refuses and
   mints nothing (an unknown is not an absence).
3. **Decision.** No candidate exists: mint under the active version. Exactly
   one exists: reuse it — this is reference reuse, not a new allocation, and
   the collision rules of 3.3 apply to it. More than one exists: reuse the one
   that is the subject of an `owl:sameAs` to every other existing candidate
   (the owner's deliberate new identity, section 4); if there is none, refuse
   and report the candidates as a duplicate identity for the owner to alias.
4. **Admission and read-back of a reused subject.** A write to a reused
   subject is admitted under the scope it was minted in — the path prefix of
   that version's scope entry (option 1) or its stored scope fact (option 2) —
   which must be the writer's own scope; read-back check 2 (section 3.5)
   accepts that scope base or fact. A reused `v0` legacy subject has no scope
   entry; it is admitted exactly as that writer's legacy writes are admitted
   today, and this design adds no new admission for it.

The lookup is bounded by the chain length plus one. Random classes are not
re-emitted under an old key and need no lookup. Under this rule, X → Y and its
rollback Y → X need no alias facts before writers resume, and an entity first
minted under any version keeps that IRI through every later supersede.

**Canonical option-2 UUID for a migrated entity.** There is exactly one
canonical new IRI per entity, so a writer that later re-emits the entity
converges on the alias target instead of minting a second `urn:uuid`:

- **Deterministic class:** the alias target is the ordinary new-mint value,
  `UUIDv5(install_uuid, <canonical key string>)` (section 2), computed
  from the entity's original key input (section 2), **not** from the legacy
  IRI string. A migrated firing and a freshly re-emitted firing for the same
  key are therefore the same IRI. If the key input cannot be recovered from
  the stored record, that entity is not aliased; it stays a historical
  identity.
- **Random class:** no writer re-emits a random entity under its old key, so
  convergence by recomputation does not apply. The alias target is
  `UUIDv5(install_uuid, "legacy-alias\n<legacy IRI>")`, a separate input
  domain that cannot equal any new-mint input, because `legacy-alias` is a
  reserved scope name (section 3.1) and every new-mint input begins with a
  registered scope. Re-running the migration
  reads back the existing alias by the legacy IRI before writing, so it is
  idempotent.

The same rule applies to option 1: the alias target is the IRI the writer
would mint today for that key (deterministic) or a migration-recorded opaque
key read back before reuse (random).

Subclass or `closeMatch` never implies `sameAs`. No alias is manufactured from
a label match.

## 5. Historical identities, signatures and outbox bytes

- Deterministic historical IRIs (chaski firing hashes, shantytown gate suffixes)
  keep their exact bytes. New installs compute new IRIs from the same key input
  under the new mode; the old IRIs remain valid references.
- Signed payloads (shuttle signing, signed knots, verdict witnesses) verify over
  the bytes that were signed, including any legacy IRI inside them. A
  re-signature is a new record.
- Outbox entries and frozen packs are replayed or recalled byte-identically;
  no migration touches them. A replayed legacy outbox entry keeps its legacy
  subject.

## 6. Unset base: disposition

**Recommendation: refuse.** With no `mode` configured, a writer that would
allocate a new instance refuses (section 2, refusal 1); reads and reference
reuse continue. This matches quipu's existing `require_base_ns` stance
("Quipu has no default namespace") and the ruling's "no hard-coded private host
default".

Hammond's unset → option 2 proposal is **not adopted** as a silent fallback,
because an implicit option 2 without a recorded `install_uuid` cannot converge
deterministic classes across restarts, and an install that later configures
option 1 would hold an unrecorded mix of identity schemes. Option 2 remains
fully supported when **explicitly** selected. If Stiwi prefers the fallback,
it is option C in the ASK below, and it still requires a recorded
`install_uuid`.

## 7. Rollback

- **Source rollback:** a normal revert of the writer change. Writers return to
  their previous constants. No data is deleted.
- **Records minted under the new mode stay.** They are valid identities;
  readers accept old and new throughout the transition, so a revert leaves
  them recallable.
- **Alias facts** are additive and carry their own provenance source; they are
  removed, if ever needed, by retracting that source, never by deleting rows.
- **Install declaration:** reverting config does not erase the persisted
  declaration. Changing it is an explicit `supersede-install` (section 2),
  and undoing that is a further supersede back to the prior values; no
  version is ever deleted, so a rollback cannot silently fork identity. This
  holds for a base change within option 1 (X → Y → X) as for a mode change:
  cross-version convergence (section 4) keeps every entity on the IRI it was
  first minted under, so a rollback neither requires nor removes alias facts. A
  source revert to a writer that predates the declaration does not consult it
  and returns to the previous constants as above.

## 8. Acceptance for implementation items (per repo)

1. Before → after counts of `aegis.gastown.local` occurrences, and of the
   legacy vocabulary base `https://scbrown.github.io/quechua/ns#` (the
   current `quechua:` prefix, a vocabulary term base in section 1's sense,
   reported separately and changed only under m5s.1), by class (section 1), measured with `git grep` on non-test,
   non-doc source.
2. Fresh disposable install, option 1: one real write per writer mints under
   `{base}{scope}/{class}/v1/`, zero legacy triples written; read-back per 3.5 passes.
3. Fresh disposable install, option 2: the same writes mint `urn:uuid:` with
   scope facts; a repeated deterministic write converges; a repeated random
   write is distinct.
4. Negative controls: unset mode refuses with no write; former-name base
   refuses, including a base above a former name
   (`http://aegis.gastown.local/`) and one under it; a new base nested in
   or above a superseded base's scope entries refuses; base overlapping `aegis-infra` refuses; a public `http://` base
   refuses; a scope entry overlapping another scope entry or another install's
   namespace refuses; an install declaration that carries a base prefix
   (i.e. was written as a namespace entry) is refused; mode disagreeing with the
   recorded declaration refuses; on an option-2 install, a configured
   `install_uuid` differing from the recorded one refuses with no write, and
   on an option-1 install a configured `base` differing from the recorded one
   refuses with no write; a scope named `legacy-alias` refuses at apply; no
   recorded declaration refuses; a principal
   writing into another scope refuses; a private-record scope (`camayoc`)
   declared `visibility = "public"` or `audience = "public"` refuses at apply
   and at write; a `#`-terminated base refuses; `declare-install` with
   values differing from an existing declaration refuses with no write;
   `supersede-install` with a stale expected version or values refuses with
   no write; a declaration chain with a fork or a gap refuses every
   allocation; on an option-2 install, a first write of a `urn:uuid:` subject
   with no scope fact refuses, a principal writing another scope's
   deterministic UUIDv5 under its own scope fact refuses (recomputation
   mismatch), and a version-5 subject presented as a random-class record
   refuses.
5. Positive overlap and audience control: on an option-1 install with an
   `https://` base, the install declaration plus all four scope entries
   `{base}chaski/` … `{base}camayoc/` apply together without an overlap
   refusal, with `shantytown` public and `camayoc` private under audience
   `org:<id>` side by side, and each writer then mints under its own scope.
6. Registry control: after the install declaration is applied, an ordinary
   `quipu namespaces` read still succeeds and lists only namespace entries
   (the declaration predicate is not parsed as a `Namespace`).
7. Legacy control: a store holding legacy-IRI records still recalls them after
   the change; a signed legacy payload still verifies.
8. rt9f.5 control: its six-type A/B gates and keyed shapes are unchanged.
9. Alias convergence (when a migration is run): a migrated deterministic
   entity and a fresh re-emission for the same key produce one IRI, not two.
10. Supersede and rollback control, run once for an option-1 base change
    (base X → base Y → base X) and once for a mode change: `supersede-install`
    from `v1` to `v2` reads back `v2` as active; writers configured for `v1`
    then refuse; before any alias fact is written, a deterministic
    re-emission under `v2` of a key first minted under `v1` reuses the `v1`
    IRI (one IRI, not two), and a key never minted before mints under `v2`;
    a second supersede back to the `v1` values reads back `v3` as active;
    under `v3`, a re-emission of a key first minted under `v1` reuses the
    `v1` IRI, and a re-emission of a key **first minted under `v2`** reuses
    its `v2` IRI rather than minting a `v1`/`v3` IRI; a key that exists under
    two candidates with no `owl:sameAs` between them refuses as a duplicate
    identity; a candidate lookup that fails refuses with no write; records
    minted under every version stay recallable throughout.

## 9. ASK Stiwi (one card, routed via the lead)

**Decision needed:** the option-1 base for non-infra instance records in this
deployment. The `gastown.local` replacement has not been chosen, and this
design does not pick it.

- **Option A (recommended):** reuse the accepted infra authority
  `https://w3id.org/intigraph/aegis/` with a path segment **sibling** to
  `id/` (the segment name is part of this decision; `id/` itself cannot host
  the scopes because `aegis-infra` owns that base and the registry refuses
  overlap), then per-writer scopes `…/<segment>/chaski/`, `…/shuttle/`,
  `…/shantytown/`, `…/camayoc/`, each its own namespace entry. **Constraint
  on the segment:** it must not be `ns`, and must not equal or prefix any
  m5s.1 vocabulary path under `https://w3id.org/intigraph/aegis/`, because
  section 1 forbids deriving vocabulary from the instance base and an
  overlapping segment would mix terms and instances under one path. The card
  asks Stiwi to name the segment (suggestion: `rec/`) subject to that check
  against the m5s.1 ownership ledger. *For:* one public naming decision
  already accepted for infra; public HTTPS; no second authority. *Against:*
  the w3id route/resolver still needs its separate target proof and upstream
  public-send step before it resolves; memory records under a public
  authority are named publicly even though the `camayoc` scope entry is
  private with an `org:`/`personal:` audience (section 3.1), so the w3id
  route for that scope must not redirect to a public resolver.
- **Option B:** a private, owner-controlled resolvable base on a domain Stiwi
  names. *For:* private records never sit under a public authority. *Against:*
  needs a new name; still must not be a hard-coded default in tool code.
- **Option C (only if Stiwi rules it):** adopt unset → option 2 as the
  documented fallback for other installs, with a recorded `install_uuid`. This
  does not change this deployment's option-1 ruling.

Until the card is answered, no writer changes its base and existing instance
IRIs remain.
