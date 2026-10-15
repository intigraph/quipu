# Bounded ORDER keys

The same 2,000-row VALUES query with four 16,385-byte literal ORDER keys previously returned 2,000 rows while retaining about 127 MiB above the process baseline. Watch the corrected private in-memory fixture refuse with an explicit retained-key budget error, then sort an ordinary numeric control correctly. No result is silently capped or ORDER key suppressed.

This delta recording replays the genuine independent measurement from candidate `94db3d948fb4dcb6ceb6149ac809a1f3be0f6f9e`, then calls corrected source `76bbbbc8908212d876f87ccba8f5cf2776c27cbd`. The original query/header recording remains separate. The new bound is 65,536 entries and 8 MiB of accounted retained keys and sort bookkeeping; it is not a total query RSS ceiling. A single evaluated expression is transient before accounting. Resolved IRI strings share cached storage, and stable sorting polls the query deadline without reevaluating keys.

A separate matched Linux process-peak measurement used the same 74,492-byte query under 512 MiB / zero swap / one CPU / 20 seconds: refusal, peak delta 9,992 KiB. These private measurements do not establish installed behavior or concurrent-service memory bounds.

[Recorded output](output.log) · [Timing](timing.log) · [Retained before](before.txt)

Replay downloaded files with `scriptreplay --log-out output.log --log-timing timing.log`. Reproduce the fixture from the repository root with `bash docs/probes/pr-demos/order-key-budget/run.sh`. No network, credentials or production store is used.
