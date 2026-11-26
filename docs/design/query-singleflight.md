# Overlapping identical query reads

`[quipu.server] query_singleflight = true` opts one serving process into bounded
sharing of concurrent local `/query` results. Default is false. This is source
work; rollout and natural-load acceptance are separate.

The key hashes the complete JSON input (including graph, temporal and rendering
options), transport, content negotiation, substring contract, operation, and
authority headers plus the verified principal/credential identity. Only outer query whitespace is trimmed. Whitespace inside a
literal is significant. Keys and metrics do not expose query text or credentials.
Remote SERVICE/federated reads and non-deterministic functions are excluded
conservatively, as are execution profiles and stores without a file-backed
generation probe. Read-only mounted stores, including active frozen packs,
require generation proof for every mounted file.

Mounted-source probes use dedicated read-only connections. The key also hashes
each source alias, SQLite data version and transaction head. Before evaluation,
the leader pins main and all mounted source snapshots and compares their heads
with the admission proof. These are per-file observations, not a claim of an
atomic cross-file transaction. A changed active frozen-pack registry, missing
mount, uncertain probe or source-file replacement disables sharing. File identity
is currently qualified on Unix; other platforms preserve ordinary composed reads.
Startup reports `ACTIVE` only when the complete initial proof succeeds.

Each process owns at most32 flights, each with at most16 live followers.
Successful responses up to1MiB share immutable bytes; larger/streaming responses
use the existing path. A dedicated read-only connection checks both SQLite
`data_version` and the indexed latest transaction before joining. Inside the
leader's actual pinned SQLite read transaction, the same generation is verified
again before evaluation. A generation change before that pin or an uncertain
probe prevents sharing. A write after the pin may proceed: existing followers
at the old watermark share the pinned answer, while a newer request observes a
different generation and cannot join that flight. The marker has
busy timeout0, a64KiB page-cache target and mmap disabled. Probes queue asynchronously for at most5ms within the caller's remaining budget, then use try-lock;
no evaluator work or scans run there. Marker calls are small synchronous reads;
their latency and overhead need measurement before enablement.

The flight leaves the registry before notification. A later request executes
again even if no write occurred: there is no completed-result cache. A failed,
cancelled or oversized leader wakes its followers without an answer; they use
the original path within their own remaining budget. A follower timeout does
not cancel the leader. The follower wait uses the configured request budget,
or a bounded query-budget wait (30s ceiling when no request budget is set).

Sharing is per process, not across reader workers. Existing process routing,
read admission, reserved point readers and heavy-operation limits continue to
admit actual executions. Per-request HTTP accounting remains in middleware;
actual store time belongs to the leader. Fixed-name coalesced and leader-duration
metrics distinguish saved executions from requests. Query observation captures
represent executions; followers retain result-size/shape response extensions.

Source controls cover an eight-request disabled/enabled burst, exact result
parity, no completed-result reuse, differing text/scope/authority/format,
writes before and after the actual snapshot pin, mounted-source commits and file
replacement, missing archive mounts, a frozen-pack duplicate burst, uncertain
generation, bounds, cancellation and independent
follower timeout. The ignored `identical_read_burst_ab` test runs a finite local
fixture A/B and prints executed count and p95; it is not a production benchmark.
Natural busy-window 408/request acceptance remains open after installation.
