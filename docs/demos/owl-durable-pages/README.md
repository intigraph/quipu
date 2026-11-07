# Durable OWL proposal pages

A synthetic two-fact fixture uses the same property, range and subclass ontology
union in both paths. The retained inline `materialize_limited` path refuses at a
total limit of one. The durable path uses pages of one, produces six entailments
across four logical passes, and matches the complete canonical reference set.

The comparison invokes both paths inside the candidate library; it does not run
a separately rebuilt base binary. The inline total-budget algorithm is preserved.
Change base: `dcf112e0447ffafe654dca72dbe4ad68fa6eb114`.
Implementation: `761216dd4a02a62a04255f685f81730e15a4b7d3`.
Recorded native test binary SHA-256:
`9e45e489d904a796d0f915828839e560bf2053c5995fcc1df4d08d0e01aa090e`.

[Recording](output.typescript), [timing](timing.txt), and readable transcript
(the recording is plain text). This small recording is retained with the source.
Download both files and replay using util-linux:

```sh
scriptreplay --log-out output.typescript --log-timing timing.txt
```

Playback was checked with `scriptreplay`; an inline web player is not provided.
Reproduce from the implementation revision:

```sh
just test --locked --features full --lib \
  owl::snapshot::pages_tests::demo_total_budget_to_bounded_pages \
  -- --exact --nocapture
```

The recording passed the HOME guard. It demonstrates candidate source behavior,
not deployment, a production-size workload, or production writer latency. Native
controls separately exercise an actual 200,001-key promotion across a 200,000-key
page boundary and crash/reopen reconciliation.
