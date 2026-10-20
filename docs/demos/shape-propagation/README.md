# Shape propagation warning audit

This is a replayable terminal recording over a synthetic source fixture, not an
installed service. A new class/property/NodeShape has no downstream evidence:
the audit reports three unresolved entities. Adding exact revision-scoped CQ
evidence resolves those three entries. The mode remains WARN and the demo makes
zero writes. This is not backfill acceptance or production enforcement.

The previous base, `43a344b5c13aa8cf5beaf5448522b6d84014f894`, has no propagation
audit command. Both fixture states therefore use the candidate auditor; this is
an evidence-coverage demonstration, not a claimed base/head runtime comparison.
The PR caption records the demonstrated source head.

```sh
uv run --no-project --with rdflib==7.6.0 python docs/demos/shape-propagation/reproduce.py
scriptreplay --log-out docs/demos/shape-propagation/output.log --log-timing docs/demos/shape-propagation/timing.log
```

The [recording](output.log) and [timing](timing.log) require downloading and a
terminal replay tool; no inline video playback is promised. Playback was checked
with util-linux `scriptreplay`. [Plain JSON output](output.json) is the readable
fallback. These small sanitized artifacts follow the repository's demo-directory
retention practice and remain with the source.

## Scheduled shallow checkout regression

A scheduled event has no `before` or pull-request base. At source commit
`587451b5d63891bccb66edca6d1b36bbfd97f77d`, a real depth-one checkout tries
`HEAD^` before fetching its parent and exits 128. The full-history control passes.
The corrected source fetches history before resolving the fallback parent; both
checkouts then return the exact expected range. Explicit event bases and failed
fetches still fail closed rather than silently choosing another range.

[Terminal recording](shallow-parent-output.log) and
[timing](shallow-parent-timing.log) show the same isolated two-commit fixture
before and after. The recording identifies the base commit and candidate script
SHA256; the PR identifies the containing head commit. This is source behavior,
not a hosted scheduled-run or deployment claim. The expected baseline Git error
is retained in the recording.

Reproduce with Git and the existing pinned RDF dependency available:

```sh
uv run --no-project --with rdflib==7.6.0 python docs/demos/shape-propagation/shallow-parent.py
```

Download both recording files and replay from this directory:

```sh
scriptreplay --log-out shallow-parent-output.log --log-timing shallow-parent-timing.log
```

The small output-only recording is retained with the existing repository demos.
It is a terminal replay, not an embedded video. All Git remotes used by the
fixture are temporary local repositories; no production graph or service is used.
