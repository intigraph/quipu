# Ownership handoff model: retirement is a prerequisite

The [terminal recording](output.log.gz), [timing file](timing.log) and
[readable transcript](transcript.txt) show the actual in-memory library model at
`f9c7da76f5a72f903e4d47743e9c246665f0508c`.

Watch `premature_receipt` and `admitted_owners`. The candidate refuses retirement
publication before the local barrier. Once the ordinary modeled handoff finishes,
the held A handle refuses and B can commit: one owner. The same source with only
[`Model::require_retired` removed](retirement-mutant.patch) records a premature
witness retirement and publishes the receipt while A still runs. It admits B
with A still writable: **two owners**, followed by an actual failed assertion
(exit 101). The probe restores the exact candidate file and repeats the positive
control, ending with one owner again.

This is a source counterfactual, not a claim that main's base
`21349300281dadfbf40c78257b46eccc1498f6f2` already had this model. No actual Store,
SQLite commit fence, signing key, authority profile, host binding or network
service participates. Oracle observations are assumptions, not credentials.
The recording does not demonstrate physical old-A fencing or runtime acceptance.
The [model contract](../../design/ownership-handoff-model.md) states those limits.

The recording preserves original output bytes and timing. Playback was verified
with util-linux `scriptreplay`. Download both artifacts, then:

```sh
gzip -dk output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

Use `--divisor 10` to shorten the real compilation pauses. The transcript is a
readable rendering of the same output, not a constructed success transcript.
These small sanitized artifacts follow the repository's existing demo retention.
Compiler stderr and full guard logs remain in the private qualification evidence.

Reproduce the ordinary positive fixture with locally cached Rust dependencies,
`just`, Git and ripgrep:

```sh
just demo ownership-handoff-model
```

For the paired source counterfactual, use an isolated checkout with a clean
`src/ownership_handoff_model/state.rs`. The script temporarily patches that one
file and restores its original bytes even on failure:

```sh
bash docs/demos/ownership-handoff-model/probe.sh --fixture-copy
```

Crew execution uses the normal HOME guard. An unexpected compiler failure is not
accepted as the negative control: the script requires the actual two-owner output
and the specific model assertion before accepting the expected exit 101.
