# Offline competency lint demonstration

The same synthetic pendant is rejected as an unknown command by the installed
CLI, accepted by the candidate, then rejected by question ID after replacing
`ASK` with `BROKEN`. Lint parses the write template without executing it.

- Before: installed clean `5d2fe613acd7cd2c172b23e3af8b2a5bddd612bf`.
- After: clean candidate `c4bc05dde5fffc2c438d962fd6e38c83bdb6ac5a`.
- Target: local CLI with disposable synthetic files; no service deployment.
- Input and repeat command: [reproduce.sh](reproduce.sh), using
  `QUIPU_BASE_BIN` and `QUIPU_CANDIDATE_BIN` to select the binaries.

[Terminal recording](terminal.output) and [timing](terminal.timing) are retained
with this small repository demo, following the existing CLI demo convention.
They require downloading for replay; no inline video player is assumed:

```sh
scriptreplay --log-out terminal.output --log-timing terminal.timing
```

The output file is also a readable transcript. Replay was checked with the
installed util-linux `scriptreplay`; both raw logs and replay output were
inspected. This proves syntax lint and its rejection behavior, not query results,
shape validity, command authorization, or share/import integration. Later commits
that add only these demonstration files preserve the demonstrated source.
