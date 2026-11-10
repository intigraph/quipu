# Read-only snapshot export demonstration

The recording runs both CLIs on one disposable cold SQLite fixture containing
one positive RDF triple. The installed base rejects the new flag; the candidate
exports the triple while fixture bytes, mtime and directory entries stay equal.
Default export returns identical RDF. This is a local CLI demonstration, not a
production export or deployment claim.

Base: clean Quipu 0.16.6 `ed9797c2626b0c1ede4ff5fb38ccfb55e7c4afc6`.
Candidate: source based on `07c6a9bb`, with the read-only export change in this PR.
The final follow-up changes command-specific help and adds controls; the recorded
export implementation is unchanged.

Download [output.log](output.log) and [timing.log](timing.log), then replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Playback was checked with `scriptreplay`. The output log is also a readable
transcript. Reproduce with [run.py](run.py), setting `BASE_BIN` and `CANDIDATE_BIN`
to the two executables. The script only writes its disposable temporary fixture.
The small recording stays with the repository so it survives branch cleanup.
