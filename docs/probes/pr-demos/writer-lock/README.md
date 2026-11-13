# Native writer ownership

Opening the same file twice concurrently used to succeed. The native binding
now refuses the second writer; closing the owner permits a new open.

The same [reproducer](../../../../examples/writer_lock_demo.rs) ran against
base `587de2083c9c54423dc8f5acd6f06ef8622608be` and implementation
`cf06b839c6872b0bb7cd26358290d02e6fe2c8c8`. The example was carried as an
untracked fixture on both revisions; the library sources were unchanged.
Each run creates its own disposable database. No served database is accessed.

| Observation | Base | Implementation |
| --- | --- | --- |
| First writer | OPENED | OPENED |
| Same-file second writer | OPENED | REFUSED |
| Open after owner closes | OPENED | OPENED |

Run the candidate example with `just demo writer-lock`. The example's
`--expect-open` argument checks the historical result when carried to the base;
the default checks refusal. Unexpected outcomes fail the command.

These are actual terminal recordings made with util-linux `script`, with
readable transcripts. Download each pair and replay with `scriptreplay`:

- Before: [output](base.log), [timing](base.timing).
- After: [output](head.log), [timing](head.timing).

```sh
scriptreplay --log-out base.log --log-timing base.timing --divisor 1000
scriptreplay --log-out head.log --log-timing head.timing --divisor 1000
```

Replay was checked locally. These files are retained in the repository with
this caption; no inline video player is assumed. Build stderr is outside the
recordings. Both commands exited zero and their HOME guards were clean.

The demo proves binding contention on Linux. The separate native integration
test exercises a real server, mutual server/binding refusal and release after
abrupt server exit. This does not prove deployment or protect against older
servers, raw SQLite, or another writer that does not take the advisory lock.
Replacing or unlinking an open database remains unsupported.
