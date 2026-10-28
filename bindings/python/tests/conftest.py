import json
import os
import pathlib

import pytest

RECORD_SHAPE = (
    "@prefix sh: <http://www.w3.org/ns/shacl#> . "
    "<urn:RecordShape> a sh:NodeShape; sh:targetClass <urn:Record>; "
    "sh:property [sh:path <urn:title>; sh:minCount 1] ."
)
TS = "2026-10-20T00:00:00Z"

# The shared corpus lives at the repository root so the native, WASM and
# Python runners read one file. QUIPU_CONFORMANCE_CORPUS overrides the path
# for installed-wheel runs from outside the checkout.
CORPUS = pathlib.Path(
    os.environ.get(
        "QUIPU_CONFORMANCE_CORPUS",
        pathlib.Path(__file__).resolve().parents[3]
        / "tests/fixtures/bindings-conformance/v1.json",
    )
)


@pytest.fixture
def corpus():
    return json.loads(CORPUS.read_text())


@pytest.fixture
def store():
    import quipu

    with quipu.Store.memory() as s:
        yield s


@pytest.fixture
def shaped(store):
    store.load_shapes("record", RECORD_SHAPE, timestamp=TS)
    return store
