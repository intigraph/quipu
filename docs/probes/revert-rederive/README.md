# Code-changing revert conformance trigger

A private Git fixture changes two source files on main, then reverts one to its
older measured version. Watch PR provenance change from fresh (0) to stale (1),
which requests the existing conformance re-derive. Strict provenance remains
stale, unknown revisions remain unverified (2), and a docs-only PR stays fresh.

Base: `91aae714475b4eed4be43b91da0930c5a89accc2`.
Candidate source: `e7fb24ee` (the accompanying recording changes no behavior).
Run from the repository root:

```sh
bash docs/probes/revert-rederive/run-demo.sh
```

The small output-only terminal recording is retained here with the source:
[transcript](output.log), [timing](timing.log). Download both and replay using:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

This is a local library demonstration, not a hosted Actions run or deployment.
