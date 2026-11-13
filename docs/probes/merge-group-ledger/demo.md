# Combined-tree ledger admission demo

This terminal recording compares base
`bc8d91fb46b87ab54c8aad32d5987c5dfc402300` (no strict queue ledger job) with
this proposal's `scripts/ci/check-merge-group-ledger.py`. The target is a local
synthetic Git merge group, not an installed service or hosted queue.

Watch the correct measured tree pass, then the same stamp refuse after code
from another entry changes `src/ahead.rs`. No ledger is re-derived or committed
by the validator. The base observation comes from the base workflow source;
GitHub cannot execute a missing job locally.

[Recording output](output.log) and [timing](output.timing) are retained here as
small review artifacts. Download both files, then replay with
`scriptreplay --log-out output.log --log-timing output.timing`. This is a terminal
recording, not inline video. Reproduce with
`python3 docs/probes/merge-group-ledger/demo.py` or run all controls with
`just merge-queue test`. The output log is also a readable transcript.

The demonstrated source is the proposal containing this caption; fixture SHAs
are generated locally and vary per replay. Required-check configuration and a
real queue-run acceptance remain operator steps before queue enablement.
