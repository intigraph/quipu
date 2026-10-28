# Real TerminusDB exports

These files were captured on 2026-10-27 from a real TerminusDB community server:

- image: `terminusdb/terminusdb-server:v12.0.8`
- digest: `sha256:43061044d1857bb6f1a2f41d7a59ad06ba5f91c03cbc90128d7c0415f32a0916` (multi-arch manifest list, amd64 and arm64; captured on arm64)
- reported version: 12.0.8, git `79cbbcffe78d253828e0f2eaf3fbe2a21273dfb6`, store 0.19.8

The database `admin/people` was created from `schema.json` and given three data
commits by `seed_live()` in `scripts/test-terminusdb-import.py`. The schema
covers Optional, Set, List, Array, Cardinality, Enum, `@inherits`, `@key`
Lexical, `sys:JSON`, `sys:Unit` in a TaggedUnion, `xdd:url`, and a property
whose range is a parent class (`Team.lead: Entity`). The files were then
fetched unmodified:

| File | Request |
|---|---|
| `schema.ttl` | `GET /api/triples/admin/people/local/branch/main/schema` (`Accept: text/turtle`) |
| `instance-<commit>.ttl` | `GET /api/triples/admin/people/local/commit/<commit>/instance` |
| `log.json` | `GET /api/log/admin/people/local/branch/main` |

The exports show what TerminusDB's writer really emits:

- an `@base` line with relative IRIs (`<schema#Person>`, `<Person/Ada%20Lovelace>`, `<../json/...>`);
- numbers in Turtle shorthand, so a stored `36^^xsd:decimal` is written as `36`;
- `sys:JSON` values as linked nodes;
- `sys:Array` cells;
- an empty instance graph at the schema-only first commit;
- four commits inside the same second.

To reproduce:

```bash
docker run -d --name tdb -p 127.0.0.1:6363:6363 -e TERMINUSDB_ADMIN_PASS=pw \
  terminusdb/terminusdb-server:v12.0.8
TERMINUSDB_LIVE_URL=http://127.0.0.1:6363 TERMINUSDB_USER=admin TERMINUSDB_PASSWORD=pw \
  python3 scripts/test-terminusdb-import.py
docker rm -f tdb
```
