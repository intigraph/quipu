"""A small typed SELECT builder, serialized as ordinary SPARQL 1.1."""

from __future__ import annotations

from dataclasses import dataclass, replace
from datetime import datetime
import re
from typing import TYPE_CHECKING, Any

from .sparql_terms import (
    Aggregate,
    Call,
    Compare,
    Expr,
    Iri,
    Literal,
    Path,
    Var,
    expression,
)

__all__ = [
    "Aggregate",
    "Call",
    "Compare",
    "Filter",
    "Graph",
    "Group",
    "Iri",
    "Literal",
    "Optional",
    "Path",
    "Select",
    "Triple",
    "Union",
    "Var",
]

if TYPE_CHECKING:
    from .client import QuipuClient


class Pattern:
    def sparql(self) -> str:
        raise NotImplementedError


@dataclass(frozen=True)
class Triple(Pattern):
    subject: Var | Iri
    predicate: Var | Iri | Path
    object: Var | Iri | Literal

    def __post_init__(self):
        if (
            not isinstance(self.subject, (Var, Iri))
            or not isinstance(self.predicate, (Var, Iri, Path))
            or not isinstance(self.object, (Var, Iri, Literal))
        ):
            raise TypeError(
                "triple requires typed RDF terms (and optionally a predicate Path)"
            )

    def sparql(self) -> str:
        return f"{self.subject.sparql()} {self.predicate.sparql()} {self.object.sparql()} ."


@dataclass(frozen=True)
class Group(Pattern):
    patterns: tuple[Pattern, ...]

    def __post_init__(self):
        object.__setattr__(self, "patterns", tuple(self.patterns))
        if not all(isinstance(p, Pattern) for p in self.patterns):
            raise TypeError("group requires typed patterns")

    def sparql(self) -> str:
        return "{ " + " ".join(p.sparql() for p in self.patterns) + " }"


@dataclass(frozen=True)
class Optional(Pattern):
    group: Group

    def __post_init__(self):
        if not isinstance(self.group, Group):
            raise TypeError("OPTIONAL requires a Group")

    def sparql(self) -> str:
        return "OPTIONAL " + self.group.sparql()


@dataclass(frozen=True)
class Union(Pattern):
    groups: tuple[Group, ...]

    def __post_init__(self):
        object.__setattr__(self, "groups", tuple(self.groups))
        if len(self.groups) < 2 or not all(isinstance(g, Group) for g in self.groups):
            raise TypeError("UNION requires at least two Groups")

    def sparql(self) -> str:
        return " UNION ".join(g.sparql() for g in self.groups)


@dataclass(frozen=True)
class Filter(Pattern):
    expression: Expr

    def __post_init__(self):
        expression(self.expression)

    def sparql(self) -> str:
        return "FILTER (" + self.expression.sparql() + ")"


@dataclass(frozen=True)
class Graph(Pattern):
    graph: Iri | Var
    group: Group

    def __post_init__(self):
        if not isinstance(self.graph, (Iri, Var)) or not isinstance(self.group, Group):
            raise TypeError("GRAPH requires Iri/Var and Group")

    def sparql(self) -> str:
        return f"GRAPH {self.graph.sparql()} {self.group.sparql()}"


def _natural(value: int) -> int:
    if type(value) is not int or not 0 <= value <= 2**63 - 1:
        raise ValueError("expected a non-negative signed 64-bit integer")
    return value


def _instant(value: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(
        r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d{1,9})?Z", value
    ):
        raise ValueError("valid_at requires a UTC RFC3339 Z timestamp")
    datetime.fromisoformat(value.replace("Z", "+00:00"))
    return value


