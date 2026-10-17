# Embeddings and Semantic Search

Quipu answers two different kinds of question. **Lexical** retrieval (SPARQL,
`CONTAINS`, exact match) needs nothing but the fact log. **Semantic** retrieval
(`/context`, `quipu_hybrid_search`, `/search`) needs vectors, and vectors need
an embedding provider you configure yourself.

Nothing about semantic retrieval is on by default. This page is the checklist
for turning it on, and — just as important — for telling whether it is on.

## What `knot` does and does not do

This asymmetry surprises people, so it is worth stating plainly:

| Write path | Auto-embeds? |
|------------|--------------|
| `POST /episode` / `quipu_episode` | Yes, when `auto_embed = true` |
| `quipu knot` / `POST /knot` (Turtle ingest) | **No** |
| `quipu-server --embed-backfill` | Yes, all entities, once at startup |
| `POST /embed_backfill` | Yes, all entities, on demand |
| `POST /embed_backfill_graph` | One named graph, bounded per call |

A graph loaded from Turtle therefore holds **no** embeddings. Semantic
retrieval over it returns nothing at all — successfully, with a `200` and an
empty result — until you backfill.

```console
$ quipu knot alphax.ttl --db na.db
knotted 2579 facts from alphax.ttl (tx 1)

$ quipu-server --db na.db --embed-backfill   # <- the missing step
```

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
be set — with either missing, the server skips provider construction entirely
and starts without embeddings.

## Telling whether it worked

Three signals, in the order you will meet them.

**At startup**, a loaded provider announces itself:

```text
ONNX embedding provider loaded (dim=384, auto_embed=true, deferred)
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

### Model identity and unchanged text

The built-in SQLite vector backend records a nullable model identity on each
new embedding. Automatic embedding skips unchanged text only when both the text
and a **known** model identity match. Changing the model, tokenizer, configured
dimension or token limit therefore makes touched entities eligible for embedding
again, even when their text is identical. This does not automatically rebuild the
whole store; use the explicit embedding backfill to rebuild untouched entities.
A failed or partial backfill leaves the remaining old-model vectors observable.
Switching back to a previous provider does not certify a mixed store as current.

`EmbeddingProvider::model_identity()` must describe an immutable model,
tokenizer and preprocessing revision for the provider's lifetime. Its default is
`None` (UNKNOWN); a model name or file path alone is insufficient. The standalone
ONNX provider hashes the exact model/tokenizer bytes loaded into an in-memory
session, dimension, token limit and pooling/normalization revision. Models that
need file-backed external weights remain supported with UNKNOWN identity.
UNKNOWN providers re-embed touched entities instead of skipping unchanged text.

Legacy vectors, low-level writes without identity and vector backends that do
not implement provenance are not silently assigned the configured model's
identity. Rebuilding produces provenance where the backend supports it. Portable
archive/import paths that copy only the vector fields also yield UNKNOWN.
SQLite's migration adds a nullable column on writable open; read-only opens do
not migrate. Older binaries can read the additive column, but writes from them
carry no identity and need verification or backfill after returning to a newer
binary. Deferred results are discarded if their provider changes before apply;
a backfill refuses provider replacement rather than stamping old results with a
new identity.

The server caches provenance counts with its background graph metrics:

- `quipu_embedding_model_mismatch_vectors`: current vectors with a different
  known model identity.
- `quipu_embedding_model_unknown_vectors`: current vectors without identity,
  or all current vectors when the configured provider identity is UNKNOWN.
- `quipu_embedding_model_counts_ready`: 1 when the backend reports these counts;
  0 when unsupported, not yet measured, or the refresh failed. Unavailable counts
  are omitted rather than reported as zero.

Scraping these metrics does not scan vectors or acquire a store connection.
There are no model-hash labels. Counts are observations, not proof of whole-store
rebuild completion, and they do not by themselves disable semantic search.
Older strict-schema maintenance tools may refuse the additional column; verify
archive and migration compatibility before using them after a rollback.
