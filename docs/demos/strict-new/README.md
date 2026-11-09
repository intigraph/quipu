# Request-local strict admission

This offline library demonstration uses two independent in-memory stores with
the same seeded `document-alpha` and candidate `document-alpha-variant`.
With the option OFF, the candidate is admitted. With it ON, the candidate is
refused without a transaction. Both stores keep global resolution and strict
mode OFF. These are synthetic identities, not a production smoke.

Run `just demo strict-new`. The recording is output-only, with its original
timing retained in [timing.txt](timing.txt) and a readable
[transcript](output.txt). Replay from this directory:

```sh
scriptreplay --log-out output.txt --log-timing timing.txt
```

The baseline source is `d854c0897597dfb6c9f0e42ba6ba55084274c41e`.
The candidate is the exact PR head named in the PR's Demo caption; its
[reproducer](../../../examples/episode_strict_new.rs) is committed alongside
these small artifacts. OFF represents compatibility behavior on the candidate,
not a separately compiled baseline binary. Authentication is a test identity
scoped through the native request adapter. The separate HTTP tests exercise the
actual authentication and writer handlers.

No network service, production data, global configuration or installed binary
is changed by this demo. The artifacts are retained with the source revision.
