# arXiv submission card

**Submission is Stiwi's action.** This file is the prepared metadata so the
walk-through is a form-fill, not a research task. Nothing here has been sent
anywhere.

Prepared by arnold, 2026-08-30, against `main.tex` at this commit. If the source
changes, rebuild and re-read the abstract below — it is a transcription of the
PDF, not a second draft of it.

Re-transcribed by kelly, 2026-08-30, after the pre-submission review changed the
abstract's third paragraph (it reported 105 and 93 for the same quantity four
lines apart). The abstract below is a fresh extraction from the rebuilt PDF and
was diffed against it.

Re-transcribed by ian, 2026-10-22: the rebuilt PDF abstract is 1,549
characters including spaces (typographic ligatures normalized for the form).
The plain-text abstract below matches the current paper source and rebuilt
PDF. The schema-evolution study retains its datatype detector miss caught by
post-merge validation. The accepted public-history ruling remains in force.

---

## 1. Public-history decision — resolved 2026-09-24

`8b369b2:benchmark/replay/corpus/corpus.json` is on the public GitHub remote and
is **still fully reversible** — the labels were `sha256(salt + iri)[:10]` with
the salt committed in the public build script, so any candidate IRI can be
confirmed by recomputing the digest. Re-measured after the fix: 24 of 27 probe
names recovered from that blob. Host names, service names and the crew roster.

The current tree is clean (resealed from a CSPRNG, map discarded, gate PASS).
**A later scrub commit does not fix history**: a push publishes every object, and
the blob stays reachable by sha.

The repository owner decided on 2026-09-24: **do not rewrite this history**.
Exposure of internal hostnames and crew or agent names is accepted and is not
a publication blocker. Secrets, credentials and personal data still require
scrubbing. This decision resolves the history question; it does not authorize
an agent to submit the paper or choose the publication licence.

Rechecked on 2026-09-25: GitHub still resolves commit `8b369b2`; the accepted
historical exposure has not been mistaken for a removed object. The current
artifact passes the full scrub gate at repository revision `f15b28d`.

---

## 2. Form fields

**Title**

    Shape-Aware Three-Way Merge for RDF Knowledge Graphs:
    The schema defines the conflict — an implementation and evaluation

**Authors**

    Steve Brown (ORCID 0009-0009-1720-1785)

The `\thanks` block in `main.tex` carries the acknowledgement of implementation
and drafting assistance from Claude (Anthropic) and Codex (OpenAI), and the
artifact-availability statement. The additional research assistance is recorded
in the October revision.

**Primary category** — recommended: `cs.DB` (databases).
The paper's contribution is a merge operator and its evaluation in a store.

**Cross-lists** — recommended: `cs.SE` (the structured-merge lineage the paper
places itself in) and `cs.AI` (multi-agent knowledge graphs, which is where the
replay corpus comes from). Add `cs.DC` only if a reviewer asks; the paper is not
about distribution.

**Comments field** — suggested:

    15 pages. Artifact, benchmark and replay corpus at
    https://github.com/scbrown/quipu ; every table names the command that
    regenerates it.

**License** — recommend `CC BY 4.0`. The artifact is already public; a
non-commercial or no-derivatives license would sit oddly beside it. This is a
one-way door on arXiv: the license cannot be loosened after announcement.

**Abstract** (plain text for the web form — no LaTeX, transcribed from the built
PDF):

Merging RDF serializations creates ordering conflicts and misses semantic ones. We describe and evaluate a three-way merge in which the schema defines a conflict: set algebra combines canonical triples, and a slot is contended only where a SHACL shape permits at most one value. Multi-valued predicates union; divergent functional values go to a person. The same shapes graph validates the result. We do not claim the first three-way merge for RDF. Our contribution is an implementation in a production multi-agent store and an evaluation against Quit Store’s Context Merge, which it extends rather than replaces, and six other strategies. Across five synthetic seeds, the shape-aware operator raised 33 conflicts, all true positives, with no triples lost or fabricated and no SHACL violations admitted. The context-overlap baseline detected the same 33 conflicts but asked 845 questions. A recorded production replay covers 105 human alias repairs (93 after excluding 12 chained pairs) and 939 duplicate-value incidents. It reproduces a triple-level limit: different names for one entity remain invisible (0 of 93 alias repairs detected, matching 0 of 21 synthetic cases), alongside the benefit on real duplicate values. A further synthetic study tests evolving shape contracts. Merging shape declarations first handles the tested cardinality changes, while datatype evolution exposes a detector blind spot caught by post-merge validation. These results qualify schema-first orchestration rather than establish a universal conflict-resolution rule.

---

## 3. What to upload

arXiv prefers LaTeX source over a PDF. Upload the source; it builds there.

    main.tex  references.bib  sections/*.tex

`main.bbl` is **not** committed. arXiv runs BibTeX, so source-only is fine — but
if the build fails on their side, generate `main.bbl` locally and add it, which
is the standard remedy. Do not upload `main.pdf` alongside the source; arXiv
takes one or the other.

---

## 4. Verification state at hand-off

The current source rebuilds with `just paper-merge`. The plain-text abstract
matches the rebuilt PDF, with no unresolved references or citations. The full
`scripts/arxiv-scrub-gate.sh` checks its declared artifact components and proves
the identifier and corpus-reversibility controls.

| check | result |
|---|---|
| `arxiv-scrub-gate.sh` | **PASS**, exit 0 — whole declared artifact and every positive control |
| bibliography | All entries cited; 0 unresolved citations |
| BibTeX warnings | **0** |
| build | tectonic, 15 pages; abstract checked against source/PDF |
| Arm E | Real library/schema fixtures; counts and tables regenerate |
| bead ids in the artifact | **0** — the one advisory (`examples/replay/main.rs`) is gone |
| policy projection | the gate was run against the config that the pre-push guard's own selftest **passes**; see the note below |

**Note on the policy projection, because it is the one thing a reader should not
take on trust.** Regenerating the pattern config on 2026-08-30 produced a config
that **breaks its consumers**: the private-IPv4 arm contained `\d`, which
`grep -E` reads as a literal `d`, so it matched nothing. The gate caught it and
refused to report (exit 2, "CONTROL FAILED"); the pre-push guard's selftest
caught it independently. The working config was restored and re-verified by
selftest — failing under the bad config, passing under the good one, both
observed — before the gate result above was taken.

The cause was **not** a defect in the generator. It was that the generator was
run from a checkout 426 commits behind its origin; the current one translates the
construct and refuses to emit anything `grep -E` would misread. The installed
config was then proven byte-identical to a fresh generation from current source,
so the projection behind the verdict above is current, not merely working.

None of this touches the artifact. It is recorded because "the gate passed" is
worth exactly as much as the instrument behind it, and for four minutes that
instrument was provably dead — which is the situation this gate's controls exist
to make visible rather than survivable.

---

## 5. Rebuilding

    just paper-merge          # tries tectonic, latexmk, then pdflatex

`tectonic` is installed at `~/.local/bin/tectonic` as of 2026-08-30. Before that
no TeX engine existed on any fleet host and each rebuild fetched one into a
session scratchpad, which is why two builds of this paper report two different
tectonic versions.
