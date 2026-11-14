# A two-sided pendant merge, end to end

This directory is a worked example of merging a pendant with Git. Two branches
changed the same pendant; `quipu git-merge` merged what could be merged and
stopped on the one fact it could not decide.

The pendant is a small service roster. Its shape says a service has at most one
owner:

```turtle
ex:ServiceShape a sh:NodeShape ;
    sh:targetClass ex:Service ;
    sh:property [ sh:path ex:owner ; sh:maxCount 1 ] .
```

## The history

| Commit | Change |
|---|---|
| base | `api` owned by alice; `search` owned by bob |
| ours | adds `indexer` (carol); changes the `api` owner to dana |
| theirs | adds `notifier` (erin); changes the `api` owner to frank |
| merge | both additions kept; the `api` owner decided as frank |

A plain `git merge` of these branches writes conflict markers into `export.nt`
and leaves a manifest whose hash matches neither side.

## What the merge did

`quipu git-merge <ref>` runs the merge with the pendant driver:

- **Merged without asking:** the `indexer` and `notifier` additions. They touch
  different subjects, so both are kept.
- **Stopped and asked:** the `api` owner. Each side alone satisfies the shape;
  together they hold two owners, which `sh:maxCount 1` forbids. Nothing is
  written until someone chooses.

The choice is recorded in [`pendant/decisions.json`](pendant/decisions.json):
the rule, the base, ours and theirs values, the side chosen, who chose it and
when. `manifest.json` is rewritten for the merged graph.

## Reproduce it

```bash
git checkout <ours>
quipu git-merge <theirs>
quipu git-merge resolve docs/examples/pendant-merge/pendant conflict:0 --choose theirs --by <you>
git add docs/examples/pendant-merge/pendant && git commit
quipu pendant-check <base> <ours> <theirs> HEAD
```

`.gitattributes` already routes `**/pendant/export.nt`, `shapes.ttl` and
`manifest.json` to the driver. CI replays every merge commit in a pull request
with `pendant-check`, so a hand-edited result does not pass.
