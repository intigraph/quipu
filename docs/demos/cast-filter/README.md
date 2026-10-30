# Cast-bound filter planning

A constructor-form date bound caused the filter driver to reject the selection
and hydrate every item before filtering. This offline recording compares engine
base `ed79bf881ec99e100d7fe3b047cb08d315bbdb59` with the engine patch in
`eb738e7b61bbe0cfdfd5cfac8d543cb7a271f1e8` on the same synthetic fixture and query.
The profiler is diagnostic instrumentation, not an installed service.

Watch the SQL statement count fall from 75,172 to 912 while all 39 sorted result
rows stay identical. These are debug-build fixture observations, not production
p95 measurements. A separate 75,000-item pair retained identical 103-row answers:
12,105 ms / 751,522 statements before, 1,183 ms / 9,012 statements after.
No external database, server, live probe or timeout change is
involved.

Run the public fixture with `just demo cast-filter`. The source is
`examples/cast_filter_profile.rs` and its input is
`examples/cast-filter-profile.rq`. The recording uses the retained query, with
only metrics and an answer fingerprint exposed; the public reproducer replaces
the deployment-specific named-graph IRIs with `example.org` scopes. All other
query structure is preserved. The retained query SHA256 is
`6d158a0c0087488cc8878b364986a0637f2ebde2e0fd9486ba8f1a4b5e435b51`.

For the base comparison, copy only that example, its query and the `demo
cast-filter` recipe into an owned checkout of the base; leave its engine code
unchanged. Build and retain separate profile executables for each engine, then
run `python3 run.py BASE_EXECUTABLE CANDIDATE_EXECUTABLE QUERY_FILE`. The script
checks nonempty answers, exact row parity and a substantial statement reduction.
Build metadata in the original archive-based diagnostic binaries is unavailable;
the engine source, build logs and executable digests are retained separately.

Download `output.log.gz` and `timing.log`, decompress the first, then replay:

```sh
gunzip -k output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

Playback was verified with util-linux `scriptreplay`. This terminal recording
requires downloading the files.
`transcript.txt` is its readable fallback. These small sanitized review artifacts
are retained with the source; deployment and runtime acceptance remain separate.
