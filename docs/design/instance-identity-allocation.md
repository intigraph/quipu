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

Every writer reads the same three inputs. Each repo maps them onto its own
config surface (quipu `[quipu.instance_identity]`, environment for Python
tools); the *names* below are proposed for review, the *semantics* are the
contract.

| Input | Option 1 (resolvable base) | Option 2 (location-free) |
| --- | --- | --- |
| `mode` | `"base"` | `"urn-uuid"` |
| `base` | absolute `https://` IRI ending in `/` (private owner-local bases may use `http://`), no query, credentials or fragment content | must be absent |
| `install_uuid` | not used | an RFC 9562 UUID chosen once per install; the UUIDv5 namespace for deterministic classes |

Proposed environment names for non-quipu writers: `INTIGRAPH_INSTANCE_MODE`,
`INTIGRAPH_INSTANCE_BASE`, `INTIGRAPH_INSTANCE_UUID`. A per-tool override
(e.g. `SHUTTLE_ENTITY_NS`) remains accepted only if it agrees with the shared
value; disagreement refuses.

### Instance IRI forms

- **Option 1:** `{base}{scope}/{class}/{key}`, where `scope` is the writer's
  owner scope (section 3), `class` is a short fixed segment
  (`firing`, `reaction`, `run`, `event`, `gate-verdict`, `record`, …) and `key`
  is either the deterministic key (lower-case hex digest or the native opaque
  26-char base32 key) or a fresh opaque key.
- **Option 2:** `urn:uuid:<uuid>`, where deterministic classes use
  `UUIDv5(install_uuid, "<scope>\n<class>\n<canonical key input>")` and random
  classes use UUIDv4 from OS entropy (entropy failure refuses). Scope and class
  are **not** recoverable from the IRI, so the record must carry them as facts
  (section 3.2).

The canonical key input for deterministic classes is the same input each
writer hashes today (e.g. chaski `label\nevent_id`), versioned with a leading
`v1\n` so a later change mints a new version rather than colliding.

### Validation and refusals (before any write)

A writer that would allocate a new instance IRI refuses, writes nothing, and
exits non-zero with a message naming the missing input when:

1. `mode` is missing or not one of the two values;
2. `mode = "base"` and `base` is missing, relative, has query/credentials/
   fragment content, does not end in `/`, or is a public base not using HTTPS;
3. `mode = "base"` and `base` equals or lies under a **former-name** base
   (`http://aegis.gastown.local/ontology/`, `urn:shuttle:`, `https://camayoc.local/ontology/`).
   Former names are read aliases, never new-allocation bases;
4. `mode = "base"` and `base` overlaps the rt9f.5 `aegis-infra` base or any
   other registered namespace's base (the quipu registry rule);
5. `mode = "urn-uuid"` and `base` is set, or `install_uuid` is missing or not a
   valid UUID (deterministic classes cannot converge without it);
6. the writer's bearer for the guarded write path is unset or empty. The writer
   never guesses identity headers or falls back to an unauthenticated route;
7. the store's recorded install mode (below) disagrees with the configured
   mode.

Reference reuse and reads never require these inputs: a reader with no
configuration still recalls every stored record.

### Recorded install declaration

