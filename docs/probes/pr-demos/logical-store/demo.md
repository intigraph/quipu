# Logical-store source fixture demonstration

Recorded source head: `766722593c68ae5a67c6de563273f44d34dd4b3e`.
Core implementation: `83659efb028f2527752e8337f77f739040c0c420`.
Target: isolated synthetic in-memory stores and a temporary fixture signing key.
No installed service, production data, real credential or routing is used.

The same build compares its unchanged scoped live-fact profile with the new
logical profile. This is a profile comparison, not a separate run of an older
binary: the new API does not exist on base `ed79bf881ec99e100d7fe3b047cb08d315bbdb59`.
The source holds a parameterized stored query over a named dataset. Watch:

- Source query present, scoped baseline query `None`.
- Logical bootstrap returns `[{"value":"closed"}]`.
- A metadata-only update leaves fact head `3 -> 3`; the signed tail returns
  `[{"state":"closed"}]` while journal position advances `0 -> 4`.
- A destination-owned graph survives refusal; interrupted bootstrap has no
  cursor; malformed signed tail retains cursor `0`.

[Readable transcript](transcript.txt), [compressed original recording](output.log.gz)
and [original timing](timing.log)
are retained here as small source-demo artifacts. No executable, key or store
is published. This is an output-only util-linux terminal recording; playback
requires downloading both files and running:

```sh
gzip -dk output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

The transcript normalizes terminal CRLF to LF; the compressed recording retains
the original bytes and timing. Playback was checked locally. No inline video embed is claimed. Reproduce the
actual calls and measured results from a source checkout with:

```sh
bash docs/probes/pr-demos/logical-store/run.sh
```

Crew verification used the HOME-escape guard. The source fixture also asserts
raw table/history equivalence, signature admission, idempotence and later-graph
tailing. This recording demonstrates library behavior only: HTTP producer,
follower service, resource/retention contracts and actual trust/routing admission
remain pending.
