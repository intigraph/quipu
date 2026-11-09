# Development graph fixtures

Generate `test-fixtures/test-store.db` with the namespace in the project's
`[quipu].base_ns` configuration:

```sh
just seed
```

For a fresh development fixture, select it explicitly:

```sh
just seed --base-ns https://example.org/fixtures/
```

The generator recreates only its fixed development database path. It applies
one namespace to the static RDF, episode ingestion, layout metadata, temporal
mutations and verification. A missing or invalid namespace refuses before the
database is removed. Selecting a namespace here does not migrate an existing
service's data or configuration.

For compatibility with the original fixture vocabulary, pass
`http://aegis.gastown.local/ontology/` explicitly. Reused episode nodes omit a
new description when their description already came from static RDF; their
original provenance is preserved.
