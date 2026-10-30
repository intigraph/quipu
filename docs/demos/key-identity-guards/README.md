# Allocated key identity guards

The [terminal recording](output.log.gz) and [timing file](timing.log) compare the
same synthetic in-memory fixture against base
`ed79bf881ec99e100d7fe3b047cb08d315bbdb59` and source candidate
`6f3c796142f570606d541821bcda8b8afc550756`.

Watch the port replacement and hash-to-lexical shape edit: base accepts both;
the candidate refuses both. An identical key assertion remains accepted.
The candidate also proves that refusal leaves the transaction head unchanged
and that the retained reference verifies without a write.

Both executables were built without default features. The base archive received
only the same example fixture, not the guard implementation. The candidate was
built from the committed source checkpoint above. The final PR refreshes this
small recording and caption without changing the demonstrated fixture. No network or installed database participates.

Reproduce the candidate fixture from the candidate checkout:

```sh
just demo key-identity-guards
```

For the same comparison with already built base and candidate fixture executables:

```sh
python3 docs/demos/key-identity-guards/run.py /absolute/path/to/base /absolute/path/to/candidate --candidate-revision 6f3c796142f570606d541821bcda8b8afc550756
```

Download the compressed output and timing file, then replay with util-linux
`scriptreplay`:

```sh
gzip -dk output.log.gz
scriptreplay --log-out output.log --log-timing timing.log
```

The [transcript](transcript.txt) remains readable without a replay tool.
The compressed recording preserves the original terminal bytes and timing. The small sanitized
recording is retained with the repository's existing demonstration artifacts.
The separate fork-operator and two-connection allocation-race regressions are
covered by the library tests; this recording demonstrates the two write refusals.
