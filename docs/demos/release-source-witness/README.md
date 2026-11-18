# Release source-byte witnesses

[Output-only terminal recording/transcript](output.log) · [Timing](timing.log) ·
[Reproducer](probe.py)

The probe replays the conflicting record from each exact historical three-way
fixture: `1870c8b5`, `6e55fd91`, and `e2d7b6e9` (base and parent revisions are
retained in `scripts/fixtures/repository-knowledge-merges.json`). Before migration,
each returns one Git conflict. Applying the candidate registry's two explicit
release-byte modes makes each merge cleanly; independent summary edits still
conflict. In a separate private source copy, edits to both approved files change
the actual renderer's emitted hash without changing registry bytes or review dates.

Repeat from the repository root:

```sh
python3 -B docs/demos/release-source-witness/probe.py
```

Replay from this directory with util-linux `scriptreplay`:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

This is a synthetic source demonstration, not installation or live index coverage.
Lookup labels are explicit fixture controls. Build-derived hashes prove published
byte identity, not renewed summary review. All other records retain strict hashes.
The small sanitized recording is retained with the PR; inline video is not claimed.
