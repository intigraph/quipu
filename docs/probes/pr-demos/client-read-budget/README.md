# Remaining client read budget

Trigger: a client has no budget left but sends a valid read. The old handler
ignores the header; the candidate returns 408, while an absent or generous
header still produces a real positive result on the same private store.

Reproduce the candidate controls:

```sh
cargo test --locked --features full --bin quipu-server client_budget -- --nocapture
```

The native handler fixtures exercise `/query` and `/context` response statuses,
a populated context result, cap clamping, invalid/duplicate header refusals,
queue expiry without executing work, real query interruption followed by a
successful positive query, and detached cancellation ownership. No running
service is changed. The execution test uses a small private graph and a bounded
cross-product query; it does not replay a production query.

Before measurement: server `fc6d989ef102527126c3dd85a4cbe834d3589694`
returned HTTP 200, `{"labels":null,"result":true}` for `ASK {}` with
`X-Quipu-Timeout-Ms: 0`. Candidate source is based on
`453845adccd9a6c3ce56950a21ffa3feb8420a1f`; the same zero-budget query
is covered by the private control below, alongside the positive absent-header
case. These are different targets (installed before, private candidate after),
so this is a behavioral demonstration, not deployed acceptance or a latency
comparison. Installed same-probe and caller adoption remain separate gates.

[Retained response transcript](transcript.txt) is a text fallback, not a video
or a claim that the candidate is installed. It excerpts the actual test output.
