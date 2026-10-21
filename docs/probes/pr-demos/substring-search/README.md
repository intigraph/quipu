# Explicit substring preparation

The same synthetic description query refuses without a prepared index, then
returns one hit with the required acknowledgement after explicit preparation.
The SDK finds that description-only hit. No existing database or remote service
is used. The demo always stops its own server and removes its temporary files.

```sh
python3 scripts/demo-substring-search.py --server ./target/release/quipu-server \
  --preparer ./target/release/examples/substring_index --sdk /path/to/sd
```

Build the preparer with `cargo build --release --example substring_index`.
The [recording](output.log) and [timing](timing.log) contain only this fixture.

A separate frozen-fixture comparison preserved exact SDK JSON for five search
terms and a zero-result control. The complete candidate matrix used 6–20 requests
and at most 86,343,680 bytes of total server cgroup growth, sampled every 100 ms;
the fixed limits were 20 requests and 200,000,000 bytes. The retained naive first
term used 5,731 requests and 779,182,080 bytes. These are individual private runs,
not production acceptance, a general memory bound or a latency percentile.
