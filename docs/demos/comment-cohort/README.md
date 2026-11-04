# Same-transaction description comments

This source-only, in-memory example searches an indexed entity whose label and
three comments jointly cover seven query terms. The actual retained base
`21547f5a206dfc81788e7f70eef92f7f06051890` regression response reports 3/7
coverage and NO CONTEXT at similarity 1.0. The candidate must report coverage
1.0 and CONTEXT for the same input. An unrelated subject must remain NO CONTEXT.
The PR identifies the exact demonstrated candidate revision.

Run `just demo comment-cohort`. The example displays the measured
[base response](base-result.json), extracted from the guarded failing regression;
it does not rerun the base. The candidate and negative control are actual local
calls with a deterministic embedding provider. No live store, model, network,
index refresh or configuration change is involved. Search thresholds are unchanged.
The unchanged original regression independently ran RED on the base and GREEN
on the candidate. Existing indexed text still needs an admitted refresh; this
demonstration cannot establish deployment or live retrieval acceptance.

The small terminal output and timing are retained in this directory with the
source. Download `output.log` and `timing.log`, then use util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

This requires terminal replay; no inline video player is claimed.
