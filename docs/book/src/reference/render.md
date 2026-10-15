# Render graph data into owned files

`qp render` produces markdown locally from a stored specification or an ad-hoc
query. A remote server prepares a read-only plan; the client writes the files.
Templates have no file loader, network access or nondeterministic functions.

```sh
mkdir -p output
qp render --query 'SELECT ?title WHERE { VALUES ?title { "Example" } }' \
  --template '# {{ rows[0].title }}' --dest output/report --db private.db
qp render --query 'SELECT ?title WHERE { VALUES ?title { "Example" } }' \
  --template '# {{ rows[0].title }}' --dest output/report --db private.db --dry-run
```

The second command reports a no-op. Both the file and manifest retain their
bytes and modification times. Dry-run prints the complete file list and the
content/mode diff without creating directories, lockfiles, manifests or events.
The destination's parent must already exist.

## Store the specification as data

```turtle
@prefix render: <urn:quipu:render:> .
<urn:example:report> a render:RenderSpec ;
    render:query "SELECT ?title WHERE { VALUES ?title { 'Example' } }" ;
    render:format "markdown" ;
    render:layout "single" ;
    render:path "report.md" ;
    render:template "# {{ rows[0].title }}" .
```

Validate this file against the supplied `shapes/render.ttl` when importing it,
then render `<urn:example:report>`:

```sh
qp knot report.ttl --shapes shapes/render.ttl --db private.db
qp render urn:example:report --dest output/stored --db private.db
```

A specification carries its query, layout, path, optional template, includes
and optional provenance footer. `markdown` is the first format; the renderer
trait leaves room for other document formats. Binary postprocessing is outside
core. Stored specifications and included byte literals travel in a pendant.
Normal share/export scrub and destination rules still apply.

## Hierarchies and byte-exact includes

Use `per-entity` layout and a path such as `{scope}/{slug}/SKILL.md`. Each row has
its own `row` context; single-file templates receive all sorted `rows`. The
small deterministic template language supports loops, conditionals and the
`upper` and `lower` filters. Undefined variables, path collisions, traversal,
case aliases and incomplete queries refuse before publication.

Includes are a JSON literal in `render:includes`, or an `includes` column on
per-entity rows. Each entry has a relative path, `sha256:` content address,
base64 bytes and a mode of `0644` or `0755`. Raw markdown templates can copy
`row.content` without parsing or rewriting YAML front matter. Includes preserve
executable modes and are verified against their hashes.

Render skills into their own root outside the harness's skills directory, then
let the existing tooling projection mount individual skill names. A scoped
query can select internal skills. The renderer does not overwrite unrelated
harness entries or manage those mounts.

## Ownership, time and publication

`.quipu-render.json` records the specification, `source_tx`, `valid_at`, the
render timestamp and every managed file's hash/mode. The complete plan is
prepared within one SQLite read snapshot. Present reads pin that transaction;
historical `--valid-at` reads use the same snapshot without changing the native
mixed-clock guard. A write committed during rendering cannot change the plan.

```sh
qp render urn:example:report --valid-at 2026-01-15T00:00:00Z \
  --dest output/historical --db private.db
qp render urn:example:report --dest output/remote --server https://graph.example
```

Only manifest-owned files can be updated or removed. Unknown files, edited
managed bytes, changed modes, symlinks and a changed destination refuse. The
client stages a complete directory and atomically publishes it on Linux/macOS.
Unchanged files retain their inode and modification time across an update.
A failed or indeterminate publication is not permission to retry blindly;
inspect the retained destination and any reported staging directory first.

The manifest is the receipt for the current render, not a disk-to-graph sync or
an append-only audit history. Historical graph access, installed CLI identity,
shape projection and a real consumer run are separate from source tests.

## Recorded CLI demonstration

`docs/probes/pr-demos/render/reproduce.py` invokes the actual candidate CLI
against a private temporary local store. The recorded result is:

```json
{
  "target": "candidate CLI / private local store",
  "before": "destination absent",
  "first_files": [
    "report.md"
  ],
  "after_utf8": "# Example",
  "repeat_noop": true,
  "repeat_same_bytes_mtime_inode": true,
  "unknown_file_refused_and_preserved": true,
  "running_installation": "not invoked"
}
```

The absent destination becomes one markdown file; the repeated render preserves
bytes, inodes and modification times. An unknown file is refused and preserved.
This recording makes no installed-service or deployment claim.
