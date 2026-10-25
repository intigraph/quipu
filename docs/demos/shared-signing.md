# Shared signing compatibility fixture

This replayable terminal recording compares the unchanged Quipu v1 signing
module at base `0d0b769d7311304583de2815c8f35b8e480b1991` with the extracted
source package. Both receive the same disposable fixed-seed identity, predicate,
target, outcome, evidence, tier and verifier. Their message, SHA-256 evidence
hash, public key and signature are byte-identical.

[Compressed terminal output](shared-signing.output.gz),
[timing](shared-signing.timing) and [runner](shared-signing.sh) are retained
with the source. Replay with:

```sh
gzip -dc docs/demos/shared-signing.output.gz > /tmp/shared-signing.output
scriptreplay --log-out /tmp/shared-signing.output \
  --log-timing docs/demos/shared-signing.timing
```

The baseline executable imports the unchanged root signing module. The shared
executable comes from `crates/quipu-signing/examples/v1_fixture.rs`; the committed
fixed vector also checks exact bytes and rejection after each signed field is
changed. The intentional v1-to-v2 source mutation fails that control.

The pull request records the exact source head and validation. This is an
extraction compatibility demonstration, not a consumer migration or registry
publication. Quipu and Yupana have not switched to the package. The package
remains `publish = false`; distribution, reviewed consumer pins and their full
acceptance remain separate work. No operational private key or registration is
used; never reuse the fixture seed for an operational identity.
