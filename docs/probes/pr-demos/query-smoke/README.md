# Query smoke CLI exit behavior

Run `python3 docs/probes/pr-demos/query-smoke/run.py` from the repository.
It invokes the real CLI entry point with controlled healthy, HTTP408 and empty
query responses on base65db3e3c and the candidate source identified by SHA256.
Before: all three exit0. After: timeout and empty exit1; healthy remains0.
No network or installed-service behavior is demonstrated.

[Recording output](output.log) and [timing](timing.log) are retained beside this
caption in the existing PR demo tree. Download both and use
`scriptreplay --log-out output.log --log-timing timing.log` for terminal replay;
inline playback is not claimed. The output is also a readable transcript.
The PR's exact head identifies the candidate containing this demo. Keep these
small artifacts with the source; re-record if smoke behavior changes.
