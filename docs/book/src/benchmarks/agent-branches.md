# Agent-run branches

Give every agent run its own branch of a shared memory graph. Let the agents
work concurrently while the shared graph keeps moving, then land every branch.
Three questions decide whether the result can be trusted:

1. **What does a person have to answer?** Ideally every real conflict, and
   nothing else.
2. **What is lost without anyone being asked?** Ideally nothing.
3. **After a late correction ("this was actually true since March") lands, do
   as-of reads give the right answer for every date?**

This benchmark measures all three for Quipu forks and for TerminusDB branches,
alongside the merge strategies from the shape-aware merge paper. Every agent is
a deterministic script. No LLM is called, so the run is its own ground truth.

## Reproduce

```bash
python3 benchmark/agent-branches/run.py      # or: just bench agent-branches
```

This needs Rust and, for the TerminusDB arm, Docker. The script starts
TerminusDB v12.0.8 on localhost, pinned by image digest (`TDB_DIGEST` in
`run.py`), and removes it afterwards.
Pass `--no-terminusdb` to skip it. Method, accounting and limits are in
[`benchmark/agent-branches/README.md`](https://github.com/intigraph/quipu/blob/main/benchmark/agent-branches/README.md).
Read that file before quoting a number.

## The workload

Each of five seeds runs the same pattern:

- 8 agents fork one 48-entity graph.
- Each agent makes 10 edits, half of them aimed at 4 contended entities. The
  edits cover single-valued slots (`sh:maxCount 1`), a tag list capped at
  `sh:maxCount 4`, and unbounded notes.
- Each agent also makes one correction to a status, valid from a past date.
- While the agents work, a live writer adds notes and bumps versions directly
  in ROOT.
- Every branch is then promoted in turn. When a conflict is reported, a
  scripted person rules for the agent, and the promote is retried.

## Results

{{#include ../../../../benchmark/agent-branches/results/RESULTS.md:3:}}

## What the numbers say

- **The shipped promote asks exactly the oracle's questions, and that match
  holds by construction.** The conflict oracle uses the same `sh:maxCount` rule
  as Quipu's merge, and every conflict here is visible at the triple level. So
  "40 asked, 40 needed" shows that the shipped promote implements its rule, not
  that the rule is the right one. What the benchmark does measure independently
  is what the other strategies cost on the same inputs, and that nothing is
  lost: Quipu's three-way `fork promote` had no silent losses and no
  `sh:maxCount` violations, and the real store matched the model of its
  operator on every seed. The positional baseline (`git merge-file` on sorted N-Triples) also
  lost nothing, but it asked 188 questions, 162 of them unnecessary.
  Node-overlap merging asked 422.
- **The old promote was the failure this benchmark exists to catch.** The
  pre-#888 two-way promote asked nothing and silently dropped 449 of the 520
  scripted writes. Every promote overwrote ROOT with the fork, so later
  promotes erased earlier agents' work and the live writer's notes. This is
  the negative control.
- **TerminusDB matched on questions and losses, through its documented merge.**
  The merge is `/api/apply` with the base commit as the common ancestor. It
  surfaced the same 40 questions and lost nothing, but it needed more attempts,
  because each attempt reports one class of failure. Three of the 40 merges
  could not be landed by the documented retry at all. A tag overflow ruled on
  main re-applied as a positional list insert, and the result failed its
  cardinality check again. A person landed those branches by hand.
- **Back-dated corrections hit a promote bug, tracked as P0 bug aegis-jxxd.**
  The store itself is not at fault. Each correction gets four as-of questions:
  - **Direct writes to ROOT:** all 160 correct.
  - **Through `fork promote`:** the after-the-correction-date question is wrong
    in all 40 cases. ROOT returns both the old and the new value. Promotion
    carries the fork fact's own `valid_from`, but it closes ROOT's superseded
    value at the promote instant, not at the fork's correction date. So
    between the correction date and the promote, both values are valid. This
    is bug aegis-jxxd (P0). The numbers above are from before its fix; re-run
    the benchmark once it lands.
  - **TerminusDB:** has commit time only. It cannot express "valid from
    March" at all, so it gets the before-the-correction-date question wrong in
    all 40 cases. That is scored from its HEAD, the most charitable reading.
  - **Transaction-time reads ("what did we believe before it landed?"):**
    correct for every arm that keeps history.

## What it cannot establish

The conflicts are all visible at the triple level. That means the shape-aware
operator matches the oracle by construction: the benchmark measures what the
other strategies cost, not whether the oracle is the right one. Corrections
sit on slots no other writer touches, so the as-of numbers are about time
semantics alone. The person always rules for the promoting agent. TerminusDB
ran over HTTP and Quipu in process, so no timings are reported.
