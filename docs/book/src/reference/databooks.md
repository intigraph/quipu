# DataBooks

A [DataBook](https://github.com/kurtcagle/databook) is a Markdown document
with YAML frontmatter and typed fenced blocks: Turtle, TriG, N-Quads, SHACL,
SPARQL. It is the document format of the W3C Holon Community Group. Quipu
writes one graph as a DataBook and reads a DataBook's graph data back. The
implementation follows the DataBook 1.5.0 release (format specification
v1.1 plus the 1.5.0 changes).

## `quipu databook export`

```sh
quipu databook export --graph https://example.org/people --out people.databook.md
quipu databook export --graph https://example.org/people --tx 41 --with-shapes
quipu databook export --title "Root graph" --version 2.0.0 > root.databook.md
```

Without `--graph` the export is ROOT, named `urn:quipu:graph:root`. `--tx N`
and `--valid-at <date>` export the graph as it stood at that point instead of
now. `--with-shapes` adds each stored shape set, as of the same point, as a
`shacl` block. Output goes to stdout, or to `--out <file>`.

The document holds:

- **Frontmatter.** `id`, which defaults to `urn:quipu:databook:sha256:<hex>`
  over the graph's Turtle, so the same content always gets the same id. Also
  `title`, `type: databook`, `version` (default `1.0.0`) and `created`.
- **`graph`.** `named_graph`, `triple_count` and `subjects`.
- **A process stamp.** It names Quipu as the transformer and the graph as the
  primary input.
- **`quipu.tx`.** The transaction the export reflects, plus `quipu.valid_at`
  for a valid-time export.
- **One `turtle` block** holding the graph as canonical, sorted Turtle.

Every block carries `databook:id`, `databook:graph` (data) or
`quipu:shapes-name` (shapes), and `quipu:sha256` over its payload. These
annotations are written on the lines before the fence, which is the form the
1.5.0 reference parser treats as canonical. So each fence holds only its
payload, and a plain Markdown tool can pass it straight to a Turtle parser.
Two exports of unchanged content are byte-identical, apart from the
`timestamp` you pass.

## `quipu databook import`

```sh
quipu databook import people.databook.md --actor alice
quipu databook import people.databook.md --graph https://example.org/staging
quipu databook import people.databook.md --with-shapes
```

Import reads both frontmatter forms: bare `---` and the 1.0 `<script>`
wrapper. It reads annotations before or inside a fence. It loads every
`turtle`, `turtle12`, `n-triples`, `trig`, `n-quads`, `manifest`,
`transformer-library` and `processor-registry` block. The target graph is
chosen in this order:

1. `--graph`, if given, for every block.
2. Otherwise, a TriG or N-Quads graph name.
3. Otherwise, the block's `databook:graph`.
4. Otherwise, the frontmatter's `graph.named_graph`.
5. Otherwise, ROOT.

All graphs commit in **one** all-or-nothing write. The transaction source is
`databook:<id>@<version>` unless you pass `--source`.

Import checks every `quipu:sha256` before it writes anything, and refuses the
document if a block has changed. A block Quipu cannot store, such as RDF 1.2
triple terms in a `turtle12` block, refuses the whole document by name, and no
graph is registered.

Import **never executes** a block:

- `sparql` and `sparql-update` blocks are reported as skipped.
- `shacl` blocks load as stored shapes only with `--with-shapes`, after the
  data commits. Shape loading is a separate step.
- `json-ld` and encrypted blocks are skipped.
- A block marked `mode=printed`, `hidden`, `reference` or
  `databook:display-only: true` is shown for reading and not loaded.

The command prints a JSON report: the graphs written, with transaction and
fact count, the shapes loaded, the blocks skipped and why, and any warnings.

Warnings follow the DataBook rules (SPEC §15.4). A missing `created` date, a
missing process stamp, or a `graph.triple_count` that does not match the data
is a warning, and the data still loads. A missing identity field, an
unrecognised `type`, malformed YAML, or no fenced data block at all is an
error.

## Limits

- One graph per export. A whole-store TriG export, with bulk data written
  beside the DataBook, is not built yet.
- Stored queries are not exported. Their `{param}` templates are not
  executable SPARQL, and mapping them to DataBook `VALUES` parameters is
  separate work.
- Blank nodes are scoped per block. Exporting and re-importing gives the same
  blank-node identities, but two blocks never share a blank node.
- The 1.5.0 reference parser recognises only three-backtick fences. Export
  therefore refuses a payload line that starts with three backticks; canonical
  Turtle escapes newlines, so graph data never contains one. Import also
  accepts longer fences.
