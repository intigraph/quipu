"""Typed builder invariants and exact existing REST wire contract."""

from dataclasses import FrozenInstanceError
import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from quipu_client import (
    Aggregate,
    Call,
    Compare,
    Filter,
    Graph,
    Group,
    Iri,
    Literal,
    Path as PropertyPath,
    QuipuClient,
    QuipuError,
    Select,
    Triple,
    Var,
)
from builder_cases import cases, literal_query
from test_client import canned
from test_client import stub as stub  # explicit pytest fixture re-export

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = json.loads((ROOT / "test-fixtures/query-builder/cases.json").read_text())


def test_reference_serialization_and_immutable_composition():
    for name, query in cases().items():
        assert query.to_sparql() == FIXTURE["queries"][name]["query"]
        assert query.to_sparql() == query.to_sparql()
    base = cases()["simple"]
    child = base.limit(3).valid_at("2026-01-02T00:00:00Z").as_of_tx(42)
    assert child.to_request() == {
        "query": base.to_sparql() + " LIMIT 3",
        "valid_at": "2026-01-02T00:00:00Z",
        "tx": 42,
    }
    assert base.to_request() == {"query": base.to_sparql()}
    with pytest.raises(FrozenInstanceError):
        base.patterns = ()
    with pytest.raises(ValueError, match="without_temporal"):
        child.stored_definition("saved")
    assert (
        child.without_temporal().stored_definition("saved")["template"]
        == child.to_sparql()
    )


@pytest.mark.parametrize("value", FIXTURE["literals"])
def test_literal_roundtrip_and_no_accidental_registry_placeholder(value):
    rendered = Literal(value).sparql()
    assert json.loads(rendered) == value
    assert "{param}" not in rendered
    assert literal_query(value).to_sparql().endswith(" }")


@pytest.mark.parametrize(
    "make",
    [
        lambda: Var("?raw"),
        lambda: Var("x )"),
        lambda: Iri("relative"),
        lambda: Iri("urn:x>"),
        lambda: Literal(float("nan")),
        lambda: Literal("\ud800"),
        lambda: Literal("x", language="en; bad"),
        lambda: Literal("x", datatype=Iri("urn:t"), language="en"),
        lambda: Triple("?s", Iri("urn:p"), Var("o")),
        lambda: PropertyPath("/", (Iri("urn:p"),)),
        lambda: Filter("raw"),
        lambda: Call("RAW", (Var("x"),)),
        lambda: Compare(Var("x"), "raw", Literal(1)),
        lambda: Select((Var("x"), Var("x"))),
        lambda: Select((Var("x"),)).limit(True),
        lambda: Select((Var("x"),)).as_of_tx(-1),
        lambda: Select((Var("x"),)).valid_at("2026-02-30T00:00:00Z"),
        lambda: Select((Var("x"), Aggregate("COUNT", None, Var("n")))).to_sparql(),
    ],
)
def test_invalid_nodes_and_semantics_refuse(make):
    with pytest.raises((TypeError, ValueError)):
        make()


def test_named_graph_language_datatype_inverse_and_call():
    v = Var("v")
    q = (
        Select((v,), distinct=True)
        .from_named(Iri("urn:g"))
        .where(Graph(Iri("urn:g"), Group((Triple(Iri("urn:s"), Iri("urn:p"), v),))))
    )
    assert "FROM NAMED <urn:g> WHERE { GRAPH <urn:g>" in q.to_sparql()
    assert Literal("hello", language="en").sparql() == '"hello"@en'
    assert (
        Literal("1.5", datatype=Iri("http://www.w3.org/2001/XMLSchema#decimal"))
        .sparql()
        .endswith("^^<http://www.w3.org/2001/XMLSchema#decimal>")
    )
    assert PropertyPath("^", (Iri("urn:p"),)).sparql() == "^(<urn:p>)"
    assert Call("BOUND", (v,)).sparql() == "BOUND(?v)"


def test_execute_and_save_use_distinct_auth_and_preserve_temporal_wire(stub):
    stub.responses["/query"] = canned({"count": 2})
    stub.responses["/queries"] = canned({"loaded": "labels", "params": 0})
    client = QuipuClient(
        stub.url,
        token="synthetic-write-token",
        client_id="builder-test",
        task_id="synthetic",
    )
    base = cases()["simple"]
    pinned = base.valid_at("2026-01-02T00:00:00Z").as_of_tx(9)
    assert pinned.execute(client) == {"count": 2}
    assert stub.requests[0]["body"] == pinned.to_request()
    assert "Authorization" not in stub.requests[0]["headers"]
    assert (
        base.save(
            client, "labels", description="Local labels", dataset="fixture-dataset"
        )["loaded"]
        == "labels"
    )
    request = stub.requests[1]
    assert request["path"] == "/queries"
    assert request["body"] == base.stored_definition(
        "labels", description="Local labels", dataset="fixture-dataset"
    )
    assert request["headers"]["Authorization"] == "Bearer synthetic-write-token"
    assert request["headers"]["X-Quipu-Task"] == "synthetic"
    stub.responses["/queries"] = canned({"reason": "write_refused"}, 403)
    with pytest.raises(QuipuError) as error:
        base.save(client, "denied")
    assert error.value.reason == "write_refused"
    assert len(stub.requests) == 3  # no automatic retry
    with pytest.raises(ValueError):
        pinned.save(client, "lost-pin")
    assert len(stub.requests) == 3
