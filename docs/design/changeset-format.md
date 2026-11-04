# Store-wide change-set format (`quipu-changeset` v1)

Status: implemented with `GET /diff` and `POST /patch` (aegis-4mk, gap G3 in
[terminusdb-gap-analysis.md](terminusdb-gap-analysis.md)). This is the
unsigned format. Local-sync push-back (aegis-s24.4,
[local-sync-mode.md](local-sync-mode.md) §8) wraps it in the signed
`quipu-changeset-v1` envelope rather than defining a second one.

## Why this shape

[standard-share-artifact.md](standard-share-artifact.md) already chose
parent-bound SPARQL Update for share deltas: ground `DELETE DATA` followed by
ground `INSERT DATA`. Quipu parses SPARQL already, any SPARQL 1.1 endpoint can
apply the body, and a comment header stays inside the bytes without changing
what the update means. This document promotes that choice to every place
Quipu exchanges a change: fork diffs, transaction ranges, share deltas, and
patches applied over HTTP.

## Body

A change-set is two sets of ground statements: **deletes**, then
**inserts**. A statement is one RDF triple plus an optional named graph. No
variables, no `WHERE`, no `LOAD`/`CLEAR`/graph management, no relative IRIs.
A statement that is both deleted and inserted is refused.

Statements are rendered in their canonical N-Triples term form, sorted, with
no duplicates. One statement goes on each line, so the same change always has
the same bytes:

```sparql
# quipu-changeset: v1
# base-tx: 4180
# target-tx: 4182
# source: fork:feature
# shapes-hash: 6f1c…
# digest: sha256:9a0e…
DELETE DATA {
  <http://ex/s> <http://ex/status> "open" .
  GRAPH <http://ex/g> { <http://ex/s> <http://ex/owner> "wu" . }
};
INSERT DATA {
  <http://ex/s> <http://ex/status> "closed" .
};
```

An empty side is omitted. A change-set with no statements is just the
header.

## Header

The header is a run of `# key: value` comment lines before the first
operation. Readers ignore unknown keys. Writers emit keys in the order below.
Every key is optional when reading. Writers always emit `quipu-changeset`; a body with no header (an old share delta) reads as v1.

| Key | Meaning |
|---|---|
| `quipu-changeset` | Format version, `v1`. Any other version is refused. |
| `base-tx` | Transaction whose state the deletes were read from (the pre-state). |
| `target-tx` | Transaction whose state the inserts reproduce, when the post-state is a recorded state. |
| `base-share` | Parent share id, when the pre-state is a share (share deltas). |
| `store-id` | `store_id` of the store the base was read from. |
| `valid-time` | RFC 3339 `valid_from` for inserted statements, normalized to UTC (`Z`) on read; anything else is refused. Defaults to the apply time. Transaction time is always the receiver's. |
| `source` | Provenance label, recorded as the applying transaction's `source`. |
| `shapes-hash` | SHA-256 of the shape sets the producer had in force, in the as-of-share encoding (`/diff`: at `base-tx`; share deltas: the result share's). It is advisory: the receiver always validates against its own current shapes. |
| `digest` | `sha256:` once, then the digest defined below as 64 lowercase hex characters. When present, it must match or the change-set is refused. |

`log_epoch` and `base_slice_hash` belong to the signed envelope (s24.4). They
are not header keys here.

## Digest

The digest is the lowercase hex SHA-256 over the change-set's RDF Patch body lines (`D …` for
each delete, then `A …` for each insert, sorted as above, each line ending in
`\n`). It covers statements only, not the header. That makes it the
`delta_sha256` the s24.4 envelope signs. The envelope binds base and valid
time itself.

## RDF Patch by content negotiation

`application/sparql-update` is the primary media type. `application/rdf-patch`
carries the same change in [RDF Patch](https://afs.github.io/rdf-patch/) form.
Header keys become `H` rows with string literals. The statements sit in one
`TX`/`TC` block:

```text
H quipu-changeset "v1" .
H base-tx "4180" .
TX .
D <http://ex/s> <http://ex/status> "open" .
D <http://ex/s> <http://ex/owner> "wu" <http://ex/g> .
A <http://ex/s> <http://ex/status> "closed" .
TC .
```

Both forms parse to the same change-set, with the same digest.

## Apply semantics (`POST /patch`)

Applying is parent-bound and idempotent. Only the touched statements are
compared, in one store lock:

- **Post-state already holds:** every delete is absent and every insert is
  present. The result is `200` with `already_applied: true`, and nothing is
  written. Retrying a lost response is safe.
- **Pre-state holds:** every delete is present and every insert is absent.
  The change commits through the same write gates as `/update`: authority,
  placement, policy, OWL and reject-mode SHACL. A gate refusal writes nothing.
- **Anything else** is `409 Conflict`. The response lists the statements that
  are already in their post-state, and nothing is written. Three-way merging
  at `base-tx` is s24.4's job. `/patch` never retargets a change-set onto a
  different base.

Blank nodes are refused on `/diff` and `/patch`. Re-inserting a blank node
mints a new node, so applying it again would not be idempotent. Skolemize
first, as `git_merge` already requires.

## Producers

- `GET /diff?from=&to=`: each side is `tx:N` (ROOT as of transaction N),
  `main` (ROOT now), or `fork:<name>` (a fork now). `graph=<iri>` diffs a
  named graph instead of ROOT. A side holding more statements than
  `server.update_full_copy_max_facts` (the `/update` whole-copy ceiling) is
  refused with `422 diff_too_large`, counted before anything is read. Shares are files, not store states, so a share
  diff is the share delta below rather than a `/diff` side.
- Share deltas: `delta.ru` is this format, with `base-share` set to the parent
  share. Old header-less deltas still materialize, because the header is a
  comment.

## Relation to s24.4

s24.4 signs `digest` as `delta_sha256`. It binds `base-tx`, `store-id` and
`valid-time` together with its own `log_epoch`, `base_slice_hash`, and
`operation_id`. It admits through a three-way merge instead of `/patch`'s
two-state check. The body, header, and digest stay exactly as written here.
