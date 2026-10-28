# agent-branches: agent-run branches, promoted

N agents each work on a fork of one shared memory graph while a live writer
keeps writing to ROOT. Every fork is then promoted. The benchmark measures
what a person has to answer, what is lost without anyone being asked, and
whether as-of reads are right after a back-dated correction lands.

The agents are deterministic scripts. No LLM is called anywhere, so the run is
its own ground truth.

Results: [`results/RESULTS.md`](results/RESULTS.md). Book page:
[`docs/book/src/benchmarks/agent-branches.md`](../../docs/book/src/benchmarks/agent-branches.md).

## Run

```bash
python3 benchmark/agent-branches/run.py                  # everything: seeds 1 7 42 99 2026
python3 benchmark/agent-branches/run.py --no-terminusdb  # skip the Docker arm
python3 benchmark/agent-branches/run.py --seeds 42 -- --agents 16 --overlap 0.8
just bench agent-branches                                # same as the first line
```

`run.py` builds `examples/agentbranches` (`--features shacl`) and runs it once
per seed. It then starts TerminusDB v12.0.8 in Docker, pinned by digest
(`terminusdb/terminusdb-server@sha256:43061044d185…`, see `TDB_DIGEST` in
`run.py`),
(port 6366 on localhost), replays each scenario through it, and removes the
container. If the run pulled the image, it removes the image too. It
re-scores every Rust arm in Python and stops if the two scorers disagree.
Finally it writes `results/RESULTS.md` and `results/summary.json`. Per-seed
detail (every question, every state of ROOT, every probe answer) lands in
`out/agentbranches-seed<N>.json`, which is gitignored.

## The scenario

The vocabulary and shapes are the merge paper's (`examples/mergebench/shapes.rs`):

- `label`, `status`, `owner` and `version` have `sh:maxCount 1`.
- `tag` has `sh:maxCount 4`.
- `note` and `relatedTo` are unbounded.

Defaults per seed:

- **Base:** 48 entities, every predicate populated, 2 of the 4 allowed tags.
- **8 agents** each fork ROOT at the base transaction and make 10 edits, half
  of them aimed at 4 contended entities:
  - set or clear a single value;
  - add a tag (at most 2 per agent per entity, so every fork is valid on its
    own);
  - add a note or relation.

  Edits on the agent's own entities never conflict.
- **One back-dated correction per agent.** It sets `status` on one of the
  agent's own entities, valid from a past date between March and August. It is
  written into the fork at that valid time, the way
  [the temporal model](../../docs/book/src/concepts/temporal-model.md)
  documents a correction.
- **16 live writes to ROOT** after the forks are taken: added notes, which the
  pre-#888 promote silently retracted, and `version` bumps, which collide with
  agents that also bumped a version.
- **Promotion in agent order.** When a promote reports a conflict, a scripted
  person rules for the promoting agent: on each conflicted slot, ROOT takes the
  fork's values. Then the promote is retried. Every arm gets the same ruling.

## Arms

| arm | runs on | what it is |
|---|---|---|
| `quipu-promote-3way` | real `quipu::Store` | `fork_create`, writes into the fork, the shipped `fork_promote` (#888) |
| `quipu-promote-2way-pre888` | real `quipu::Store` | **negative control**: the pre-#888 promote, reimplemented from its definition. It applies `fork_diff(main, fork)` to ROOT; it is not built from old code |
| `quipu-direct-root` | real `quipu::Store` | **as-of control**: no forks; each correction is written straight into ROOT |
| `terminusdb-apply` | TerminusDB 12.0.8 | a branch per agent, then `POST /api/apply` with `before_commit` = the base commit, the documented merge. A 409 conflict or a 400 schema failure is a question; the person applies the ruling on main ("Option A" in their merge how-to) and retries |
| `shape-aware`, `triple-3way`, `context-merge`, `union`, `lww-theirs`, `git-canonical` | model graphs | the merge paper's strategies (`examples/mergebench/strategies.rs`, reused, not copied), run end to end over the same N promotions. `git-canonical` is the positional baseline: the real `git merge-file` over sorted N-Triples |
| `two-way` | model graphs | the pre-#888 promote on model graphs (ROOT := fork) |

The harness checks itself on every seed. The real-store 3-way promote must
equal the model `shape-aware` operator: same questions, same oracle, same ROOT
after every promotion. The real-store pre-#888 promote must equal the model
`two-way` arm. If either check fails, the harness exits non-zero.

## Accounting

- **questions**: slots an arm put to a person, summed over promotions.
- **needed**: the merge paper's oracle (`ground_truth` in
  `examples/mergebench/generate.rs`, without the alias class) over the arm's own
  `(base, ROOT before this promotion, fork)`. Arms whose earlier merges left
  ROOT in different states see different inputs, so `needed` differs between
  arms. **missed** = needed but not asked. **unnecessary** = asked but not
  needed.
