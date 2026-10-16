# Portable MCP workflow demo

Trigger: the same `quipu_events` call with `since:0, limit:1` on a fresh
private store, after writing one fixture fact. Before: unknown tool. After:
a bounded event batch with `next_offset`. Both runs first prove a query
returns the fixture (`count:1`), so an empty or inaccessible store cannot
masquerade as coverage.

Base: clean `5fd07d21dfbec0f29fce54af86ae4fb8f02739a0`, version0.14.1.
Candidate: that base plus this proposal, built with `--features full`;
its build metadata honestly reports `git_dirty:true`. This is local candidate
behavior, not an installed-service or composed-history acceptance claim.
The code behavior demonstrated is unchanged by the subsequent documentation
and fixture-contract corrections.

[Terminal recording](output.log) and [timing](timing.log) are small retained
repository artifacts. Download both, then replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

This is a terminal recording, not an inline video. The output log is also a
readable transcript. Reproduce against two retained binaries:

```sh
python3 scripts/demo-mcp-workflows.py /path/to/base/quipu-server /path/to/head/quipu-server
```

Both inputs use the same stdio MCP protocol and disposable local stores.
No HTTP listener, real graph write, network credential or service restart is
used. Fixture paths and binary paths are omitted from the recorded output.
