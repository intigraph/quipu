"""RDF term types for inputs and results.

Results follow the native JSON contract: a language-tagged or datatyped
literal arrives as :class:`Literal`; integers, floats and booleans arrive as
Python ``int``/``float``/``bool``; IRIs and plain strings both arrive as
``str`` because the native result format does not distinguish them (an open
fidelity row, not something this package guesses at).
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class IRI:
    """An IRI used as an object value (``Store.set``)."""

    value: str

    def __str__(self) -> str:
        return self.value


@dataclass(frozen=True)
class Literal:
    """A literal with a language tag or datatype."""

    value: str
    lang: str | None = None
    datatype: str | None = None

    def __str__(self) -> str:
        return self.value


def to_native(value: Any) -> Any:
    """Encode an object-position value in the native tagged form."""
    if isinstance(value, IRI):
        return {"iri": value.value}
    if isinstance(value, Literal):
        if value.lang is not None:
            return {"value": value.value, "lang": value.lang}
        if value.datatype is not None:
            return {"value": value.value, "datatype": value.datatype}
        return {"str": value.value}
    if isinstance(value, bool):
        return {"bool": value}
    if isinstance(value, int):
        return {"int": value}
    if isinstance(value, float):
        return {"float": value}
    if isinstance(value, str):
        return {"str": value}
    raise TypeError(f"{type(value).__name__} is not an RDF object value")


def from_native(value: Any) -> Any:
    """Decode one native result value into a Python value or :class:`Literal`."""
    if isinstance(value, dict) and "value" in value and ("lang" in value or "datatype" in value):
        return Literal(value["value"], value.get("lang"), value.get("datatype"))
    return value
