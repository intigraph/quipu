# Embeddings and Semantic Search

Quipu answers two different kinds of question. **Lexical** retrieval (SPARQL,
`CONTAINS`, exact match) needs nothing but the fact log. **Semantic** retrieval
(`/context`, `quipu_hybrid_search`, `/search`) needs vectors, and vectors need
an embedding provider you configure yourself.

Automatic embedding is enabled by default when a provider is configured.
No model is downloaded automatically. Without model and tokenizer paths,
lexical graph operations still work and the loader reports that semantic
indexing is unavailable. Set `auto_embed = false` to disable automatic vector
writes. This does not disable model initialization, explicit semantic queries,
or existing import identity resolution, which may also use the provider.

## Writes and staged imports

Native CLI writes (`knot`, `ingest`, episodes) and server writes use the configured
provider. A share import indexes eligible, uniquely owned entities in its
**staging graph**. It does not adopt the graph into ROOT or turn on named-graph
semantic search. Explicit search scope and search policy still apply.

Import responses include `embedding.state`, candidate/indexed counts, the local
provider recipe when available, and an explanatory message. `ready` certifies
indexing for eligible staged entities; `partial` preserves entities already
owned by ROOT or another graph. `pending`, `unavailable`, `failed`, `disabled`,
`external`, and `quarantined` must not be described as searchable completion.
Vector-index inference uses bounded batches outside the server's global writer
lock. Existing import identity resolution remains a separate admission step;
this indexing status does not certify that step's lock latency.

A share may declare `manifest.embedding` with an ONNX basename,
`model_sha256`, `tokenizer_sha256` (lowercase SHA-256), and `dimension`. These
fields participate in the share identity. The native provider hashes its actual
files and validates the model output dimension. Import with an
available mismatched provider refuses **before staging**. Legacy shares without
this recipe remain importable; local indexing does not attest which model the
publisher intended. Re-importing with a real provider recomputes eligible
vectors instead of treating matching text as proof of model identity.

Existing graph data can still use the explicit startup `--embed-backfill` or
bounded `POST /embed_backfill_graph` operations when needed.

## Browser explorer boundary

The explorer uses its separate JavaScript ONNX adapter and self-hosted model
assets. It loads the runtime on the first semantic query and rebuilds its
in-memory vectors when the ROOT label/comment snapshot changes. It does not
copy staged named graphs into that index. Native import readiness does not
certify browser indexing or browser model-byte identity; the current browser
adapter does not validate `manifest.embedding` against its JavaScript model.
A loaded pendant alone must not be presented as browser semantic readiness.

## Configuring a provider

Two things are required, and having one without the other is the common
failure:

1. **The runtime.** Build with `--features onnx`. This supplies the ONNX
   runtime *only*. It does not supply a model, and a build with the feature on
   is not a build that can embed.
2. **A model on disk, plus the paths to it** in `.bobbin/config.toml`:

```toml
[quipu.embedding]
auto_embed = true
model_path = "models/all-MiniLM-L6-v2/onnx/model.onnx"
tokenizer_path = "models/all-MiniLM-L6-v2/tokenizer.json"
dimension = 384
```

The model files are fetched separately (for example from the
`sentence-transformers/all-MiniLM-L6-v2` repository on Hugging Face). Sandboxed
environments may not have network access to a model host, in which case the
files have to be provisioned into the image or volume ahead of time.

`dimension` must match the model. Both `model_path` and `tokenizer_path` must
be set. A partial or invalid configuration is an error; omitting both produces
a visible unavailable status. The same setup is used by native CLI loaders and
the server.

## Telling whether it worked

Three signals, in the order you will meet them.

**At startup**, a loaded provider announces itself:

```text
embedding provider ready (dim=384, auto_embed=true)
```

**`--embed-backfill` is fatal when it cannot run.** The flag is an explicit
request for a capability, so a server that cannot honour it exits non-zero
rather than starting up without it. The error names the configuration it
needs. Drop the flag if you deliberately want to serve without embeddings.

**Every retrieval response carries its own status.** `/context`,
`quipu_unified_search`, and `quipu_hybrid_search` all report:

```json
{
  "entities": [],
  "summary": {
    "total_entities": 0,
    "direct_hits": 0,
    "embeddings": { "configured": false, "embedded_entities": 0 }
  }
}
```

Read it as:

| `configured` | `embedded_entities` | Meaning |
|--------------|---------------------|---------|
| `false` | `0` | No provider. Configure `[quipu.embedding]`. |
| `true` | `0` | Provider attached, store never embedded. Run a backfill. |
| `true` | `> 0` | Semantic retrieval is live — an empty result really is "no match". |

That last row is the point of the field: without it, an empty `entities` list
means either "nothing matched" or "this was never going to work", and the two
are indistinguishable from the response alone.

## What degrades without a provider

Only the semantic half. Everything lexical keeps working:

- **Works:** SPARQL (including exact-match grounding), `/query`, `/knot`,
  `/context` text search, link expansion, SHACL, the reasoner.
- **Empty or refused:** vector similarity in `/search` and
  `quipu_hybrid_search`, the `Semantic` relevance hits inside `/context`.

`quipu_hybrid_search` called with a `query` string and no provider returns an
error naming the missing configuration, rather than an empty result set. You
can also bypass the provider entirely by passing a pre-computed `embedding`
array.
