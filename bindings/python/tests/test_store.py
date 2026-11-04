"""The Pythonic surface over the shared engine facade."""

import json

import pytest

import quipu
from conftest import RECORD_SHAPE, TS

GOOD = '<urn:good> a <urn:Record>; <urn:title> "Before" .'


def test_import_name_version_and_engine_identity(store):
    caps = store.capabilities()
    assert caps["contract"] == "quipu-bindings/v1"
    assert caps["version"] == quipu.CORE_VERSION
    # The wheel's version is the core's; a mismatch means the build skipped
    # scripts/sync-python-binding.py after a core release bump.
    assert quipu.__version__ == quipu.CORE_VERSION, "run scripts/sync-python-binding.py"
    assert caps["features"]["shacl"] is True
    assert caps["storage"] == "memory"
    assert caps["full_parity"] is False
    assert isinstance(caps["revision"], str) and caps["revision"]


def test_context_manager_closes_and_closed_store_refuses():
    with quipu.Store.memory() as s:
        assert not s.closed
    assert s.closed
    with pytest.raises(quipu.StoreClosedError) as err:
        s.query("ASK { ?s ?p ?o }")
    assert err.value.commit_state == "not_started"
    s.close()  # idempotent


def test_writes_refused_until_shapes_adopted(store):
    with pytest.raises(quipu.SchemaNotAdoptedError) as err:
        store.knot(GOOD, timestamp=TS)
    assert err.value.code == "schema_not_adopted"
    assert err.value.commit_state == "not_started"
    assert err.value.retryable is False
    assert store.query("SELECT ?s WHERE { ?s ?p ?o }") == []


def test_shapes_knot_query_current_and_as_of_history(shaped):
    assert "record" in json.dumps(shaped.shapes())
    tx = shaped.knot(GOOD, timestamp=TS)["tx_id"]
    assert isinstance(tx, int) and tx > 0
    shaped.set("urn:good", "urn:title", "After", timestamp="2026-10-20T00:01:00Z")
    q = "SELECT ?t WHERE { <urn:good> <urn:title> ?t }"
    assert shaped.query(q) == [{"t": "After"}]
    assert shaped.query(q, as_of_tx=tx) == [{"t": "Before"}]
    values = [(e["op"], e["value"]) for e in shaped.history("urn:good") if e["predicate"] == "urn:title"]
    assert ("assert", "Before") in values and ("assert", "After") in values
    assert all(isinstance(e["tx"], int) for e in shaped.history("urn:good"))
    assert shaped.query("ASK { <urn:good> a <urn:Record> }") is True
    triples = shaped.query("CONSTRUCT { ?s <urn:title> ?t } WHERE { ?s <urn:title> ?t }")
    assert triples == [("urn:good", "urn:title", "After")]


def test_write_gate_refusal_is_typed_and_leaves_no_fact(shaped):
    with pytest.raises(quipu.ValidationError) as err:
        shaped.knot("<urn:bad> a <urn:Record> .", timestamp=TS)
    assert err.value.details["violations"] == 1
    assert err.value.details["issues"][0]["focus_node"] == "urn:bad"
    assert err.value.retryable is False
    assert shaped.query("SELECT ?s WHERE { ?s a <urn:Record> }") == []
    with pytest.raises(quipu.QuipuError):
        shaped.history("urn:bad")
    shaped.knot(GOOD, timestamp=TS)  # positive control
    assert shaped.query("SELECT ?s WHERE { ?s a <urn:Record> }") == [{"s": "urn:good"}]


def test_low_level_invoke_returns_the_native_report_unmodified(shaped):
    report = shaped.invoke("quipu_knot", {"timestamp": TS, "turtle": "<urn:bad> a <urn:Record> ."})
    assert report["conforms"] is False
    raw = shaped.invoke_json("quipu_query", json.dumps({"query": "ASK { ?s ?p ?o }"}))
    assert json.loads(raw)["result"] is False


def test_large_integers_and_tagged_literals_are_lossless(shaped):
    shaped.knot('<urn:big> a <urn:Record>; <urn:title> "Hola"@es; <urn:n> 9007199254740993 .', timestamp=TS)
    [row] = shaped.query("SELECT ?n ?t WHERE { <urn:big> <urn:n> ?n; <urn:title> ?t }")
    assert row["n"] == 9007199254740993 and type(row["n"]) is int
    assert row["t"] == quipu.Literal("Hola", lang="es")
    raw = shaped.invoke_json("quipu_query", json.dumps({"query": "SELECT ?n WHERE { <urn:big> <urn:n> ?n }"}))
    assert "9007199254740993" in raw


def test_set_accepts_typed_terms(shaped):
    shaped.knot(GOOD, timestamp=TS)
    shaped.set("urn:good", "urn:title", quipu.Literal("Hallo", lang="de"), timestamp="2026-10-20T00:02:00Z")
    assert shaped.query("SELECT ?t WHERE { <urn:good> <urn:title> ?t }") == [{"t": quipu.Literal("Hallo", "de")}]


def test_inputs_without_exact_json_form_are_refused_before_dispatch(store):
    for bad in ({"query": float("nan")}, {"query": 2**70}, {1: "x"}, {"query": object()}):
        with pytest.raises(quipu.InvalidInputError) as err:
            store.invoke("quipu_query", bad)
        assert err.value.commit_state == "not_started"
    with pytest.raises(quipu.InvalidInputError):
        store.invoke_json("quipu_query", "{")


def test_unsupported_operation_is_typed(store):
    with pytest.raises(quipu.UnsupportedCapabilityError) as err:
        store.invoke("quipu_update", {"entity": "urn:never"})
    assert err.value.operation == "quipu_update"
    assert err.value.commit_state == "not_started"


def test_episode_respects_the_adopted_vocabulary(store):
    store.load_shapes(
        "note",
        "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:NoteShape> a sh:NodeShape; "
        "sh:targetClass <http://aegis.gastown.local/ontology/Note> .",
    )
    with pytest.raises(quipu.QuipuError):
        store.episode("stray", nodes=[{"name": "stray", "type": "NotInTheVocabulary"}], source="test")
    store.episode("kept", nodes=[{"name": "kept", "type": "Note"}], source="test")
    assert len(store.query("SELECT ?s WHERE { ?s a <http://aegis.gastown.local/ontology/Note> }")) == 1


def test_file_store_persists_shapes_and_facts_across_reopen(tmp_path):
    path = tmp_path / "kg.db"
    with quipu.Store.open(path) as s:
        assert s.capabilities()["storage"] == "file"
        s.load_shapes("record", RECORD_SHAPE, timestamp=TS)
        s.knot(GOOD, timestamp=TS)
    with quipu.Store.open(str(path)) as s:
        assert s.capabilities()["schema_state"] == "adopted"
        assert s.query("SELECT ?s WHERE { ?s a <urn:Record> }") == [{"s": "urn:good"}]
        with pytest.raises(quipu.ValidationError):
            s.knot("<urn:bad> a <urn:Record> .", timestamp=TS)
