# Counters-only capture

The same synthetic library event is emitted by the actual observation path in
three isolated processes. With the new flag false, the existing detailed journal
stores its synthetic query sentinel. With the flag true, only one aggregate
request is persisted, without query evidence. An invalid aggregate directory
reports capture UNKNOWN, leaves the query journal empty and preserves caller
success. The false-mode positive control proves the evidence check can see a
journal containing the sentinel.

This is a source-library demonstration of flag modes, not a running-service or
four-interface deployment certification. The child probe is an existing unit-test
entrypoint. Both modes run the same candidate binary; this is a configuration
comparison, not a recording of an older installed release.

Build the test binary with:

```sh
just test --locked --no-default-features --lib search_outcome --no-run
python3 docs/demos/search-counters-only/demo.py --test-binary PATH_FROM_BUILD_OUTPUT
```

The small terminal recording and real timing are retained beside this caption.
Download `session.log` and `session.time` and replay with:

```sh
scriptreplay --timing session.time --typescript session.log
```

The log is also a readable transcript. Its header identifies the demonstrated
source revision. No live query, credential or household data is used.
