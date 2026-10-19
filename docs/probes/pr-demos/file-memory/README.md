# File-memory CLI demo

The same synthetic commit request is rejected by base `6019c8d2f8c42123bfaad938583d0b4045fceffe` (exit 1); candidate `9db6f238d335175b1b42a48999ee11ff094d53a7` accepts it, returns an identical receipt on replay, and exports the exact 27 UTF-8 bytes including CRLF. The base artifact is SHA256 `07f8c4bb051c6b5bd5339e82c34f3cc8a7904f6a630a4ab1d08a7095b8fc4da0`; candidate artifact is `5040146a77f802a0f832e8a54365812f9e8bfe0dd5b8fbd6076224486f998124`.

This is a replayable terminal recording, retained here with a readable transcript, using isolated temporary local databases. It demonstrates explicit filesystem authority, not remote owner identity, installed service behavior or native harness uptake. Source changes after the recorded candidate must preserve this behavior or replace the recording.

Run the demo with explicit compiled base and candidate CLI paths:

```sh
python3 docs/probes/pr-demos/file-memory/demo.py /path/to/base-quipu /path/to/head-quipu
```

Replay the captured output locally with util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The recording completes successfully and its raw transcript contains synthetic data only. No inline video playback is claimed. Files are small source-controlled review artifacts and remain with the repository history.
