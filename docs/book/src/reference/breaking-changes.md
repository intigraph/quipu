# Breaking changes and running-system corrections

<!-- Generated from releases/changes/*.json; use just breaking render. -->

Draft records are incomplete and cannot satisfy release readiness. Doctor produces read-only plans; a plan never grants migration authority.

## w3id writer vocabulary: upgrade readers before writers

**ID:** w3id-writer-vocabulary-transition — **Status:** draft

**Affected:** chaski (versions unknown); shuttle (versions unknown); shantytown (versions unknown); camayoc (versions unknown)

**Target platforms:** unknown (draft)

Readers matching only historical vocabulary terms can miss new firings or transitions. Instance and signature identities must remain unchanged.

**Source:** `https://github.com/intigraph/chaski/pull/71`

### Detect

For a known firing and event ID, collect one native identity binding and populated controls in every explicit dataset. Read both historical and canonical predicates. Missing scope, duplicate bindings, denied/partial/stale reads or unknown installed versions are UNKNOWN.

**Canonical predicates:** `https://w3id.org/intigraph/camayoc/core/ns#eventId`

**Historical predicates:** `http://aegis.gastown.local/ontology/eventId`

### Correct

Operation: managed-reader-first-writer-upgrade/v1. Separate admission required.

**Correction packet:** `not supplied (draft)` — SHA256: unknown.

1. Install alias-capable readers and required shapes through the managed lane first.
2. Prove old and canonical fixtures each resolve one identity; duplicates, unknown terms and incomplete reads refuse.
3. Deploy the exact new writer artifact while retaining configured instance bases and signing identities.
4. Verify one admitted native canary and old-record recall; then resume the original schedule and observe its natural run.

### Prerequisites

- Exact affected releases and installed writer/reader/shapes digests are still unknown; this draft cannot ship.
- Quiesce only the admitted writer; preserve outbox, signing identity, original instance base and receipts.
- Capture a pre-upgrade backup and prove a functional restore before changing the store.

### Validate

- Known input event identity matches one native binding.
- Legacy record recall and canonical writer vocabulary pass together.
- Instance IRI and signed transition bytes are unchanged.
- Exact affected/target releases, platform artifacts and admitted command packet are bound before readiness.

### Roll back

Restore the prior writer and configuration via the managed inverse.

Retain dual readers and stop the scoped writer. Fix forward, or use a separately admitted pre-upgrade backup restore; an old-only reader rollback is unsafe.

**Rollback packet:** `not supplied (draft)` — SHA256: unknown.

**Data-loss limit:** Backup restore can lose post-backup writes and requires a separate data-loss admission. No re-key, replay of uncertain effects or signature rewrite.

**Release readiness:** incomplete: review_ref, approval_ref, status=ready, targets, affected exact versions, reviewed correction packet, reviewed rollback packet

**Required release approver:** Stiwi.
