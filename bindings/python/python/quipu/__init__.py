"""Quipu as a local Python library.

    import quipu

    with quipu.Store.memory() as store:
        store.load_shapes("records", SHAPES_TTL)
        tx = store.knot('<urn:a> a <urn:Record>; <urn:title> "A" .')["tx_id"]
        rows = store.query("SELECT ?t WHERE { <urn:a> <urn:title> ?t }")
        before = store.query("SELECT ...", as_of_tx=tx)
        facts = store.history("urn:a")

Every call runs the native engine in-process through the same operation
registry the WASM ``Database`` uses: the same handlers, transactions,
vocabulary gate and SHACL write gate as the server. No HTTP, no sidecar.

A store handle is bound to the thread that opened it; calls from another
thread raise :class:`ThreadAffinityError`. Open one store per thread.
"""

from __future__ import annotations

import os
from typing import Any

from . import errors
from ._quipu import CORE_VERSION, Engine as _Engine, __version__
from .errors import (
    BusyError,
    InvalidInputError,
    PolicyDeniedError,
    QuipuError,
    ReadOnlyReplicaError,
    SchemaNotAdoptedError,
    StoreClosedError,
    ThreadAffinityError,
    UnsupportedCapabilityError,
    ValidationError,
)
from .terms import IRI, Literal, from_native, to_native

__all__ = [
    "CORE_VERSION",
    "IRI",
    "BusyError",
    "InvalidInputError",
    "Literal",
    "PolicyDeniedError",
    "QuipuError",
    "ReadOnlyReplicaError",
    "SchemaNotAdoptedError",
    "Store",
    "StoreClosedError",
    "ThreadAffinityError",
    "UnsupportedCapabilityError",
    "ValidationError",
    "__version__",
    "errors",
]


def _drop_none(**fields: Any) -> dict:
    return {k: v for k, v in fields.items() if v is not None}


class Store:
    """A local, governed Quipu store. Use :meth:`memory` or :meth:`open`."""

    __slots__ = ("_engine",)

    def __init__(self, engine: _Engine) -> None:
        self._engine = engine

    # -- lifecycle ---------------------------------------------------------

    @classmethod
    def memory(cls) -> Store:
        """A fresh in-memory store with no adopted schema."""
        return cls(_Engine.memory())

    @classmethod
    def open(cls, path: str | os.PathLike[str]) -> Store:
        """Open (or create) a file-backed store. Adopted shapes persist.

        Do not open a database that a running quipu-server is serving. The
        file stays consistent, but the server never sees these writes in its
        caches, read models or change feed, and this store runs with default
        configuration rather than the server's governance, label and SHACL
        settings. Nothing detects that case yet: use the REST client
        (``quipu_client``) for a served store. One writer per database file.
        """
        return cls(_Engine.open(os.fspath(path)))

    def close(self) -> None:
        """Release the store. Idempotent."""
        self._engine.close()

    @property
    def closed(self) -> bool:
        return self._engine.closed

    def __enter__(self) -> Store:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    # -- contract ----------------------------------------------------------

    def capabilities(self) -> dict:
        """Engine identity (version, revision, features), storage, schema
        state and per-operation availability."""
        return self._engine.capabilities()

    def invoke(self, operation: str, input: dict | None = None) -> Any:
        """Low-level: run one exact native operation (e.g. ``quipu_knot``) and
        return its native result as Python objects, unmodified."""
        return self._engine.invoke(operation, {} if input is None else input)

    def invoke_json(self, operation: str, input: str) -> str:
        """Low-level: JSON text in, JSON text out, native lexemes preserved."""
        return self._engine.invoke_json(operation, input)

    # -- shapes ------------------------------------------------------------

    def load_shapes(self, name: str, turtle: str, *, timestamp: str | None = None) -> dict:
        """Adopt a SHACL shape set. Its target classes become the write vocabulary."""
        return self.invoke(
            "quipu_shapes", _drop_none(action="load", name=name, turtle=turtle, timestamp=timestamp)
        )

    def remove_shapes(self, name: str) -> bool:
        """Remove a shape set; returns whether it existed."""
        return bool(self.invoke("quipu_shapes", {"action": "remove", "name": name})["found"])

    def shapes(self) -> Any:
        """The adopted shape catalogue (native ``list`` result)."""
        return self.invoke("quipu_shapes", {"action": "list"})

    # -- writes (all through the SHACL write gate) -------------------------

    def knot(
        self,
        turtle: str,
        *,
        timestamp: str | None = None,
        graph: str | None = None,
        **options: Any,
    ) -> dict:
        """Write Turtle. Raises :class:`ValidationError` when the write gate refuses
        it; the native SHACL report is in ``details``."""
        result = self.invoke(
            "quipu_knot", _drop_none(turtle=turtle, timestamp=timestamp, graph=graph, **options)
        )
        if isinstance(result, dict) and result.get("conforms") is False:
            raise ValidationError(
                f"write refused by SHACL: {result.get('violations', '?')} violation(s)",
                code="validation_failed",
                operation="quipu_knot",
                # The native report carries no tx_id and the conformance corpus
                # asserts no fact leaked, but the facade contract only promises
                # `not_started` for refusals before dispatch.
                commit_state="unknown",
                details=result,
            )
        return result

    def episode(
        self,
        name: str,
        *,
        nodes: list[dict] | tuple = (),
        edges: list[dict] | tuple = (),
        episode_body: str = "",
        source: str | None = None,
        **extra: Any,
    ) -> dict:
        """Ingest an agent episode (node types must be in the adopted vocabulary)."""
        return self.invoke(
            "quipu_episode",
            _drop_none(
                name=name,
                nodes=list(nodes),
                edges=list(edges),
                episode_body=episode_body,
                source=source,
                **extra,
            ),
        )

    def set(
        self, entity: str, predicate: str, value: Any, *, timestamp: str | None = None
    ) -> dict:
        """Replace the current value of ``entity predicate``. ``value`` may be
        ``str``, ``int``, ``float``, ``bool``, :class:`IRI` or :class:`Literal`."""
        return self.invoke(
            "quipu_set",
            _drop_none(entity=entity, predicate=predicate, value=to_native(value), timestamp=timestamp),
        )

    # -- reads -------------------------------------------------------------

    def query(
        self,
        sparql: str,
        *,
        as_of_tx: int | None = None,
        valid_at: str | None = None,
        graph: str | None = None,
    ) -> Any:
        """Run SPARQL. SELECT returns a list of ``{var: value}`` dicts, ASK a
        ``bool``, CONSTRUCT/DESCRIBE a list of ``(s, p, o)`` tuples.

        ``as_of_tx`` reads the store as of a committed transaction;
        ``valid_at`` reads at a valid time. Use :meth:`invoke` for the full
        native result (markers, truncation, labels)."""
        result = self.invoke(
            "quipu_query", _drop_none(query=sparql, tx=as_of_tx, valid_at=valid_at, graph=graph)
        )
        if "rows" in result:
            return [{k: from_native(v) for k, v in row.items()} for row in result["rows"]]
        if "result" in result:
            return bool(result["result"])
        if "triples" in result:
            return [(t["subject"], t["predicate"], from_native(t["object"])) for t in result["triples"]]
        return result

    def history(self, iri: str) -> list[dict]:
        """Every assert/retract on an entity in the default graph, oldest first,
        with ``valid_from``/``valid_to`` (valid time) and ``tx`` (transaction)."""
        entries = self.invoke("quipu_entity_history", {"iri": iri})["history"]
        return [{**e, "value": from_native(e["value"])} for e in entries]
