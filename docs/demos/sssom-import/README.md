# Pending SSSOM import

The same external TSV claims both authorship and acceptance. The base refuses
the command; the candidate preserves those fields as evidence and writes a
pending proposal. Watch `local_acceptance: pending`, the absent database, and
the refusal to replace the source file.

Base: `ce86ba4cc1b5bf0bcf98a67c2de8a6e1f075946b` (installed CLI binary).
Candidate: `5ee36a194574c9e439f16498228d01179c9a69b9` (clean source build).
Both run against the same isolated synthetic input from [probe.sh](probe.sh).
This demonstrates CLI import, not deployed behavior or merge integration.

The real terminal recording is [output.log.gz](output.log.gz), with
[timing.log](timing.log) and a readable [transcript](transcript.txt).
Playback requires downloading these files and a terminal with `scriptreplay`:

```sh
gzip -dc output.log.gz > output.log
scriptreplay --log-out output.log --log-timing timing.log
```

Repeat with binaries built at the revisions above:

```sh
BEFORE_QUIPU=/absolute/path/to/base-quipu \
AFTER_QUIPU=/absolute/path/to/candidate-quipu bash probe.sh
```

The small recording and caption are retained in this repository alongside the
existing terminal demos. It has no inline web player. Later caption-only changes
do not alter the demonstrated source. Native/share/Git adapters, split reminting,
and independent SSSOM validator qualification remain outside this slice.
