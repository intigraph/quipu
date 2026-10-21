# Cross-write cardinality demonstration

Writing one value twice in separate requests must not bypass `sh:maxCount 1`.
Watch the second knot and the query that follows it:

- Before: both knots exit zero, and the current query returns `"A"` and `"B"`.
- After: the second knot exits one, and the current query returns only `"A"`.
- In both runs, a separate valid subject is accepted as a positive control.

This is a real terminal recording from two fresh disposable SQLite databases,
not a service deployment or a constructed transcript. The baseline is Quipu
0.14.1 at `a7510ca8ea23605b395f164b8bd56dd3de7c26ac`; the candidate implementation
is `a095717f05910c42d2762ffcddf8e0be18584272` (0.15.0 source build). Exact executable
SHA256 values appear in the recording. Subsequent demo-only additions do not
change the demonstrated implementation.

[Readable transcript](output.log) · [timing log](timing.log) · [reproducer](run.py)

Download the output and timing files together, then replay with util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Replay was checked with `scriptreplay`; no embedded player is assumed. The small
recording stays with this source history; no external artifact host is required.

The timed stream is unchanged; only the recorder trailer's extra final newline
is normalized for source formatting.

To repeat with Python 3.11+ and two locally available executables:

```sh
BEFORE_QUIPU=/path/to/before AFTER_QUIPU=/path/to/candidate python3 run.py
```

The reproducer always creates temporary databases and checks the actual exit
statuses and query values. It uses no running server, credentials, provider,
or production database. It demonstrates this simple predicate-path case;
inverse paths, graph isolation, atomic replacement, blank nodes and concurrent
writers are covered separately by `tests/write_cardinality.rs`.
