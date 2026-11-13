# Documentation prerequisite demo

Trigger: `just check` with `mdbook` present and `mdbook-mermaid` absent.
Before: the full hook suite starts before its book hook diagnoses the missing
plugin. After: the shared preflight refuses immediately and explains Cargo's
bin-directory PATH and the supported install command.

This is an isolated source fixture: executable stubs replace mdbook and
pre-commit; the latter calls the real book-hook script. It demonstrates failure
ordering and diagnostics, not a complete hook suite or a macOS installation.
The separate native controls also check unusable executables and retained build
and hook commands. Actual Linux book rendering was checked separately.

Base: `d649bcd1bb356175b97804b30e3f3e4609cd267f`.
Candidate implementation: `1daf255390a53cd75b03edfa368c604df2bf10d9`.
The recording-only follow-up does not change that implementation.

Reproduce from the repository root:

```bash
python3 docs/probes/pr-demos/doc-tool-preflight/run.py
```

[Readable terminal recording](output.log) and [timing data](timing.log) are
retained here as small review artifacts. Download both into one directory and
replay with util-linux `scriptreplay` (no inline player is claimed):

```bash
scriptreplay --log-out output.log --log-timing timing.log
```