@dataclass(frozen=True)
class Select:
    projections: tuple[Var | Aggregate, ...]
    patterns: tuple[Pattern, ...] = ()
    grouping: tuple[Var, ...] = ()
    sources: tuple[Iri, ...] = ()
    named_sources: tuple[Iri, ...] = ()
    distinct: bool = False
    _limit: int | None = None
    _valid_at: str | None = None
    _tx: int | None = None

    def __post_init__(self):
        for field in (
            "projections",
            "patterns",
            "grouping",
            "sources",
            "named_sources",
        ):
            object.__setattr__(self, field, tuple(getattr(self, field)))
        if not self.projections or not all(
            isinstance(p, (Var, Aggregate)) for p in self.projections
        ):
            raise TypeError("SELECT requires Var or Aggregate projections")
        names = [
            p.name if isinstance(p, Var) else p.alias.name for p in self.projections
        ]
        if len(set(names)) != len(names):
            raise ValueError("duplicate projection name")
        Group(self.patterns)
        if not all(isinstance(v, Var) for v in self.grouping) or not all(
            isinstance(v, Iri) for v in (*self.sources, *self.named_sources)
        ):
            raise TypeError("GROUP BY requires Vars and FROM requires Iris")
        if type(self.distinct) is not bool:
            raise TypeError("distinct must be bool")
        if self._limit is not None:
            _natural(self._limit)
        if self._tx is not None:
            _natural(self._tx)
        if self._valid_at is not None:
            _instant(self._valid_at)

    def where(self, *patterns: Pattern) -> Select:
        return replace(self, patterns=self.patterns + patterns)

    def group_by(self, *variables: Var) -> Select:
        return replace(self, grouping=variables)

    def from_graph(self, *graphs: Iri) -> Select:
        return replace(self, sources=graphs)

    def from_named(self, *graphs: Iri) -> Select:
        return replace(self, named_sources=graphs)

    def limit(self, count: int) -> Select:
        return replace(self, _limit=_natural(count))

    def valid_at(self, instant: str) -> Select:
        return replace(self, _valid_at=_instant(instant))

    def as_of_tx(self, tx: int) -> Select:
        return replace(self, _tx=_natural(tx))

    def without_temporal(self) -> Select:
        """Explicitly remove transport-only temporal pins before saving."""
        return replace(self, _valid_at=None, _tx=None)

    def to_sparql(self) -> str:
        grouped = {v.name for v in self.grouping}
        if self.grouping or any(isinstance(p, Aggregate) for p in self.projections):
            if any(
                isinstance(p, Var) and p.name not in grouped for p in self.projections
            ):
                raise ValueError("non-aggregate projections must be in GROUP BY")
        out = (
            "SELECT "
            + ("DISTINCT " if self.distinct else "")
            + " ".join(p.sparql() for p in self.projections)
        )
        for keyword, graphs in (
            ("FROM", self.sources),
            ("FROM NAMED", self.named_sources),
        ):
            for graph in graphs:
                out += " " + keyword + " " + graph.sparql()
        out += " WHERE " + Group(self.patterns).sparql()
        if self.grouping:
            out += " GROUP BY " + " ".join(v.sparql() for v in self.grouping)
        if self._limit is not None:
            out += " LIMIT " + str(self._limit)
        return out

    def to_request(self) -> dict[str, Any]:
        out: dict[str, Any] = {"query": self.to_sparql()}
        if self._valid_at is not None:
            out["valid_at"] = self._valid_at
        if self._tx is not None:
            out["tx"] = self._tx
        return out

    def stored_definition(
        self, name: str, *, description: str = "", dataset: str | None = None
    ) -> dict[str, Any]:
        if self._valid_at is not None or self._tx is not None:
            raise ValueError(
                "stored definitions have no temporal fields; use without_temporal() explicitly, or execute the pinned request"
            )
        if not isinstance(name, str) or not re.fullmatch(
            r"[A-Za-z_][A-Za-z0-9_.-]*", name
        ):
            raise ValueError("invalid stored-query name")
        if not isinstance(description, str) or (
            dataset is not None and not isinstance(dataset, str)
        ):
            raise TypeError("description and dataset must be text")
        result = {
            "action": "load",
            "name": name,
            "description": description,
            "template": self.to_sparql(),
            "params": [],
        }
        if dataset is not None:
            result["dataset"] = dataset
        return result

    def execute(self, client: QuipuClient) -> dict[str, Any]:
        request = self.to_request()
        return client.query(request.pop("query"), **request)

    def save(
        self,
        client: QuipuClient,
        name: str,
        *,
        description: str = "",
        dataset: str | None = None,
    ) -> dict[str, Any]:
        return client.save_query(self, name, description=description, dataset=dataset)
