# Transition keys require human-root endorsement

The [recording](output.log.gz), [timing](timing.log) and
[transcript](transcript.txt) run the same in-memory library probe on base
`d707ec62fe105f3296143321e681aec8daa46f28` and the repaired gate.

Before: an ordinary graph writer's registration is enough to admit its signed
transition, in ROOT or a named identity graph. After: both self-enrollment cases
are refused. A real human-root-signed endorsement still admits the transition;
the disabled legacy flag control remains admitted. No live store, key ceremony,
registration, transition or configuration change participates.

The probe is added by this change. For the base run, only the same example and
its `just demo` recipe were overlaid on the actual base library sources. The
candidate was built in a private source fixture. Its gate source SHA256 is
`bed1c7002a953461542a08c163d671013ea0e1a031f12b2aeb0a01e5e9a9f57a`; the shared probe SHA256 is
`80de51c9c1335de473203091dbdfb4c654690b6593197aea96739f3f9f88e303`. These are source qualifications, not deployed-version receipts.

Run the candidate:

```sh
just demo transition-trust-root
```

For a comparison, build `examples/transition_trust_root.rs` against the base and
candidate libraries in private checkouts, then pass the two owned executables to
[`probe.sh`](probe.sh) as `TRANSITION_BASE_PROBE` and `TRANSITION_HEAD_PROBE`.
It checks all four actual outcomes for each executable. Crew qualification uses
the normal HOME guard; compiler and guard logs remain private.

Download the recording and timing files before playback:

```sh
gzip -dk output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

The compressed recording retains its original bytes; the readable transcript
normalizes terminal line endings. No inline player is promised. The
[migration runbook](../../runbooks/trust-root-ceremony.md#shuttle-transition-registrations-aegis-udr9)
explains why existing named-graph keys need explicit ROOT migration and human
endorsement before enforced transitions. Flags remain unchanged.
