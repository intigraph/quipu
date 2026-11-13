# Fluent HTTP and CLI adapter

This adapter implements [the fluent contract](fluents-v1-contract.md). It adds
no schema loader, credential issuer or alternate local database write path.
The existing native writer reserves the transaction, binds the authenticated
writer IRI, checks ownership and SHACL, and commits the observation atomically.
Live use still requires the governed fluent vocabulary and a property binding.

`POST /fluent/observations` accepts this closed JSON object:

```json
{
  "graph": "urn:example:graph",
  "fluent": "urn:example:count",
  "event": "urn:example:event:1",
  "subject": "urn:example:subject",
  "predicate": "urn:example:count-predicate",
  "property_shape": "urn:example:count-shape",
  "as_of": "2026-11-12T01:00:00Z",
  "value": {"Integer": 3}
}
```

Use a registered named bearer with an authenticated IRI. Anonymous/open-write
requests and legacy shared credentials without such an IRI are refused.
Restricted writer credentials retain their existing route restrictions.
Caller-supplied `writer`, transaction and recording-time fields are rejected.
The graph must already exist and its property shape must select the subject.
Sum values are increments. Latest/set can use `{"Term": "<urn:example:value>"}`.

Success returns `{"tx": 123}`. Repeating the same event and content returns the
original transaction without appending another observation. Changing an
existing event's content is refused. A transport timeout is indeterminate:
retain the event ID and bytes for an idempotent retry.

`GET /fluent/current?graph=<IRI>&fluent=<IRI>` derives the declared policy's
current value, producing event IDs, producing transaction IDs and store snapshot
transaction. An unknown fluent or graph refuses. This read creates nothing.
Existing scoped-read authorization still applies.

The CLI requires the `remote` feature and an explicit server origin:

```sh
quipu fluent append observation.json --server https://example.org
quipu fluent current --server https://example.org --graph urn:example:graph --fluent urn:example:count
```

The append credential comes from `QUIPU_AUTH_TOKEN`, then
`QUIPU_AUTH_TOKEN_FILE`; credentials never appear in command arguments.
The CLI calls these HTTP routes only, refuses `--db`, disables redirects,
bounds request/response sizes and uses a 30-second transport timeout.