On first allocation the install records one declaration (mode, base or
install UUID, timestamp, principal) in the protected namespace registry graph
`urn:quipu:graph:namespaces` through the existing local, explicit
`quipu namespaces apply` path. Startup never writes it. Effective policy is
config ∪ persisted, and disagreement refuses (the registry's existing rule),
so dropping config cannot silently switch an install's identity mode.

Quipu gap to close in the implementation item: the registry currently admits
only HTTP(S) bases, while `namespace::require_base_ns` already admits `urn:`.
Option 2 needs a registry entry kind with no base prefix (`mode = "urn-uuid"`,
`install_uuid`, `types` allowlist) whose owner scope is enforced from facts,
not from IRI prefix.

## 3. Owner key and scoped allocation

### 3.1 Scopes

One owner scope per writer. A scope is the pair *(owner principal, allowed
types)*; a principal allocates only inside its own scope.

| Scope | Owner principal | Allocates |
| --- | --- | --- |
| `chaski` | chaski reaction runtime | firing, reaction, focus |
| `shuttle` | shuttle exporter | run, workflow, event |
| `shantytown` | shantytown gate writer | gate verdict, gate approval, GitHub entity mirror |
| `camayoc` | `cy` record writer | memory record, topic |

The scope list is the proposed owner-key policy **for decision**; it is not
enacted by this document. There is no general-operator mint: an operator who
needs a record in a scope runs that scope's writer. Verifier registrations
stay human-loaded and keep their current identity (frozen bytes).

### 3.2 Enforcement

- **Option 1:** the scope is a `[[quipu.namespaces]]` entry per writer, base
  `{base}{scope}/`, with a `types` allowlist and opaque or key minting. The
  guarded write path already refuses a mint outside the declared types and
  graph. New: a write whose subject lies under a scope base is admitted only
  for that scope's owner principal.
- **Option 2:** the IRI carries no scope, so each allocated record carries an
  allocation fact naming its scope and owner. The predicate is taken from the
  loaded w3id vocabulary (m5s.1 ownership ledger); **this design does not mint
  one**. If no suitable term exists, the implementation item files a schema
  change proposal through `quipu_propose_schema_change` first.

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
| legacy `aegis.gastown.local` → option 2 | same, on an option-2 install | same rule; UUID is v5 of the legacy IRI string under `install_uuid` so the alias is reproducible |
| `urn:shuttle:` → option 1 or 2 | shuttle runs/workflows/events | same rule; signed registrations are never aliased in place |
| option 1 → option 2, option 2 → option 1 | an install that later changes mode | a migration, not a config edit: new declaration, per-class alias facts, both readable; startup never performs it |
| legacy vocabulary → w3id vocabulary | terms, not instances | owned by m5s.1 (`owl:equivalentClass`/`equivalentProperty` and the published mapping); **never** `owl:sameAs` between instances and terms |

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
  declaration. Changing it is an explicit migration (section 4), so a rollback
  cannot silently fork identity.

## 8. Acceptance for implementation items (per repo)

1. Before → after counts of `aegis.gastown.local` / `scbrown.github.io/quechua`
   occurrences by class (section 1), measured with `git grep` on non-test,
   non-doc source.
2. Fresh disposable install, option 1: one real write per writer mints under
   `{base}{scope}/`, zero legacy triples written; read-back per 3.5 passes.
3. Fresh disposable install, option 2: the same writes mint `urn:uuid:` with
   scope facts; a repeated deterministic write converges; a repeated random
   write is distinct.
4. Negative controls: unset mode refuses with no write; former-name base
   refuses; base overlapping `aegis-infra` refuses; mode disagreeing with the
   recorded declaration refuses; a principal writing into another scope
   refuses.
5. Legacy control: a store holding legacy-IRI records still recalls them after
   the change; a signed legacy payload still verifies.
6. rt9f.5 control: its six-type A/B gates and keyed shapes are unchanged.

## 9. ASK Stiwi (one card, routed via the lead)

**Decision needed:** the option-1 base for non-infra instance records in this
deployment. The `gastown.local` replacement has not been chosen, and this
design does not pick it.

- **Option A (recommended):** reuse the accepted infra authority
  `https://w3id.org/intigraph/aegis/` with a path segment **sibling** to
  `id/` (the segment name is part of this decision; `id/` itself cannot host
  the scopes because `aegis-infra` owns that base and the registry refuses
  overlap), then per-writer scopes `…/<segment>/chaski/`, `…/shuttle/`,
  `…/shantytown/`, `…/camayoc/`, each its own namespace entry. *For:* one public naming decision
  already accepted for infra; public HTTPS; no second authority. *Against:*
  the w3id route/resolver still needs its separate target proof and upstream
  public-send step before it resolves; memory records under a public authority
  need audience controls (`org:`/`personal:`) from the registry.
- **Option B:** a private, owner-controlled resolvable base on a domain Stiwi
  names. *For:* private records never sit under a public authority. *Against:*
  needs a new name; still must not be a hard-coded default in tool code.
- **Option C (only if Stiwi rules it):** adopt unset → option 2 as the
  documented fallback for other installs, with a recorded `install_uuid`. This
  does not change this deployment's option-1 ruling.

Until the card is answered, no writer changes its base and existing instance
IRIs remain.