- **silent losses**: a scripted write is *due* in ROOT from the promotion of
  the agent that made it (the live writer's writes from the start). The first
  later state of ROOT that no longer reflects the write is where it was
  dropped. The drop is silent unless that promotion asked about the slot. A
  writer's own later write to a single-valued slot supersedes its earlier one.
  This is not a loss.
- **violations**: final-ROOT slots over their `sh:maxCount`.
- **as-of probes**, four per correction:
  - `valid-before`: the value valid before the correction date. Expect the old
    value.
  - `valid-after`: the value valid after it. Expect the new value.
  - `current`: expect the new value.
  - `belief-before-landing`: the transaction-time cut just before the
    correction reached ROOT, asked about the after-date. Expect the old value.

  Quipu probes go through the public SPARQL path (`query_temporal` with
  `valid_at` / `as_of_tx`).

## What the TerminusDB arm cannot express, and how it is scored

- **No valid time.** A back-dated correction is an ordinary commit; its valid
  date goes in the commit message. The valid-time probes are answered from
  main's HEAD, the most charitable native reading. Commit-time travel to a
  probe date would answer from before the database existed, because commits
  carry wall-clock times. `belief-before-landing` is a commit read, which
  TerminusDB does natively.
- **Set overflow after a ruling.** A `Set` diff is a positional list patch
  (`CopyList` / `SwapList`). After the person applies a ruling on main, the
  retried apply re-inserts the branch's additions. The result is a cardinality
  failure again: 6 members against a maximum of 4. `match_final_state` and
  `keep` do not change this (checked against 12.0.8). In that case the person
  lands the branch by hand with a per-field three-way merge. A value both sides
  changed is asked and counted, the same as anywhere else. The results table
  reports how many merges went this way.
- **One failure class per attempt.** A 409 lists patch conflicts. A 400 lists
  the first schema failure it hit. Further questions surface on later attempts,
  which is why its promote-attempt count is higher.
- **No merge commits.** The commit graph is linear (`Commit.parent` is
  `Optional`). `apply` lands each branch as one squash commit.
- Not run: `apply` with the target tip as `before_commit`. Their docs say this
  silently reverts target-side changes. It is the analogue of the pre-#888
  promote.

The schema mirrors the shapes: `Optional` for the single-valued predicates, and
`Set` with `@max_cardinality 4` for `tag`. `owner` and `relatedTo` are stored as
strings rather than document links. Referential integrity is not under test.

## Known bug measured here

Through `fork promote`, every `valid-after` probe is wrong: ROOT answers both
the old and the corrected value. Promote closes ROOT's superseded value at the
promote instant, not at the correction's valid time. This is tracked as P0 bug
aegis-jxxd. `quipu-direct-root` scores 160/160, which isolates the bug to
promote. Re-run the benchmark when the fix lands.

## What this cannot establish

- **The workload is synthetic.** Every conflict is triple-visible, so the
  shape-aware operator matches the oracle by construction. This is the same
  caveat as `benchmark/mergebench/BUILD_REPORT.md` §3. The numbers measure what
  the other arms cost, and they show the shipped promote does what its model
  says. They are not evidence that the oracle is the right one.
- **Corrections sit on slots no one else touches.** This keeps the as-of
  probes about time semantics alone. A correction that also conflicts is not
  measured here.
- **One fixed ruling (the promoting agent wins).** Another ruling changes the
  final states, but not which slots needed a person.
- **TerminusDB runs over HTTP; Quipu runs in process.** No timings are reported
  for that reason.
