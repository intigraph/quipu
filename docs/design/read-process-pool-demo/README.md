# Scratch reader-process demonstration

Recorded on candidate aac459e8b6e21b28f1c1bd5a83f016ec1e2ee1ad, based on
be9f52b40b5801450a1c323cb8eecb92b3bf96ca, using owned binaries built with
`just --command cargo build --features full --bin quipu --bin quipu-server`.
The fixture uses a tiny local SQLite graph and never contacts a deployed service.

The existing default keeps readers in the writer process. With two opt-in reader
processes and a 256 MiB budget, watch two distinct child PIDs serve the same true
ASK. Stopping one child leaves the writer and other child responsive (38 ms in
this run). Killing a reader returns 503 rather than falling back to the writer;
killing the parent stops surviving children. The fixture also checks primary
ingress counting, authorization, peer spoofing, occupied-port refusal, and a
sampled three-process PSS peak of 69.5 MB. This is not a production latency result
or a guarantee about every possible query's memory use.

Reproduce from the repository root with matching owned binaries:

```bash
just read-process-test "$SERVER_COPY" "$CLI_COPY"
```

[output.txt](output.txt) is the real readable transcript; download it with
[timing.txt](timing.txt) and replay using
`scriptreplay --log-out output.txt --log-timing timing.txt`. These small sanitized
recordings stay beside the design for review. Playback requires a terminal;
there is no browser video embed. Source delivery leaves the option disabled.
