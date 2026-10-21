# CLI help without opening a store

The same `quipu shapes --help` invocation runs in an empty temporary directory
with a fake HOME. Before, it prints `no shapes loaded` and creates
`.bobbin/quipu/quipu.db`. After, it prints command help and creates no database.

The baseline is the clean installed binary from
`0e89d729e254430806853ade78f9a7287de9e921`. The candidate is a source build on
`9b78ab21bbd9b66da785694a95a8c23f59f4fcc9` with `git_dirty=true`; its three
product-source hashes are retained in [source.sha256](source.sha256).
These are scratch CLI observations, not a deployment receipt. The intervening
base changes concern server authentication, outside this help dispatch.

Download [the terminal output](terminal.output) and
[the timing file](terminal.timing), then replay with util-linux:

```sh
scriptreplay --log-out terminal.output --log-timing terminal.timing
```

This is a terminal recording with a readable transcript, not an inline video.
The small recording is retained in this directory. CRLF was normalized to LF;
each recorded event's byte count was adjusted for its removed carriage returns.
The command output and timing intervals are otherwise unchanged.

Reproduce using explicit old and new binaries:

```sh
bash docs/demos/cli-help/reproduce.sh /path/to/old/quipu /path/to/new/quipu
```

The real-binary tests additionally cover all supported nested help paths,
unknown options, existing-store bytes and mtimes, credential/config/network
syscall spies with positive controls, and a valid shapes load/list/remove cycle.
