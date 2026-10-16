---
name: quipu-embedded-artifacts
description: >-
  Build a graph-backed status brief with the real Quipu WebAssembly explorer
  inside a hosted artifact. Use for embedded pendant drilldown, generated
  priority and decision rows, preserving an existing artifact URL and database,
  or refreshing its graph snapshot. Includes the proposed page-bound signed
  action pattern; signing requires its separate enrollment and verification work.
---

# Quipu embedded artifacts

Ship the engine, a generated pendant, and its small presentation together.
Keep live status rows separate from the dated graph snapshot, and make that
distinction visible when a reader opens a record.

## Establish the host contract first

Read the existing page, URL, database schema and publication authority. Keep
the current collections, document IDs and access rules. Use the host's
Artifact tools to read and publish when available; otherwise provide a ready
folder to its existing publisher. A different hosting product is not the same
artifact.

Publish a scratch probe before replacing the real page. Show independent
results for JavaScript, minimal Wasm instantiation, the released Quipu engine,
native pendant import and promotion, a positive drilldown query, and a module
worker. Record actual hosted errors, URL and engine identity. A local browser
pass does not establish the hosted CSP, asset types, MIME types or origin.

One observed Artifact host supports Wasm and module workers but does not serve
archives. On that host, ship a native pendant archive **inside base64 JSON**;
decode it before passing bytes to `Explorer.loadPendant`. This changes the
transport only: native verification still checks the manifest and payloads.
The publisher supplies the document wrapper, so page content uses title,
styles and markup without its own doctype, head or body tags. Recheck these
constraints for each host rather than assuming they are portable guarantees.

## Stage the actual explorer

Use the repository's existing `just explorer release` staging recipe, or its
shared release build. Verify the release archive against its checksum sidecar
and record the exact staged JS and Wasm hashes. Never use the performance
harness as the explorer, or fetch release assets from a third-party origin at
page runtime.

For the full workbench, retain its relative worker, runtime and model paths.
For a small read-only brief, the supplied [assets](assets/) use the released
engine directly with a small worker and record inspector. They support native
verified import, node drilldown and read-only SPARQL, without semantic-model
downloads, browser edits or a service worker. They are a presentation over the
engine, not another RDF interpreter.

## Generate the graph and rows

Query the graph for existing entities and provenance before selecting data.
Use the authoritative tracker graph and its actual item IRIs; do not guess an
ontology node from an item ID. Treat a failed query as UNKNOWN.

The included generator supports a bounded Seeds brief. Its vocabulary IRIs
are the actual tracker vocabulary; changing deployment hostnames does not
change that vocabulary. Read [the input contract](references/inputs.md).

```bash
python3 skills/quipu-embedded-artifacts/scripts/generate.py \
  --config brief-config.json --out ready --work producer-work \
  --bundle docs/book/src/explore/pkg --quipu /path/to/quipu
```

Both output directories must be new. The generator reads at most 100 selected
IDs, 500 links, and 1,000 linked-node facts, with sentinel limits and a 2 MB
response cap. It fails on missing or ambiguous selected records rather than
shipping a silently shorter brief. Rank, visible-ask selection, concise titles,
next steps, presentation state and ask wording remain curator-owned. All other admitted Desk cards remain in the pendant,
accessible through an **All Desk cards** query.

It loads the projection and current outward identifier catalogue into an owned
local store, registers four stored queries using native MCP stdio, and calls
native `quipu share`. No listener or live-store write is needed. Optional
descriptions containing blocking private identifiers are omitted with visible
notices. An unsafe ask uses its source title with an explicit withholding
notice. Identities are never rewritten; unsafe required fields still meet the
unchanged outward refusal. Keep raw query responses and producer logs outside
the published folder and public repository.

The resulting folder contains engine assets, `pendant.json`, `snapshot.json`,
`rows.json` and the presentation modules. Native manifest, RDF, shapes and
stored queries remain in the work directory and inside the pendant envelope.
Do not hand-edit a manifest or checksum. Regenerate instead.

## Integrate with the existing page

Add `explorer.css` and a container, then attach the read-only presentation:

```html
<link rel="stylesheet" href="./explorer.css">
<section id="graph" aria-label="Explore the brief"></section>
```

```javascript
import { attachExplorer } from './explorer.js';
const explorer = await attachExplorer(document.getElementById('graph'));
// Connect each existing row's button to its source item ID.
button.onclick = () => explorer.openItem(row.item, row.title);
// An adjacent link can open all admitted Desk cards.
deskLink.textContent = `All Desk cards (${explorer.deskCount})`;
deskLink.onclick = () => explorer.openDesk();
```

Preserve the original database read subscriptions and their ordering. The
page does not apply `rows.json`: the existing authorized owner applies those
operations through ArtifactData. Only version-checked `update` operations are
generated: priority, owner, item and approved rank for priorities; item identity
only for asks. Never replace a curated row with a raw tracker title or state.
Use actual current document IDs and versions; an absent binding refuses rather
than creating a new document or guessing a version. Today and the
status log remain owner-written prose. Never infer a new human approval,
Desk admission or enrollment from a displayed row.

## Verify, publish and refresh

Test the actual generated page at a subpath in a real browser. Verify row
counts and ordering, exact priority-to-node and ask-to-node drilldown, the
full Desk query, unchanged database subscriptions, visible omissions, keyboard
use and a narrow viewport. Refuse a damaged pendant, a write query and a live
row missing from the snapshot. Render source text with `textContent`.

Publish to the **same URL** through its existing publisher and preserve its
capabilities and access rules. Record the served bytes and repeat the actual
hosted row drilldown. Local mocked database reads establish subscription
compatibility only; they do not prove the hosted database or publication.

Refresh by rerunning the normal admitted Desk export, updating the curator's
rank/ask inputs, supplying the current owner prose snapshot, regenerating into
new directories, and comparing the new rows and share identity. Verify before
the owner applies rows and republishes. Keep the prior folder and row snapshot
as the concrete rollback. A newer live row absent from the pendant must show a
refresh message rather than unrelated or empty details.

## Page-bound signed actions

This is a separate proposed extension, not a capability of the read-only
assets. See [the general design](../../docs/design/page-bound-signed-actions.md).
It requires approved device enrollment, a server-verifiable action envelope,
nonce consumption, revocation, and an auditable native ingestion path. A secure
context or successful local signature alone does not establish those outcomes.
