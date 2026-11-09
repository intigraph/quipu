# Actionable query and vocabulary refusals

Trigger: `ASK { ?s rdfs:label ?o }` without a declaration. Before, the public
query returned the parser's generic `Prefix not found` message. After, it names
`rdfs:` and requests an explicit declaration without choosing a namespace.
An explicit custom namespace remains valid. Blank-node labels are never
reported as namespace prefixes; oversized name inventories use generic advice.

[Native fixture transcript](transcript.txt) records the same public integration
probe with the original error mapping deliberately restored at all three parser
entry points, then with the repaired mappings restored byte-for-byte. The base
is `d3cdf6483ac25068d141ed69d42a04885f6401b6`; the PR caption pins the demonstrated
source commit. This is a text transcript: terminal replay timing was not captured.

Unknown types still refuse before writing. The native MCP fixture records a
bounded, loaded-only class suggestion, confirms that the refused subject has
no facts, and successfully writes and reads the legal type. Lexical similarity
is advisory and does not establish semantic equivalence. Timeout guidance directs
exploratory discovery to search while preserving authoritative query checks.

Repeat the private native probes from a source checkout:

```sh
cargo test --locked --no-default-features --features full --lib sparql_structure::diagnostics -- --nocapture
cargo test --locked --no-default-features --features full --lib vocabulary::suggestions -- --nocapture
```

The integration control must fail if the three `query_error` calls in
`src/sparql/mod.rs` and `src/sparql/read_snapshot.rs` are replaced by their original
`Error::InvalidValue(format!("SPARQL parse error: {e}"))` mappings. Restore the exact
source before repeating the positive. These are isolated library/MCP fixtures;
the transcript does not measure deployed client adoption or production error rates.
