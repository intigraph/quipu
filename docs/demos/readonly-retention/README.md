# Read-only retention regression

This readable transcript shows actual isolated server runs with eligible old
events and superseded vectors. On base `733b02f0`, both configured read-only
checks fail because rows disappear; writable and zero-retention controls pass.
With the retention startup guard, all five checks pass. Disabling the guard
reintroduces both failures; restoring it passes again. This is a transcript,
not a terminal recording; the original run did not capture replay timings.

Run `just test --release --locked --features full --test readonly_retention_server
--jobs 2 -- --test-threads=1` as one command. To test a separately built baseline,
set `L64_REGRESSION_SERVER` to its server executable. Fixtures use temporary
local stores and loopback listeners; no installed service is exercised.

The transcript is retained with this regression in the repository. It proves
startup behavior for these fixtures, not production deployment or a soak.
