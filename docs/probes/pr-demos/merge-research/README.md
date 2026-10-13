# Merge research: paper and real-Dolt controls

The trigger is the same paper artifact, viewed before and after adding the E/F
studies. Base `222274080ee90e10723d4995a9f14172ced582dd` has a 14-page PDF with
neither new section. Demonstrated source `88209e8d` has a 16-page PDF containing
both sections. The recording then runs the actual Dolt 2.1.8 control fixture:
20 synthetic Memories, six true conflict slots, three storage arms, type/query
controls and a checked server stop. It is a source/test fixture demonstration,
not installed service acceptance or an arXiv submission.

Watch the native JSON and typed-row decision counts: both are ten in this small
control run and both have four entirely false native decisions. The typed layout
reduces collateral semantic-slot questions without a native false-decision
reduction. The paper's full counts use twenty template copies per seed, not the
two copies shown here. Full evidence is in `benchmark/memory/results/` and
`benchmark/shapes/results.json`.

[Output recording](output.log.gz) and [timing](timing.log) are retained here with
the research artifact. Metadata and raw output were inspected; the gzip uses no
original filename or timestamp. This is a replayable terminal recording, not an
inline video. Download both and replay:

```bash
scriptreplay --log-out <(gzip -dc output.log.gz) --log-timing timing.log
```

Playback was verified with util-linux `scriptreplay`. To reproduce the recorded
sequence from the repository root, with a fresh authorized local fixture root:

```bash
MERGE_DEMO_BASE=222274080ee90e10723d4995a9f14172ced582dd \
MERGE_DEMO_SCRATCH=/tmp/merge-demo-fixtures bash benchmark/memory/demo.sh
```

Subsequent proposal commits add the recording and its publication-gate entry;
the benchmark code and demonstrated research conclusions are unchanged. The
printed source revision remains the demonstrated revision rather than being
rewritten to imply a later recording.
