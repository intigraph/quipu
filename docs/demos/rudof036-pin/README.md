# Rudof 0.3.36 dependency qualification

The previous 0.3.25 pin could discard a property-path lookup error and return
empty or partial success. Upstream commit `cda0e41080671afe1b4c9a6a7fbaed0c2bba7a56`
repairs that path. Version 0.3.36 is the first tagged release containing it,
at `02d3f464857e898915828a8cb28268fc8f92f228`.

The [actual test transcript](transcript.txt) records consumer SHACL compatibility,
the unmodified release library and W3C suites, and the upstream
`failed_path_lookup_is_an_error` control. That control answers local SPARQL
queries with HTTP 500 and requires a validation error in native and SPARQL modes.
A valid empty path remains a valid empty result. Repeat the control in a clean
upstream 0.3.36 release checkout with:

```sh
cargo test --locked -p shacl --lib failed_path_lookup_is_an_error
```

The W3C fixture is pinned to `a7f5f90686fcc4d023834d07e100d1fda6814a58`.
The previous retained library qualification passed 59 tests and its W3C suite
passed 116 with 18 ignored; the newer release has a larger suite. These are
source qualification receipts, not a same-input provider performance benchmark
or an installed-service demonstration. No recording or live endpoint was used.
Whole-process memory acceptance and bounded-engine integration remain separate.

The adapter delegates node-expression state to the native engine. Static write
planning skips obligations only for a constant true deactivation expression;
false, absent, and dynamic expressions retain conservative obligations.
Rollback is the ordinary source revert restoring the previous direct pins,
lockfile, and two compatibility adapters. This change performs no deployment.
