# Episode request budget

Trigger: resolution with a deliberately slow synthetic embedding provider and a
20 ms request deadline. All data belongs to a separate in-memory store.

On base `ec3b707dc2976e579e634f276e752c7715a9ac99`, one node committed in 4 ms.
The 19- and 57-node batches consumed 41 and 123 ms, then reported a zero query
budget. Both refusals left zero facts. [The actual baseline output](before.txt)
is a transcript, not a recording.

The candidate checks the deadline during resolution. Watch the original 20 ms
budget in the error and the unchanged fact counts. The recording identifies source revision
`8b7a473c4b77056071f66dc8849df02bc3ec2c26`; the final changes only add the
recording and caption. It also includes the one-node commit control.

Reproduce from a source checkout with the normal build environment:

```sh
cp docs/design/episode-budget-demo/probe.rs examples/episode_budget_demo.rs
cargo run --example episode_budget_demo
rm examples/episode_budget_demo.rs
```

[Candidate recording](output.txt) and [timing](timing.txt) are retained here as
small public artifacts. Download both files to replay with a terminal:

```sh
scriptreplay --log-timing timing.txt --log-out output.txt
```

This demonstrates private source behavior. It does not establish the production
configuration or deployed repair. The retained incident payload is not included.
The CI controls also exercise loaded vocabulary, 50 and 200 nodes, an expired
batch, atomic refusal, and a subsequent successful write.

The capture uses `python3 docs/design/episode-budget-demo/capture.py` under
`script`, in the normal guarded build environment. No inline playback is claimed.
