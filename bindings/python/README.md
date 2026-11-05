# quipu-kg: Quipu as a local Python library

`import quipu` runs the Quipu engine in-process: governed writes through the
same SHACL write gate as the server, SPARQL (current and as-of), and
bitemporal entity history. No server, no HTTP. For the remote REST client, see
[`python/quipu_client`](../../python/README.md).

Not yet on PyPI. Build and install a wheel from this directory:

```bash
python3 ../../scripts/sync-python-binding.py   # after a core version bump
pip install maturin && maturin build --release && pip install target/wheels/quipu_kg-*.whl
```

The wheel's version is the core crate's. release-plz bumps only the root
crate, so the sync script sets this crate's version and refreshes the
`quipu-ai` path entry in its Cargo.lock (registry pins are untouched).

```python
import quipu

SHAPES = """@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:RecordShape> a sh:NodeShape; sh:targetClass <urn:Record>;
  sh:property [sh:path <urn:title>; sh:minCount 1] ."""

with quipu.Store.open("kg.db") as store:          # or quipu.Store.memory()
    store.load_shapes("records", SHAPES)           # writes need adopted shapes
    tx = store.knot('<urn:a> a <urn:Record>; <urn:title> "First" .')["tx_id"]
    store.set("urn:a", "urn:title", "Second")
    try:
        store.knot("<urn:b> a <urn:Record> .")     # no title: refused
    except quipu.ValidationError as e:
        print(e.details["issues"])
    q = "SELECT ?t WHERE { <urn:a> <urn:title> ?t }"
    print(store.query(q))                          # [{'t': 'Second'}]
    print(store.query(q, as_of_tx=tx))             # [{'t': 'First'}]
    print(store.history("urn:a"))
```

- **Results** are Python objects: SELECT gives a list of dicts, ASK a bool,
  CONSTRUCT a list of `(s, p, o)` tuples. Tagged and typed literals come back
  as `quipu.Literal`. IRIs and plain strings both come back as `str`, because
  the native result format does not tell them apart.
- **Low level**: `store.invoke("quipu_knot", {...})` returns the native result
  unmodified; `store.invoke_json(op, text)` is JSON text in and out.
  `store.capabilities()` reports the engine version, git revision, features,
  storage, schema state and which operations are available.
- **Errors** are typed (`quipu.errors`) and carry `code`, `operation`,
  `commit_state`, `retryable` (always `False`) and `details`. Only
  `commit_state == "not_started"` means nothing was written.
- **One writer per file. Never open a database a running `quipu-server` is
  serving.** The file stays consistent, but the server's caches, read models
  and change feed never see this library's writes, and the library runs with
  default configuration, not the server's governance, label and SHACL
  settings. Native bindings and writer-mode servers now take a shared
  nonblocking advisory lock on the database file for the handle's lifetime.
  A second participant refuses before SQLite initialization. Older servers,
  raw SQLite and other nonparticipating writers are not detected. Do not
  replace or unlink a database while it is open. Use the REST client
  ([`quipu_client`](../../python/README.md)) for a served store.
  File opens require a plain filesystem path; SQLite `file:` URIs refuse
  rather than lock a different file. Memory stores need no process lock.
- **Threads**: a store belongs to the thread that opened it. Calls from other
  threads raise `quipu.ThreadAffinityError`. Open a separate database per thread. No
  free-threaded CPython support yet.

This first slice covers query, export, entity history, knot, episode, set,
retract and shapes. It does not claim full parity with the native tool set.
