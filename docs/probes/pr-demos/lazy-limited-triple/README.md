# Limited triple scan demonstration

The private native-evaluator probe returns one subject from 10,000 synthetic
facts. Its measured SQLite work changes from 140,078 instructions to 30.
The baseline fails the chosen ceiling of 10,000 instructions; the candidate
passes, with a positive instruction counter and one-row control.

```sh
just test --features full --lib explicit_limit_one_avoids_grouping_the_entire_graph -- --nocapture
```

The recorded display replays these measured results without executing a query:

```sh
python3 docs/probes/pr-demos/lazy-limited-triple/demo.py
```

See [output.log](output.log) and [timing.log](timing.log) for the terminal replay.
This demonstrates private evaluator work, not production latency, admission,
network behavior or incident recovery.
