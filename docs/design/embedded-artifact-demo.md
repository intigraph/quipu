# Embedded artifact demonstration

[Watch or download the recording](assets/embedded-artifact-demo.webm).

Trigger: open a priority's record. The initial synthetic fixture has a static
row with no embedded graph. In the generated fixture, **Open record** selects
`https://example.org/item/sample-release` and shows its facts through the real
Quipu Wasm query engine. The positive query returns exactly that one record.

Base: `21bd8e8e42c0a238a3c5d4af1d4a0b5f2bcfe8f6` has no embedded-artifact
skill or generator. Demonstrated source:
`be1fb388ef679862b44608ce5e7bb7e78e5703e1`. This is a local synthetic fixture,
not a recording of an installed service or a hosted Artifact. Its remote input
is explicitly stubbed; native shape loading, query registration, outward share,
manifest verification, promotion and browser queries are actual execution.
The static opening scene illustrates the missing interaction; it is not an
executable version of a previously shipped brief.

Reproduce from the repository root with matching installed native Quipu and
released browser assets:

```bash
just explorer release
just --command python3 skills/quipu-embedded-artifacts/scripts/demo.py \
  --out /tmp/quipu-artifact-example --work /tmp/quipu-artifact-work \
  --bundle docs/book/src/explore/pkg --quipu /path/to/quipu
just --command python3 -m http.server 8080 --bind 127.0.0.1 \
  --directory /tmp/quipu-artifact-example
```

Both generated directories must be new. Visit `before.html`, then `index.html`
and select **Open record**. The owner-facing row file uses version-checked
updates; this demonstration does not apply database operations or enroll keys.

The small WebM is retained in the existing design-assets directory with this
caption so review does not depend on a private recording host. Native browser
playback is supported; GitHub may require downloading the file rather than
embedding it. The recording contains synthetic public data and no credentials.
