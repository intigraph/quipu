# Domain shapes in practice

Quipu enforces engine contracts: provenance, historical evaluation, admission,
content-bound verdicts, certification, trust roots and single-use nonces.
A domain producer defines what a host, a media record or a work item means.
Those are separate responsibilities even when the same validator checks both.

The [Camayoc domain example](https://github.com/scbrown/camayoc/tree/main/shapes/aegis)
stages three domain files:

- `homelab-infra.shapes.ttl`: hosts, networking, services and storage.
- `personal-media.shapes.ttl`: people, accounts, libraries and entertainment.
- `software-factory.shapes.ttl`: crew and software production.

Camayoc records the exact upstream source and each shape's destination. Existing
IRIs remain unchanged. A separate compatibility payload retains the original
bytes while the source transition is reviewed. The destination's union/parity
tests detect a lost constraint; class competency queries have populated positive
and missing-label negative fixtures. Neither test claims production acceptance.

## Vocabulary before custom classes

[Quechua's trim decisions](https://github.com/scbrown/quechua/blob/main/trim/classes.tsv)
already distinguish reusable governance terms, public replacements and
infrastructure-specific vocabulary. Keep those decisions: a domain-specific
class does not become a new public Quechua term merely because it moved files.
Public replacements are explicit profile changes, not namespace renames or
automatic rewrites of existing data.

## Work uses the Seeds profile

[Seeds owns its work-item profile](https://github.com/scbrown/seeds/blob/main/shapes/seeds.shapes.ttl).
Camayoc references its pinned source rather than maintaining another WorkItem
gate. New records use `schema:Action`; historical WorkItem/Bead names remain
deprecated compatibility vocabulary. The existing replacement/subclass chain
does not itself rewrite stored types.

An iCalendar `Vtodo` remains a separate node linked through `skos:exactMatch`,
as Seeds' profile specifies. Do not replace that relation with `owl:sameAs` or
claim that the records share an IRI.

## A transition without a validation gap

1. Review and publish the producer's destination packs and competency queries.
2. Propose exact released source pins through the deployment repository.
3. Load destination bytes before removing original source ownership, preserving
   the existing loaded-set naming/count contract and classification guard.
4. Revalidate existing data with no new violations and prove the intended alias
   queries, using the deployment's bounded, authorized reader.
5. Only then remove compatibility source copies. Retain prior pins and the prior
   engine artifact for the normal scoped rollback.

This chapter describes the source transition. It grants no load, unload, private
data migration or deployment authority. The original Quipu copies remain until
the destination gates have been measured.

## Warning-first propagation

The source CI compares immutable RDF snapshots, including anonymous constraints,
against a revision-scoped term/CQ catalogue. It retains a JSON report and warns
about unresolved entries. Read and parse failures remain failures. Gaps become
blocking only after the October backfill and a separate enforcement decision.
An event receiver must deduplicate by exact merge identity and retain uncertain
writes for native readback; source CI alone does not create a task or change a pin.

## Warning check in the existing CI target

The existing Pre-commit checks job runs the shape-propagation hook on every
invocation. Its Python environment pins rdflib 7.6.0, runs the RDF and CI
counterexamples, resolves the exact event base and checked-out head, and reads
Quechua/Camayoc inputs at fixed commits. Shallow CI checkouts explicitly fetch
source history. Execution and input-read errors fail; evidence gaps remain WARN.
The complete delimited JSON report is retained in the job log, including source
commits and input hashes. This replaces the earlier proposed separate workflow
steps and report artifact; the workflow files themselves are unchanged.

For an offline local check, set QUIPU_PROPAGATION_QUECHUA_REPO and
QUIPU_PROPAGATION_CAMAYOC_REPO to existing source repositories. That mode reads
the pinned Git objects rather than working files, never fetches, and refuses a
missing object without falling back to network. With neither set, public source
reads have byte and timeout limits. No service credential or shape write is used.
