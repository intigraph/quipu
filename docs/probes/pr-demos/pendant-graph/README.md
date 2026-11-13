# Pendant graph review demonstration

The same local Person fixture changes Alice's age from 30 to 31. Before, the
installed CLI at `99c777e5fe97d08214e40052af5170b59c049db8` returns the semantic
report without graph context. The candidate adds Alice's four before/after
context edges while preserving the semantic diff and SHACL result. Open
[graph.html](graph.html) locally to compare the directed neighborhoods, and
expand an edge's identity keys. [report.json](report.json) is the actual source
report. This demonstrates a local fixture and candidate, not deployment.

The exact candidate source revision is the linked PR head. The implementation
extends the existing semantic snapshot model; blank-node identity and folding
retain its documented limits. The diagram and full retained edge list state
their bounds. Alias proposals and hardware verdicts remain separate.

[Terminal recording](output.log.gz), [timing](timing.log): download, decompress
`output.log.gz`, then run `scriptreplay --log-out output.log --log-timing timing.log`.
This is a replayable terminal recording, not an embedded video. Reproduce from
the repository root with a baseline CLI and candidate CLI:

```sh
PENDANT_BASELINE=quipu PENDANT_CANDIDATE=target/debug/quipu \
  bash docs/probes/pr-demos/pendant-graph/run.sh
```

The small recording and offline HTML are retained here with the implementation;
CI generates fresh per-pack graph artifacts on each applicable PR check.
