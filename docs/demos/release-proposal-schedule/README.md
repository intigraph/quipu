# Daily release proposal configuration demonstration

A main push previously ran both `release` and `release-pr`. The candidate
keeps `release` on push and moves `release-pr` into a separate daily
07:00 UTC workflow. Watch the effective action command, proposal trigger and
ready gate change in the recording.

Target: immutable source configuration, base `d66142d4`, candidate `9c497d70`.
The later recording-only commit does not change those workflow bytes. Run:

```sh
python3 docs/demos/release-proposal-schedule/demo.py
```

The [terminal output](output.log) and [timing file](timing.log) are an output-only
util-linux recording. Download both to the same directory and replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

There is no video embed. The readable output is also the transcript fallback.
The small recording is retained here with the reproducible demonstration; it
contains no credentials or private infrastructure. It proves the effective
source configuration, not a scheduled GitHub execution, release, installation
or merge queue. No workflow was dispatched to produce it.
