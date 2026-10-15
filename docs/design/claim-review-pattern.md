# Claim-review pattern (future idea, from ClaimsKG)

> Created: 2026-10-15
> Status: IDEA, not scheduled. Revisit only if the comparison below finds a gap.
> Source: BeyondFacts 2027 CFP on the W3C semantic-web list (GESIS / LIRMM group).
> Related: [statement-identity.md](./statement-identity.md),
> [episode-retraction.md](./episode-retraction.md),
> [cross-graph-alignment.md](./cross-graph-alignment.md) (three-state `verify`),
> [governance](../book/src/concepts/governance.md)

## One-line

ClaimsKG keeps a claim, each review of it, and each review's verdict as
separate nodes; check whether quipu can hold several competing judgments
about one assertion the same way.

## What ClaimsKG is

A public knowledge graph of fact-checked claims built by GESIS: about 74k
claims and 72k reviews from 13 fact-checking sites (Politifact, Snopes, Full
Fact, AFP and others), 1996 to 2023. Last release January 2023.

- Home: https://data.gesis.org/claimskg/
- SPARQL: https://data.gesis.org/claimskg/sparql
- Dataset DOI: https://doi.org/10.7802/2620
- Paper: Tchechmedjiev et al., "ClaimsKG: A Knowledge Graph of Fact-Checked
  Claims", ISWC 2019.

## The pattern

Built on schema.org `ClaimReview`:

| Node | Carries |
|---|---|
| Claim | text, author, date, mentioned entities (DBpedia-linked) |
| Review | reviewer, date, headline, URL; points at one Claim |
| Rating | the reviewer's original label ("Pants on Fire") **and** a normalized value (true / false / mixture / other) |

One claim can have many reviews, and each review keeps its own verdict. Nothing
collapses them into a single truth value; a consumer picks the aggregation.

## Why it might matter for quipu

Agents assert things that contradict each other. The questions to answer:

1. Can quipu hold N independent judgments of the same statement (one per
   agent or verifier), each with its own provenance, without one overwriting
   the others?
2. Does a judgment keep both the judge's own label and a value on a shared
   scale, so judgments from different verifiers can be compared?
3. Is "which judgment wins" left to the reader (a stored query or policy)
   rather than fixed at write time?

If statement identity, provenance and the verdict/verify surface already
answer all three, close this note. If not, the gap is the design work.

## Next step

About an hour: map the three questions onto current quipu (statement
identity, episodes, retraction, `verify`), and record the answer here.
