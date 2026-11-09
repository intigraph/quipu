# SHACL path post-state admission demo

The old write path accepts all three separately committed subject writes even
though their final inverse-path values violate `sh:class`. The candidate gathers
bounded path context and refuses the write that introduces the violation.
A complete valid document is a positive control; the invalid complete document
still refuses. All stores are disposable and in memory.

Run from this directory with two locally built conformance adapters:

```sh
BEFORE_ADAPTER=/path/to/before AFTER_ADAPTER=/path/to/after python3 run.py
```

`output.log` and `timing.log` retain a terminal demonstration; replay it with
`scriptreplay --log-timing timing.log --log-out output.log`. The binaries use
the development profile with no default features and `dev-tools`; this is
source qualification, not a release or deployment claim.
