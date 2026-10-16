# Optional query admission timing

The existing `query_observation_start` and `query_observation_end` log records
report `dispatch_ms`, `store_wait_ms`, and `store_held_ms`. `dispatch_ms` begins
before async admission and ends when the blocking query closure starts. A large
value alone cannot distinguish a queued admission permit from blocking-task
scheduling or deep-stack thread startup.

The following startup setting adds a permit-acquisition observation:

```toml
[quipu.server]
log_query_admission = true
```

It defaults to false, including configurations that omit the key. This change
does not enable it in an installation. Changing the setting follows the normal
configuration and restart process; it is not a hot toggle.

With the setting disabled, the observation record shape is unchanged and no
shared timing allocation is created. With it enabled, the same two records
include `admission_ms`. There are no additional events, metric labels, query
texts, or request bodies.

`admission_ms` measures elapsed milliseconds from the observation's existing
origin until the admission permit is granted. It includes the small pre-acquire
handler/helper interval. The timestamp is recorded immediately after successful
async acquisition, before dispatching blocking work, for both ordinary and
reserved-point query paths. Admission policy, capacity, cancellation, request
budgets, and point-read eligibility are unchanged.

The start record contains null. The end record also contains null when no
permit was observed, including cancellation while queued or a closed admission
semaphore. Missing observation is never converted to zero. A granted permit
remains observable if its HTTP future later disappears while blocking work
continues. The timestamp is single-assignment and shared only with that query.

When both values are present, `dispatch_ms - admission_ms` gives the remaining
post-admission interval. This includes blocking scheduling and deep-stack startup;
it is not a CPU measurement or an identification of a particular queue. The
existing process/request identity joins still apply.

## Private controls

Native tests use an isolated semaphore and a one-thread blocking pool. Holding
the permit and occupying the blocking pool produce similar combined delays,
but their acquisition timestamps separate the two stages. Further controls
cover default-off output, null versus granted acquisition, single assignment,
cancelled and closed admission, and a real in-memory HTTP result. Existing
point-read deadlines and abandoned-worker observation tests remain in place.

These controls validate the measurement seam. They do not attribute a production
tail or demonstrate that a deployment has enabled this option.

Rollback is to leave or restore `log_query_admission = false` through normal
configuration delivery, or revert the introducing source commit through the
normal reviewed deployment lane. Neither rollback changes query admission.
