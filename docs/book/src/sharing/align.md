# Aligning concepts across graphs

Sharing moves a graph. It does not move an opinion about what the graph's concepts **are**.

Import a colleague's graph and you may now hold two nodes for one thing — their
`bobbin-release` and your `Bobbin_release-artifact`. Nothing in the import can decide they are
the same, because nothing in the import knows. `quipu align` is the step that closes that gap:
it **proposes** candidate pairs with evidence, the operator **decides**, and accepted pairs are
**recorded** as `owl:sameAs` in a dedicated alignment graph.

Three properties hold throughout, and they are the reason the verb is split in three:

- **Nothing is applied on a score.** A candidate is a proposal until a human accepts it.
- **The record lives outside both source graphs**, so an imported graph stays byte-recoverable
  against its own share hash.
- **Rejections are remembered**, so the same pair is not proposed at you again.

## propose

```text
quipu align propose <graph-a> <graph-b> [--out <set.tsv>] [--db <path>]
```

Enumerates both graphs and emits candidate pairs as a **SSSOM mapping set** — a TSV with a YAML
metadata header, the standard interchange format for ontology mappings, so the artifact is
diffable, shareable and readable by tools that have never heard of quipu.

Over REST the same call returns the set inline:

```console
$ curl -s http://quipu.example/align/propose -H 'content-type: application/json' \
    -d '{"graph_a":"…/plane/crew/records","graph_b":"urn:shuttle:graph:identity"}'
{
  "candidates": 0,
  "set_aside": 0,
  "summary": "0 candidate(s); 0 entity(ies) set aside as ambiguous",
  "expected_version": "sha256:ce548369e02ee10e…",
  "set_tsv": "#curie_map:\n…"
}
```

**The summary reports both numbers on purpose.** `set_aside` counts entities excluded because
they carry more than one label — alignment never guesses which label is the one to match on. A
caller that prints only the candidate count hides a graph it could not read.

### Reading a zero

`0 candidates` has more than one cause, and they need different actions:

| cause | what you see | what to do |
|---|---|---|
| the graph IRI is not in this store | an **error** naming the IRI | fix the IRI — a namespace prefix is the usual culprit |
| entities exist but carry no `rdfs:label` | `0 concepts`, non-zero `unlabelled` | alignment matches on labels; ask the publisher to share labels |
| both graphs enumerate fine, nothing matches | `0 candidates`, `unlabelled` 0 | a real zero: there is nothing to align |

Candidates need more than a shared label. Two entities with identical `rdfs:label` and no
`rdf:type` in common produced **0 candidates** on a live run; adding a shared type to both
produced **1**. If a pair you expect is missing, check the types before the labels.

An absent graph is a question the store cannot answer, so it is refused rather than reported as
an empty result. An **empty** graph is a legitimate answer of zero and is returned as one.

## decide

```text
quipu align decide <set.tsv> --decisions <rows.tsv> --reviewer <who> [--out <set.tsv>]
```

Applies the operator's accept/reject rows to the proposed set and stamps who reviewed it.
Rejections are written as SSSOM **negative** mappings (`predicate_modifier=Not`) rather than
dropped, which is what makes them survive the next import.

`decide` prints the set's **version**:

```console
$ quipu align decide set.tsv --decisions rows.tsv --reviewer you --out decided.tsv
expected-version: sha256:99d5c39c476e4a2d…   <- pass this to `align apply`
wrote decided.tsv
```

⚠️ **`propose` prints a version too, and it is not the one to use.** Deciding changes the set,
so the two differ — measured on one run: `sha256:27eb4ae7…` from propose,
`sha256:99d5c39c…` from decide. Carrying propose's version to `apply` fails the concurrency
check, which is the correct outcome and an annoying way to learn it.

## apply

```text
quipu align apply <set.tsv> --graph-a <iri> --graph-b <iri> \
    --expected-version <sha> [--actor <who>] [--db <path>]
```

Writes the accepted pairs as `owl:sameAs` through the existing `knot` primitive, into an
alignment graph **derived** from the two source IRIs — not into either source.

Two contracts worth knowing before you script it:

**`--expected-version` is required and is never recomputed.** It is the version you read before
you started deciding. If the set changed underneath you, `apply` refuses and writes nothing.
Recomputing it here would hash the set being written, always match, and silently void the check —
so a lost decision would look like a success.

**The derived alignment graph is created for you; a graph you name is not.** The derived IRI is
computed by quipu and never handed back, so requiring you to pre-create it would be asking for a
name you are not told. Any other graph IRI you pass must already exist — otherwise a typo would
mint a new empty graph and report a successful write that nobody can find.

## export

```text
quipu align export <set.tsv> --license <iri> [--format tsv|ttl] \
    [--prefix <name>=<iri>]... [--author-prefix <name>] [--out <file>]
```

Writes a proposed or decided set as **standard SSSOM** — SSSOM/TSV with its YAML metadata
block, or SSSOM/RDF as Turtle — for tools that have never heard of quipu. It is read-only and
leaves the working set untouched, so the `expected-version` you carry to `apply` does not move.

The working TSV is quipu's own file: it holds full IRIs and no licence, because its exact bytes
are the version `apply` checks. Export adds what the standard requires and the working file
cannot carry, and **refuses rather than guesses** wherever that takes a judgement only you can
make:

| export needs | where it comes from | if it is missing |
|---|---|---|
| CURIEs for every identifier | the set's `curie_map` plus each `--prefix` | refused, naming the IRI no prefix covers |
| a `license` IRI | `--license` | refused — quipu does not pick a licence |
| an `author_id` entity | the reviewer, under `--author-prefix` | refused for a bare name like `malcolm` |
| a `mapping_date` | stamped by `decide` on every row it rules on | omitted: older sets carry none |

Nothing is upgraded on the way out. A declined row keeps no `author_id`, so no SSSOM reader
mistakes it for curation; an undecided row keeps the matcher's justification and its
confidence. Declined rows travel through the declared extension slots `quipu_review` and
`quipu_reviewed_by`.

```console
$ quipu align export decided.tsv --license https://creativecommons.org/publicdomain/zero/1.0/ \
    --prefix A=https://a.example/ --prefix B=https://b.example/ \
    --prefix crew=https://crew.example/agent/ --author-prefix crew --out set.sssom.tsv
```

`--format ttl` writes the same set as a `sssom:MappingSet` whose mappings are `owl:Axiom`
nodes (`owl:annotatedSource`, `owl:annotatedProperty`, `owl:annotatedTarget`), with negations
as `sssom:predicate_modifier sssom:NegatedPredicate`, authors as `pav:authoredBy` and dates as
`dcterms:created` (`xsd:date`). The Turtle is parsed back before it is written.

## What the record is

An alignment is an ordinary, queryable fact:

```sparql
SELECT ?a ?b WHERE { ?a owl:sameAs ?b }
```

Visible, retractable, and attributable — rather than a silent edit to someone else's
identifiers. Because it is a graph like any other, it is itself shareable: publish the alignment
and a colleague can apply your judgements to their own copy, or disagree with them in the open.
