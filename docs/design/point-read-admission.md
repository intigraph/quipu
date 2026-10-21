# Opt-in admission for point reads

A bounded request budget prevents an expensive backlog from running forever.
It does not make a small request available while the backlog occupies every
reader. In a private 500-triple fixture, 90 concurrent two-hop COUNT queries
left 87 requests in flight when a known-true and known-false point ASK arrived.
Both answers were correct, but each took 9.23 seconds. Health continued to
answer immediately. This measured latency failure does not reproduce the
historical client timeout.

The experimental configuration reserves one existing connection and one async
admission permit for a conservative point-query profile:

```toml
[quipu.server]
read_pool_size = 4
reserve_point_reads = true

[quipu.search]
request_timeout_ms = 10000
```

It is off by default. It requires a positive admission budget and at least
two usable pooled readers. With four readers, ordinary requests have three
connections and three admission permits; eligible requests share the fourth.
Neither lane borrows the other's connection or permit. No new connection is
opened. An unavailable pool (including in-memory or unsupported external
vector backends) and a one-reader pool retain the ordinary path and announce
that the reservation is unavailable. A zero request budget with the option
enabled refuses startup.

The eligible profile is deliberately narrow:

- At most 1024 ASCII bytes, with ASK, optional WHERE, and exactly one
  triple containing three absolute IRI terms.
- No variable, path, join, filter, prefix declaration, dataset clause or
  additional graph pattern.
- No RDF, RDFS or OWL vocabulary predicate.
- Only query and optional boolean verbose / protocol metadata fields.
  Graph selection, temporal coordinates, labels, federation and caller
  priority fields use ordinary admission.

This recognizer only chooses scheduling. The original query still passes
through the native parser, evaluator, label policy and response formatting.
Unsupported spellings use ordinary admission; they do not acquire a new
meaning. The reserved reader performs the same attachment synchronization as
other pooled readers.

Both lanes are FIFO and bounded in active work. A point flood cannot take an
ordinary permit or connection. A queued point request spends its original
request budget and can return the existing HTTP 408 without reaching the
blocking pool. Once blocking work starts, its permit stays with that work even
if the client disconnects. Expensive requests keep their existing deadlines
and honest timeout responses.

This removes the ordinary backlog from the point-read queue. It does not
promise a universal one-second service bound: disk stalls, attachment
synchronization, policy work and an earlier point request can still delay a
read. The one-second target is an acceptance metric for the controlled overload
fixture, with confirmed concurrent expensive work and positive and negative
truth controls. A fixture whose load drains before the probe is
nondiscriminating.

Rollback is to disable reserve_point_reads through the normal configuration
lane. All readers and permits then return to ordinary service. Source delivery
does not authorize production activation; that requires its own reviewed
configuration and observed acceptance.
