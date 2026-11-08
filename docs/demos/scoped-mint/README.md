# Scoped mint fixture recording

Base `c2e3a34979ba4d34144363e01b2350ab623231af` has no mint operation for
scoped credentials: the route refuses them. This candidate adds an independently
enrolled mint-only grant. A valid fixture credential with no grant still gets
403; the exact grant permits minting, and the repeat returns the same IRI without
a new transaction. Wrong selectors, an ungranted principal, a broader update,
revocation and expiry refuse. Headers claiming a different principal do not
replace the authenticated identity recorded on the mint transaction.

The [output](output.log) and [timing](timing.log) retain actual local HTTP calls
through the authorization middleware and real handlers. All stores, graphs,
namespace policies and credentials are disposable synthetic fixtures. This is
candidate behavior, not a before-binary recording or installed acceptance.
Production credential delivery and grant enrollment remain off.

Reproduce from the checkout:

```sh
cargo test --no-default-features --features shacl,onnx,server \
  --bin quipu-server \
  scoped_mint_exact_selectors_default_off_and_no_authority_fallback -- --nocapture
```

For terminal playback, download both logs and run:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The source tests assert status, exact identity, convergence, authenticated
transaction provenance and refusal atomicity; the recording prints the actual
response fields and statuses. It does not expose token values or exercise a
real operator credential. The wider quality gate also tests mixed-scope grant
refusal, independently composed type grants and the commit/revocation barrier.
