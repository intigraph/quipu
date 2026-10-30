# Allocated key identity guards

The [terminal recording](output.log) and [timing file](timing.log) compare the
same synthetic in-memory fixture against base
`ed79bf881ec99e100d7fe3b047cb08d315bbdb59` and source candidate
`fee819e5b2a8217fe5f66822d7ed2a91677172da`.

Watch the port replacement and hash-to-lexical shape edit: base accepts both;
the candidate refuses both. An identical key assertion remains accepted.
The candidate also proves that refusal leaves the transaction head unchanged
and that the retained reference verifies without a write.

Both executables were built without default features. The base archive received
only the same example fixture, not the guard implementation. The candidate was
built from the committed source checkpoint above; the final PR also completes
the top-level help summary and adds this small recording and caption. No network or installed database participates.

Reproduce the candidate fixture from the candidate checkout:

```sh
just demo key-identity-guards
```

For the same comparison with already built base and candidate fixture executables:

```sh
python3 docs/demos/key-identity-guards/run.py /absolute/path/to/base /absolute/path/to/candidate --candidate-revision fee819e5b2a8217fe5f66822d7ed2a91677172da
```

Download both recording files and replay with util-linux `scriptreplay`:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The transcript remains readable without a replay tool. The small sanitized
recording is retained with the repository's existing demonstration artifacts.
The separate fork-operator and two-connection allocation-race regressions are
covered by the library tests; this recording demonstrates the two write refusals.
