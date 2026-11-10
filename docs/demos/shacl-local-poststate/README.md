# Value-local SHACL post-state checks

A string is stored before its subject becomes a target of an integer-or-boolean
constraint. Before, two of seven write partitions admit that invalid graph;
after, all seven reject it. The same valid integer still passes every partition.
An unequal equality prefix can still be completed into equal value sets: the
completion passes all seven partitions on both builds.

The recording calls the real native conformance adapter against disposable
in-memory stores. It proves source behavior, not a deployed service. Base:
`8dd5315b7910c37820f3ed19f140457f8b0d6d23`. Exact before/after binary hashes
are printed in the recording; the source change is the accompanying PR.

Run `demo.py` with `SHACL_DEMO_BASE` and `SHACL_DEMO_CANDIDATE` set to independently
built, retained adapter executables. Replay the terminal recording with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The output and timing files are a terminal recording with a readable transcript,
not an inline video. They stay in this repository for the lifetime of the change.
No production API, graph, process, configuration or schema is changed.
