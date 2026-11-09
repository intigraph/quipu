# Native data-merge risk plan demo

The same copied candidate executable promotes a protected fixture with receiver
policy inactive, then refuses it with policy configured and leaves ROOT unchanged.
Both runs verify the original plan and reject a mutated proof. Every store is
private and in memory. This demonstrates a policy control on candidate source;
it is not a base-binary comparison or installed/runtime acceptance.

Base source: `f2aa50698952df037b945385b5d4d47d5856d068`. The candidate is the
source accompanying this recording in this commit. No deployment policy was
installed. The planner and policy adapters are native-only.

Read [the actual transcript](demo-transcript.txt), or download
[the terminal recording](demo.cast) and replay with `asciinema play demo.cast`.
The recording is asciicast v2, not an inline video.

Reproduce the inactive control with `just demo merge-risk-plan`. For the configured
control, set `QUIPU_MERGE_RISK_POLICY` to the following receiver-owned JSON and run
`cargo run --locked --offline --no-default-features --example merge_risk_plan -- --configured`:

```json
{"threshold":40,"protected_classes":["urn:Credential"],"protected_terms":["urn:value"],"recall_queries":{}}
```

The small recording and transcript are retained in Git with the feature. No model,
network service, production store, or runtime activation is part of this demo.
