# First query error counter baseline

With a first counter sample of 1 and no preceding sample, Prometheus computes
zero increase. With the same error following a scraped zero, it computes a
positive increase and rate. The fixture checks both results in the actual
Prometheus engine:

```sh
promtool test rules first-event-promql.yml
```

[Readable transcript](transcript.txt) (Promtool 3.5.0; transcript fallback,
no terminal recording captured). The lazy input is `_ 1 1`; the baseline input
is `0 1 1`. The three assertions passed under the HOME escape guard. No server,
production data or forced timeout is involved.

The source baseline is commit `5483ea64580c27a7c4d43bd1b200773d9df12355`;
the accompanying change exports zero `/query` outcome counters. This demo
isolates the PromQL effect; the Rust regression tests the emitted metric text.
It does not establish installed acceptance. The zero must be scraped before
the first event: no exporter can reconstruct an earlier missing scrape.
