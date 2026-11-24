# Distinct validation context

A synthetic SQLite fixture repeats one metadata triple in 100,002 assertion
rows. The old reader charges its 100,001-row overflow bound; the candidate SQL
read directly from `src/store/shacl_poststate.rs` returns one distinct triple.
A control with 100,001 distinct triples still returns the overflow sentinel.
Predicate filtering selects a small context even when other predicates are large.

The source change is commit `0279b1bd884897386bff80b51ca0facf68056ab1`,
against base `6c023491d26f5f8d002b324e6108e728f9e7aa0f`.
This is a query-level demonstration on an isolated fixture, not a deployed
service or a compiled Rust write-lane result. The Rust regression separately
checks a successful update, a real `maxCount` refusal and the unchanged cap.

Repeat from the repository root:

```sh
python3 docs/demos/distinct-validation-context/run.py
```

The [output](output.log) and [timing](timing.log) are a replayable terminal
recording retained here with its caption. Replay after downloading both files:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The readable output is also a transcript fallback; no inline playback is assumed.
