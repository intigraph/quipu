# Current label preservation

An ordinary episode that names an existing node by its slug keeps that node's
current graph-local label. The response now includes a resolution candidate with
`matched_on: "label kept: <existing label>"`, even when similarity resolution is
disabled. Existing similarity hints remain present. Idempotent no-op replays do
not report a newly discarded assertion. Snapshot replacement retains its existing
label behavior.

The lookup binds the label attribute, subjects and graph, rather than decoding
each subject's complete history. Retired labels and labels in another graph do
not suppress the incoming label.

The private in-memory baseline at `abb9e5a1` took 1,024,702 microseconds for 64
calls on a node with 10,000 unrelated facts and one label; the same candidate
probe took 1,948 microseconds, and the final recorded run took 3,961 microseconds. The old missing-hint assertion failed with `[]`;
the candidate passes. These are source fixture measurements, not production
latency or a service SLO. The replayable candidate recording and timing are
`output.log` and `timing.log`; download both for `scriptreplay`. Retained baseline
stdout is textual evidence, not a timed baseline recording.

Run `bash docs/probes/kept-labels/run-demo.sh` from the source root. The test uses
an in-memory store and never calls a service. Recordings contain source fixture
output only.
