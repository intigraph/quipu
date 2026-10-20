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
