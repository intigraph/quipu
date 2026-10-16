# Generator input contract

The Python generator uses only the standard library and an installed matching
`quipu` / `quipu-server` pair. The server companion handles transient local MCP
stdio; it is not a new network service. Keep endpoint authentication in the
environment, never in generated files. Remote calls carry `X-Quipu-Client` and
the configured task ID.

Paths in the configuration are resolved against the command's working
directory. Example configuration:

```json
{
  "server": "https://graph.example.org",
  "graph": "urn:project:example",
  "task": "example-task",
  "ranks_file": "rank-override.json",
  "asks_file": "asks-override.json",
  "desk_file": "desk-export.json",
  "row_bindings_file": "row-bindings.json",
  "snapshot_dir": "snapshot",
  "extra_asks": [{"item": "example-other", "where": "board"}]
}
```

`rank-override.json` contains only curator rank and exact item ID:

```json
{"ranks": [{"rank": 1, "item": "example-first"}]}
```

`asks-override.json` determines the visible asks, their order, and their current
document IDs. These IDs must refer to the existing collection:

```json
{
  "asks": [{"order": 1, "item": "example-first"}],
  "existing_doc_ids": ["first"]
}
```

`desk-export.json` is the existing admitted exporter output, not a manually
constructed substitute for cards that its capability gate refused:

```json
{
  "ops": [{
    "op": "set",
    "collection": "cards",
    "doc_id": "example-first",
    "data": {"bead": "example-first", "ask": "The admitted request"}
  }]
}
```

A curated item not in that admitted export can still link to its board record;
it is labelled `board`, not newly admitted to the Desk. The generator does not
create Desk cards, approvals, or human deferrals.

Supply the existing owner-maintained snapshot layout:

```text
snapshot/
  brief/today.json
  updates/<document-id>.json
  priorities/<document-id>.json
  asks/<document-id>.json
```

Today has `updated`, `headline`, and `points`. Updates have `at`, `headline`,
and `points`. Existing priority/ask documents include `item`; their filenames
retain document identity. Composite legacy item text can be corrected by the
explicit curated document-ID mapping without deleting its retained document.

Supply actual document identities and current versions from the owner's
ArtifactData read, separately from document data. A receipt may wrap this map
in a `bindings` field with its observation timestamp:

```json
{
  "priorities": {"example-first": {"doc_id": "first", "version": 3}},
  "asks": {"example-first": {"doc_id": "first", "version": 2}}
}
```

The generator emits `rows.json` in the private **work directory**, outside the
publish-ready folder, with version-checked `update` operations. It
never creates, replaces, deletes or writes a document. Priority patches contain
only `priority`, `owner`, `item` and curator rank. Ask patches contain only
`item`. Concise title, next step, presentation state, ask wording and status prose
remain curator-owned. A missing ID/version fails closed. On a version conflict,
read the document again and regenerate the proposed patch; do not drop the
precondition or replace the document. Review the operations and original rules
before the existing owner applies them. Keep the input snapshot for rollback.
Source graph identity is retained only in that work directory's
`provenance.json`; it is not needed by the browser and is not published.
Browser metadata is stored in the pendant's RDF at
`urn:brief:publication-metadata`, under `https://schema.org/description`.
The viewer queries that record after verified import. A diagnostic
`publication-metadata.json` copy stays in private work; neither it nor
owner operations belong in the published folder.

Four bounded queries read selected item fields, one-hop relations, linked
record summaries and the current privacy catalogue. This is a dated read
projection, not an atomic transaction snapshot or a complete board export.
No comment history is included. Keep any privacy omissions visible.
