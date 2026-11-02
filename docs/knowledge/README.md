# Repository pendant source

The repository-owned release job exports code and document **pointers**, plus
reviewed summaries of ideas, interfaces, decisions, freshness and starting points.
It does not export source chunk text or line spans. Source witnesses bind each
summary to its defining tracked file. The native index must resolve every
curated pointer before the build can proceed. Stored queries and validating
shapes travel with the share.

The generated CLI command index and MCP tool index, the Agent Primer page and
its embedded source (`src/primer.md`) are required tracked, nonempty sources.
In the current Python producer their source-bound summaries enter the curated
registry, so the native index
must resolve their real document pointers. After importing the finished pendant,
the builder requires those same labelled pointers and exact paths in the
receiver as well as README, through the typed `Document` reader.
Missing sources, absent indexed pointers or a dropped receiver pointer stop
publication. Both the current Python producer and the legacy shell builder use
these checks. The pendant carries pointers and reviewed summaries, not full
reference text; follow their revision-bound source links for the complete text.
Generate and commit the references with `just docs generate` before cutting the
release; `just docs generated-check` verifies the committed copies.
It also generates man pages and completions using the catalogue's per-flag help;
the CLI help and generated command index use those same descriptions.
When a reference or primer changes, review its registry summary and update that
record's `source_sha256` in `repository-knowledge.json`. A stale source witness
refuses the build rather than silently binding a summary to different bytes.

`alignments.ttl` is currently a valid empty SSSOM/RDF carrier. Its accompanying
status file explicitly records zero approved mappings and seals those bytes.
This is not evidence of global absence. Missing, changed or malformed input
refuses; an explicit empty carrier does not. Dependencies and glossary references
are not identity alignments. Code/commit deduplication and unrelated exactMatch
records are not cross-repository mapping approval.

Candidate mappings must go through Quipu's existing `align propose` and
`align decide` curation path. Keep their source graphs and source witnesses;
do not fill an approval author from a lexical score. Only the reviewed mapping
record may produce derived identity through the engine's existing apply path.
The current source profile deliberately does not import arbitrary nonempty
mapping data until an actual curated artifact and its transport are reviewed.

The copied release workflow verifies the three public assets only after the
ordinary release finishes. The managed trusted host runs this repository's own
publisher, keeps its publication policy private, pins both native engine
executables and the indexer, and refuses retained uncertain upload outcomes.
Default configuration still publishes only the existing Quipu corpus. Enabling
another repository requires its actual trusted host configuration and a clean
release containing this job; committing this source does not publish an asset.

The publisher requires the engine's model/digest/dimension manifest recipe.
Loading and vector rebuilding belong to the native engine. No hosted job gets
private publication policy or a new credential route. These source controls do
not establish public asset availability, live loading or service acceptance.
