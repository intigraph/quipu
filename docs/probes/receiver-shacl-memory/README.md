# Receiver SHACL memory on a repository share

The release repository share is built and then re-imported into a fresh receiver
store (`scripts/build-repository-share.py`, `quipu import <stage>`). On v0.18.2
that receiver round-trip needs more memory than the producer's 4 GiB unit limit.
This probe records where the memory goes (aegis-49s, finding 3).

- Binary: `quipu-ai-v0.18.2`, release `x86_64-unknown-linux-gnu`.
- Input: that release's own repository share, 359,444 triples in `export.nt`
  (73 MB), with its `shapes.ttl` (223 node shapes).
- Measured with GNU time on a Linux host, 2 rayon threads unless noted.
- Reproduce: `docs/probes/receiver-shacl-memory/measure.sh <quipu> <share-dir>`.

## Result

| Measurement | Peak RSS | Wall |
|---|---|---|
| `quipu import` of the share, shapes loaded | 4.81 GiB | 48 s |
| `quipu import` of the share, no shapes loaded | 0.86 GiB | 25 s |
| `quipu validate` of the share | 4.46 GiB | 7.3 s |
| `quipu validate`, one trivial shape (data load only) | 0.46 GiB | 1.5 s |
| `quipu validate`, `RAYON_NUM_THREADS=1` | 4.21 GiB | 9.8 s |

The validation reports 0 violations and 1 warning. Nearly all of the import's
peak is SHACL validation, not parsing or storage.

Memory is linear in the data. It is not a quadratic blow-up:

| Prefix of `export.nt` | Triples | Peak RSS |
|---|---|---|
| 12% | 43,133 | 0.62 GiB |
| 25% | 89,861 | 1.28 GiB |
| 50% | 179,722 | 2.28 GiB |
| 100% | 359,444 | 4.46 GiB |

That is about 12.5 KB per triple. Validating each shape on its own shows three
shapes carry about 4.5 GiB between them; every other shape adds under 0.4 GiB:

| Shape | Focus nodes | Added peak |
|---|---|---|
| `ChunkShape` | 36,005 chunks | +2.0 GiB (about 56 KB per node) |
| `QuipuNoHandAuthoredIdentity` | 38,688 `owl:sameAs` subjects | +1.6 GiB |
| `CodeSymbolShape` | code symbols | +0.9 GiB |

## Why

The validator is `shacl` 0.3.36 (rudof) in native mode over an in-memory graph.
For each property shape it validates every value node as its own focus node and
records the outcome in a shared `(node, shape)` typing cache
(`validator/engine/validate.rs`, `validator/cache/shared.rs`). The cache lives
for the whole validation and is never evicted, so memory grows with the number
of value nodes: every label, mention and file path of every chunk and symbol.
A single rayon thread barely changes the peak, so the cost is the cache, not
concurrency.

`QuipuNoHandAuthoredIdentity` targets the subjects of `owl:sameAs`. Since the
public-name rewrite (#970), every published entity carries one, so that shape
now validates the whole entity set.

The share itself keeps growing: `export.nt` was 46 MB at v0.17.0, 54 MB at
v0.18.0 and 73 MB at v0.18.2. The receiver's peak grows with it.

## Not shown

This does not measure the producer's whole unit (indexing, query and import
together), nor any server-side admission path. It proposes no engine change:
not memoizing literal value nodes, or releasing the cache after each target
shape, are follow-up options.
