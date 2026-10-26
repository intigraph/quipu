# Master mapping lifecycle demonstration

An isolated in-memory store starts without a mapping. Explicit row acceptance
creates a decided mapping whose unique equivalent-class target is writable.
Loading a replacement pinned shape flags the mapping, makes the target
non-writable, and refuses application with the old pin. The recording shows the
actual receipts and transaction IDs; preparation itself creates no transaction.

Source: `ecf50cb1d227761ffbe11588b53ed0b475673ed4`, based on
`e4ede323402cfcb0068d613d95d3fc4acd5f712e`. This is a new capability: the base
has no compatible mapping API, so the demonstration compares an empty fixture
with its decided and flagged states on the candidate. It proves source behavior,
not an installed service, production grant, or consumer activation.

Repeat the observable behavior from the repository root:

```sh
cargo run --example shape_mapping
```

The small [terminal recording](recording.log) and [timing file](timing.log) are
retained in Git with this caption. Playback was verified with util-linux
`scriptreplay`; this is a terminal recording, not a video or inline animation.
Download both files into the same directory and run:

```sh
scriptreplay --log-out recording.log --log-timing timing.log
```

To make another recording, compile the example, set `C1_DEMO_BINARY` to its
absolute executable path, and use the included [recording script](run-demo.sh)
inside a fresh util-linux `script` pseudo-terminal. The script disables only
that private terminal's newline conversion so the recorded bytes remain valid
under ordinary text tooling. No service, credentials, or network are required.
