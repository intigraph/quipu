# Cooperative heavy read admission

The server can cap declared `owner-rows` queries before they take ordinary
read capacity. The default `heavy_read_operations = []` leaves scheduling
unchanged. Configuration lives in `[quipu.server]`; enabling it is a separate
deployment decision, not a consequence of landing the source.

When enabled with `heavy_read_operations = ["owner-rows"]`, at most
`heavy_read_permits` of those queries can occupy ordinary readers. The cap is
acquired before shared read admission. A waiting heavy query cannot reserve
a shared reader, leaving `N - K` ordinary readers available to other work.
The server validates `0 < K < N` against its actual usable ordinary pool after
point reservation. Unknown or duplicate operations, worker-process serving
and unavailable reserves refuse startup. No connection is added.

The operation uses the same finite header parser as request completion logs.
Absent, unknown or duplicate declarations do not match. This is cooperative
scheduling metadata, not authentication, query cost estimation or a privilege.
An undeclared expensive query still uses ordinary admission and can saturate
it. Declaring a listed operation cannot gain access to the point reserve: it
uses the capped ordinary path even when its text fits the point grammar.
The query evaluator, graph policy, row limits and text remain unchanged.

One original request deadline covers class waiting, shared waiting and query
execution. Cancellation while waiting returns capacity without starting
blocking work. Once dispatched, both permits move into the blocking closure;
canceling an HTTP future cannot release them while that work is alive. Active
read telemetry continues to describe actual blocking work. Waiting for the
new class cap is not a new telemetry lane or operation metric label.

Native controls exercise class-before-shared order, continued cheap progress,
the two deadline waits, cancellation before dispatch and permit ownership
after cancellation of a running query. Configuration controls use the actual
project TOML loader and validate defaults and unusable reserves.

These controls do not establish served fairness or production causality. A
matched finite private-copy experiment using actual owner-row texts is still
required, with exact result and positive cheap controls, owner progress,
deadlines, resource bounds and a discriminating class-bypass fault. Production
capture, configuration and activation remain separate gates.
