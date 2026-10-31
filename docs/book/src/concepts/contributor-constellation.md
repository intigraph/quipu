# Explore the contributor constellation

Open [Explore](../explore/.) to follow the ideas behind Quipu into the code.

The loading panel follows engine startup, pendant download, and graph indexing.
Download percentages use the response size when available; otherwise the panel
shows transferred bytes. Once the graph is ready, the workbench appears. If
loading fails, **Try again** repeats the selected load without leaving the page.
The repository pack includes the vision, source-backed episodes, design decisions,
a trust directive, and the book's chapter order alongside indexed code and docs.
The vision remains an aspiration; each decision links to its own evidence and
implementation. These narratives are extracted from repository documents, not
imported from an operational knowledge graph.

## A phone walkthrough

At a 390px-wide viewport:

1. In **Knowledge constellation**, search for **vision**, then select **Quipu vision**.
2. In its card, tap **guides: Federation has explicit boundaries**.
3. Read the decision and its source excerpt. Tap **governs: src/provider/mod.rs**
   to reach the actual indexed code module.
4. Follow another related entity. Each click centers that entity in the map.
   The card shows its types, with **Properties** and **Provenance** disclosures.
   **Read the source** opens the repository file; **Inspect facts & edit** opens
   its local facts. SPARQL, export and the propose-as-PR flow remain available.

Related entities come from queries against the whole loaded graph. Arrows mark
outgoing (→) and incoming (←) links, and every link names its predicate. The list
includes entities outside the initial contributor overview, such as types and
provenance sources. An engine-truncated answer is explicitly marked incomplete.
The URL's `entity` parameter identifies the selection: browser back and forward,
reloading, or opening that URL in another tab restores it in the map.

![The contributor constellation on a 390px viewport](../images/contributor-constellation-mobile.png)

The main cord joins five pendant cords: vision/book, decisions/rules, episodes,
code, and reading trail. These are deterministic topic/type groups, not inferred
Louvain communities. Entity IRIs sort the knots along each pendant; following a link can add
entities beyond the initial overview. Drag or use arrow keys to pan; pinch,
use the mouse wheel or the zoom buttons to zoom. **All clusters** fits the
reading map; tap a cluster to enter it. At that overview scale, cluster controls
replace individual node controls. Overview cords show up to eight actual knots
in IRI order at reduced scale. Visible node targets and controls are at least
44 CSS pixels, independent of zoom. The node card and search results provide
text navigation without hovering or precise graph taps.

## Knot size and relation details

Knot radius is `5 + min(13, 3 × sqrt(degree))` CSS pixels. Degree counts distinct
incoming and outgoing `(subject, predicate, object)` links in the currently loaded
map, including newly explored neighbors. It is local view importance, not global
PageRank. Selection changes highlighting, never importance size. The on-screen
legend states the metric. Geometry is static; no force simulation or animation
is needed, and reduced-motion preferences disable transitions.

Click a relation line, focus it and press Enter/Space, or use **Relation details**
in the entity card. The panel names the predicate, subject, and object as full
IRIs, with buttons to inspect each endpoint or the predicate itself. Subject
provenance properties are labeled as subject-level evidence, not edge provenance.

**Fact history, valid time & transaction time** shows the receiving store's
assertions/retractions, valid intervals, transaction IDs/timestamps, sources and
actors. Relation history filters by the exact predicate and IRI object. Entity
history covers its outgoing facts in ROOT, matching this map's dataset. History
is capped at 2,000 facts and explicitly marked incomplete at the cap. It is not
an all-graph or producer audit view. A static RDF share does not carry the original
producer transaction log: receiver import time must never be presented as original
author time. Episode/source properties retain whatever the pack actually states.
An older browser bundle without `factHistory` displays an unavailable message;
it never fabricates history from current SPARQL rows. The query and edit paths
still wait for the normal native verification ceremony, including saved previews.

## Ask the same question with SPARQL

The canned **How does the vision guide code?** query follows two explicit edges
and checks the indexed module's path. It does not infer that a similarly named
file implements a decision.

```sparql
PREFIX q: <https://quipu.dev/knowledge/>
SELECT ?decision ?module WHERE {
  q:vision q:guides ?decision .
  ?decision q:governs ?module .
  ?module a ?type .
}
```

## Keep the released knowledge current

The release producer runs `scripts/build-contributor-knowledge.mjs` after Bobbin
indexes the repository. Its curated registry is
`docs/knowledge/contributor-stories.json`. Each story names a source passage and
code witnesses; generation refuses a missing passage, missing file, or private
identifier in the curated text. The book order comes from `SUMMARY.md`.

The producer includes this projection in its ordinary CONSTRUCT share scope,
then adopts shapes, imports and promotes into a fresh receiver. The contributor
proof requires all six knowledge classes, four source-backed episodes, and
vision → decision → typed code paths. A code-only pack cannot pass that proof.
The release workflow ships that same output as its repository pendant asset.

Use `just contributor generate` to inspect the deterministic projection.
`QUIPU_BIN=/path/to/quipu just contributor pack /tmp/repository-share` runs the
full producer and receiver proof. The output directory must not already exist.
The page consumes the released artifact when the documentation build stages it;
a source change alone does not change the pack already on the site.

The browser's delta producer uses the same 128 MiB payload budget as its full
export, so adding contributor knowledge does not disable proposing an edit.
The default remote delta budget remains 8 MiB. During release publication, docs
allow missing, not-yet-staged assets for 30 minutes. Once staged, every asset
must be published byte-for-byte; beyond the grace a missing asset fails.
