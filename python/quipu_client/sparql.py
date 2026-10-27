"""Typed SPARQL query builder (aegis-994; design: terminusdb-gap-analysis §G7).

Queries are built as data and serialized to standard SPARQL 1.1 text, so
programs compose them without string templating while every query stays
readable by any RDF tool. This is deliberately NOT WOQL: there is no second
query language here, only a typed way to write the first one.

Two Quipu-specific pieces sit OUTSIDE the SPARQL text, exactly where the server
keeps them:

- ``valid_at`` / ``as_of_tx`` are request-level pins on ``POST /query``
  (``valid_at`` and ``tx``), not syntax. The builder carries them and
  :meth:`QuipuClient.run` sends them; :meth:`Select.to_sparql` never emits them.
- :class:`Param` placeholders render as the stored-query template syntax
  (``<{name}>`` for an IRI, ``'{name}'`` for text, ``{name}`` for an int), so
  :meth:`Select.to_stored_query` produces a payload the governed ``queries``
  table validates at load (``src/store/queries.rs``).

Every name that reaches the text (variables, IRIs, prefixed names, function
names) is validated on construction, and literals are escaped, so no caller
value can change the query's shape.
"""

from __future__ import annotations

import re
from typing import Any, Iterable, Union

__all__ = [
    "Var",
    "IRI",
    "PName",
    "Literal",
    "Param",
    "Path",
    "Expr",
    "Select",
    "Ask",
    "triple",
    "optional",
    "union",
    "filter",
    "bind",
    "values",
    "graph",
    "minus",
    "seq",
    "alt",
    "inv",
    "zero_or_more",
    "one_or_more",
    "zero_or_one",
    "eq",
    "ne",
    "lt",
    "le",
    "gt",
    "ge",
    "and_",
    "or_",
    "not_",
    "add",
    "sub",
    "mul",
    "div",
    "in_",
    "not_in",
    "exists",
    "not_exists",
    "call",
    "count",
    "sum_",
    "avg",
    "min_",
    "max_",
    "sample",
    "group_concat",
    "asc",
    "desc",
    "STANDARD_PREFIXES",
]

# Prefixes declared automatically when (and only when) a query uses them.
STANDARD_PREFIXES: dict[str, str] = {
    "rdf": "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
    "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
    "xsd": "http://www.w3.org/2001/XMLSchema#",
    "owl": "http://www.w3.org/2002/07/owl#",
}

_VAR_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
_PREFIX_RE = re.compile(r"^([A-Za-z]([A-Za-z0-9_.-]*[A-Za-z0-9_-])?)?$")
_LOCAL_RE = re.compile(r"^([A-Za-z0-9_]([A-Za-z0-9_.-]*[A-Za-z0-9_-])?)?$")
_PARAM_RE = re.compile(r"^[A-Za-z0-9_]+$")
_PLACEHOLDER_RE = re.compile(r"\{[A-Za-z0-9_]+\}")
# Mirrors ParamKind::Iri's refusal set in src/mcp/named_query.rs.
_IRI_BAD = set('<>"{}|\\^`')
_LANG_RE = re.compile(r"^[A-Za-z]+(-[A-Za-z0-9]+)*$")
_PARAM_KINDS = ("iri", "text", "int")

# SPARQL 1.1 built-in calls (§17.4) the builder will emit by name. Anything
# else must be an IRI-named extension function, so a caller string can never
# smuggle syntax in through `call`.
_BUILTINS = frozenset(
    """STR LANG LANGMATCHES DATATYPE BOUND IRI URI BNODE RAND ABS CEIL FLOOR ROUND
    CONCAT STRLEN UCASE LCASE ENCODE_FOR_URI CONTAINS STRSTARTS STRENDS
    STRBEFORE STRAFTER YEAR MONTH DAY HOURS MINUTES SECONDS TIMEZONE TZ NOW UUID
    STRUUID MD5 SHA1 SHA256 SHA384 SHA512 COALESCE IF STRLANG STRDT SAMETERM
    ISIRI ISURI ISBLANK ISLITERAL ISNUMERIC REGEX SUBSTR REPLACE""".split()
)


class BuilderError(ValueError):
    """A builder input that cannot become valid SPARQL. Raised at
    construction, while the author is present, never at call time."""


# --------------------------------------------------------------------- terms


class Term:
    """Anything that serializes to one SPARQL term or expression."""

    def to_sparql(self) -> str:  # pragma: no cover - abstract
        raise NotImplementedError

    def _walk(self) -> Iterable["Term"]:
        yield self


class Var(Term):
    """A SPARQL variable. ``Var("x")`` and ``Var("?x")`` are the same."""

    def __init__(self, name: str):
        name = name[1:] if name.startswith(("?", "$")) else name
        if not _VAR_RE.match(name):
            raise BuilderError(f"invalid variable name {name!r}")
        self.name = name

    def to_sparql(self) -> str:
        return f"?{self.name}"

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Var) and other.name == self.name

    def __hash__(self) -> int:
        return hash(("var", self.name))

    def __repr__(self) -> str:
        return f"Var({self.name!r})"


class IRI(Term):
    """An absolute IRI, serialized as ``<...>``."""

    def __init__(self, value: str):
        if value.startswith("<") and value.endswith(">"):
            value = value[1:-1]
        if not value or any(c in _IRI_BAD or c.isspace() or ord(c) < 0x20 for c in value):
            raise BuilderError(f"invalid IRI {value!r}")
        self.value = value

    def to_sparql(self) -> str:
        return f"<{self.value}>"

    def __repr__(self) -> str:
        return f"IRI({self.value!r})"


class PName(Term):
    """A prefixed name such as ``foaf:Person``; its prefix must be declared
    on the query (or be one of :data:`STANDARD_PREFIXES`)."""

    def __init__(self, value: str):
        prefix, sep, local = value.partition(":")
        if not sep or not _PREFIX_RE.match(prefix) or not _LOCAL_RE.match(local):
            raise BuilderError(
                f"{value!r} is not a prefixed name; use IRI(...) for a full IRI"
            )
        self.prefix = prefix
        self.local = local

    def to_sparql(self) -> str:
        return f"{self.prefix}:{self.local}"

    def __repr__(self) -> str:
        return f"PName({self.prefix}:{self.local!r})"


class _A(Term):
    def to_sparql(self) -> str:
        return "a"


A = _A()


class Literal(Term):
    """An RDF literal. Python ``int``/``float``/``bool``/``str`` are coerced
    automatically everywhere except triple subjects and predicates; use
    ``Literal`` for a language tag or datatype."""

    def __init__(self, value: Any, *, datatype: Any = None, lang: str | None = None):
        if datatype is not None and lang is not None:
            raise BuilderError("a literal has a datatype or a language tag, not both")
        if lang is not None and not _LANG_RE.match(lang):
            raise BuilderError(f"invalid language tag {lang!r}")
        self.value = value
        self.datatype = _iri_like(datatype) if datatype is not None else None
        self.lang = lang

    @classmethod
    def datetime(cls, value: str) -> "Literal":
        """``"..."^^xsd:dateTime``."""
        return cls(value, datatype="xsd:dateTime")

    def to_sparql(self) -> str:
        v = self.value
        if self.datatype is None and self.lang is None:
            if isinstance(v, bool):
                return "true" if v else "false"
            if isinstance(v, int):
                return str(v)
            if isinstance(v, float):
                if v != v or v in (float("inf"), float("-inf")):
                    raise BuilderError("NaN/inf need an explicit xsd:double literal")
                return repr(v) if "e" in repr(v) or "." in repr(v) else f"{v}.0"
        text = '"' + _escape(str(v)) + '"'
        if self.lang is not None:
            return f"{text}@{self.lang}"
        if self.datatype is not None:
            return f"{text}^^{self.datatype.to_sparql()}"
        return text

    def _walk(self) -> Iterable[Term]:
        yield self
        if self.datatype is not None:
            yield self.datatype


def _escape(s: str) -> str:
    return (
        s.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
    )


class Param(Term):
    """A stored-query placeholder.

    ``kind`` is ``iri`` | ``text`` | ``int`` (the server's ParamKind). It
    serializes as the template syntax the server substitutes into:
    ``<{name}>``, ``'{name}'`` or ``{name}``. An optional param needs a
    ``default`` (the server refuses one without, see queries.rs)."""

    def __init__(
        self,
        name: str,
        kind: str = "iri",
        *,
        required: bool = True,
        default: Any = None,
        description: str = "",
    ):
        if not _PARAM_RE.match(name):
            raise BuilderError(f"invalid param name {name!r} (letters, digits, _)")
        if kind not in _PARAM_KINDS:
            raise BuilderError(f"param {name!r}: kind must be one of {_PARAM_KINDS}")
        if not required and default is None:
            raise BuilderError(
                f"param {name!r} is optional but has no default; the server "
                "refuses that at load (it would leave the placeholder verbatim)"
            )
        self.name = name
        self.kind = kind
        self.required = required
        self.default = None if default is None else str(default)
        self.description = description

    def to_sparql(self) -> str:
        if self.kind == "iri":
            return f"<{{{self.name}}}>"
        if self.kind == "text":
            return f"'{{{self.name}}}'"
        return f"{{{self.name}}}"

    def spec(self) -> dict[str, Any]:
        out: dict[str, Any] = {
            "name": self.name,
            "type": self.kind,
            "required": self.required,
            "description": self.description,
        }
        if self.default is not None:
            out["default"] = self.default
        return out

    def render(self, raw: Any) -> str:
        """Substitute a value exactly as the server's ``ParamKind::render``
        does, so a bound builder query and the stored one run the same text."""
        raw = str(raw)
        if self.kind == "iri":
            IRI(raw)  # same refusal set
            return f"<{raw}>"
        if self.kind == "text":
            return "'" + raw.replace("\\", "\\\\").replace("'", "\\'") + "'"
        try:
            return str(int(raw.strip()))
        except ValueError:
            raise BuilderError(f"param {self.name!r}: {raw!r} is not an integer") from None


def _iri_like(x: Any) -> Term:
    """Coerce something that must be an IRI-valued term (predicate,
    datatype, graph name): ``IRI``/``PName``/``Var``/``Param`` pass, ``"a"``
    is rdf:type, ``"<...>"`` is an IRI, ``"?x"`` a variable, ``"p:l"`` a
    prefixed name."""
    if isinstance(x, (IRI, PName, Var, Param, _A)):
        return x
    if isinstance(x, str):
        if x == "a":
            return A
        if x.startswith("<") and x.endswith(">"):
            return IRI(x)
        if x.startswith("?"):
            return Var(x)
        return PName(x)
    raise BuilderError(f"expected an IRI, prefixed name or variable, got {x!r}")


def _node(x: Any) -> Term:
    """Coerce a subject: like :func:`_iri_like` but ``"a"`` is not special."""
    if isinstance(x, str) and x == "a":
        raise BuilderError("'a' is only valid as a predicate")
    return _iri_like(x)


def _obj(x: Any) -> Term:
    """Coerce an object / expression operand. A bare ``str`` is still read as
    an IRI-ish name (``"?x"``, ``"ex:y"``, ``"<...>"``); a string literal must
    be ``Literal("...")`` so a value can never be mistaken for a name."""
    if isinstance(x, Term):
        return x
    if isinstance(x, (bool, int, float)):
        return Literal(x)
    if isinstance(x, str):
        return _node(x)
    raise BuilderError(f"cannot use {x!r} as a SPARQL term")


# --------------------------------------------------------------- paths


class Path(Term):
    """A SPARQL 1.1 property path (always fully parenthesized)."""

    def __init__(self, op: str, parts: list[Term]):
        self.op = op
        self.parts = parts

    def to_sparql(self) -> str:
        if self.op in ("/", "|"):
            return "(" + self.op.join(p.to_sparql() for p in self.parts) + ")"
        if self.op == "^":
            return f"(^{self.parts[0].to_sparql()})"
        return f"({self.parts[0].to_sparql()}){self.op}"

    def _walk(self) -> Iterable[Term]:
        yield self
        for p in self.parts:
            yield from p._walk()


def _path_part(x: Any) -> Term:
    t = x if isinstance(x, Path) else _iri_like(x)
    if isinstance(t, Var):
        raise BuilderError("a variable cannot appear inside a property path")
    return t


def seq(*steps: Any) -> Path:
    """``a/b/c``."""
    if len(steps) < 2:
        raise BuilderError("seq needs at least two steps")
    return Path("/", [_path_part(s) for s in steps])


def alt(*choices: Any) -> Path:
    """``a|b``."""
    if len(choices) < 2:
        raise BuilderError("alt needs at least two choices")
    return Path("|", [_path_part(s) for s in choices])


def inv(step: Any) -> Path:
    """``^a``."""
    return Path("^", [_path_part(step)])


def zero_or_more(step: Any) -> Path:
    """``a*``."""
    return Path("*", [_path_part(step)])


def one_or_more(step: Any) -> Path:
    """``a+``."""
    return Path("+", [_path_part(step)])


def zero_or_one(step: Any) -> Path:
    """``a?``."""
    return Path("?", [_path_part(step)])


# --------------------------------------------------------- expressions


class Expr(Term):
    """An expression tree node. Compose with the functions below, or with
    ``&`` (and), ``|`` (or) and ``~`` (not)."""

    def __init__(self, text_fn, children: list[Term]):
        self._text_fn = text_fn
        self.children = children

    def to_sparql(self) -> str:
        return self._text_fn([c.to_sparql() for c in self.children])

    def _walk(self) -> Iterable[Term]:
        yield self
        for c in self.children:
            yield from c._walk()

    def __and__(self, other: Any) -> "Expr":
        return and_(self, other)

    def __or__(self, other: Any) -> "Expr":
        return or_(self, other)

    def __invert__(self) -> "Expr":
        return not_(self)

    def as_(self, var: Any) -> "Aliased":
        """``(expr AS ?var)`` in a projection."""
        return Aliased(self, var)


class Aliased:
    def __init__(self, expr: Term, var: Any):
        self.expr = expr
        self.var = var if isinstance(var, Var) else Var(var)

    def to_sparql(self) -> str:
        return f"({self.expr.to_sparql()} AS {self.var.to_sparql()})"


def _binop(op: str):
    def make(a: Any, b: Any) -> Expr:
        return Expr(lambda t: f"({t[0]} {op} {t[1]})", [_obj(a), _obj(b)])

    make.__doc__ = f"``(a {op} b)``"
    return make


eq = _binop("=")
ne = _binop("!=")
lt = _binop("<")
le = _binop("<=")
gt = _binop(">")
ge = _binop(">=")
add = _binop("+")
sub = _binop("-")
mul = _binop("*")
div = _binop("/")


def and_(*xs: Any) -> Expr:
    """``(a && b && ...)``."""
    if len(xs) < 2:
        raise BuilderError("and_ needs at least two operands")
    return Expr(lambda t: "(" + " && ".join(t) + ")", [_obj(x) for x in xs])


def or_(*xs: Any) -> Expr:
    """``(a || b || ...)``."""
    if len(xs) < 2:
        raise BuilderError("or_ needs at least two operands")
    return Expr(lambda t: "(" + " || ".join(t) + ")", [_obj(x) for x in xs])


def not_(x: Any) -> Expr:
    """``(!a)``."""
    return Expr(lambda t: f"(!{t[0]})", [_obj(x)])


def in_(x: Any, options: Iterable[Any]) -> Expr:
    """``(x IN (a, b))``."""
    opts = [_obj(o) for o in options]
    return Expr(lambda t: f"({t[0]} IN ({', '.join(t[1:])}))", [_obj(x), *opts])


def not_in(x: Any, options: Iterable[Any]) -> Expr:
    """``(x NOT IN (a, b))``."""
    opts = [_obj(o) for o in options]
    return Expr(lambda t: f"({t[0]} NOT IN ({', '.join(t[1:])}))", [_obj(x), *opts])


def call(name: Any, *args: Any) -> Expr:
    """A SPARQL built-in by name (``call("regex", v, Literal("^a"))``), or an
    extension function named by an IRI / prefixed name."""
    if isinstance(name, str) and name.upper() in _BUILTINS:
        fname: Term | str = name.upper()
        return Expr(lambda t: f"{fname}({', '.join(t)})", [_obj(a) for a in args])
    fn = _iri_like(name)
    if isinstance(fn, (Var, Param, _A)):
        raise BuilderError(f"{name!r} is not a function name")
    return Expr(lambda t: f"{t[0]}({', '.join(t[1:])})", [fn, *(_obj(a) for a in args)])


class _Exists(Expr):
    def __init__(self, keyword: str, patterns: list["Pattern"]):
        self.keyword = keyword
        self.patterns = patterns
        super().__init__(lambda _t: "", [])

    def to_sparql(self) -> str:
        return f"{self.keyword} {_group(self.patterns, 0)}"

    def _walk(self) -> Iterable[Term]:
        yield self
        for p in self.patterns:
            yield from p._walk()


def exists(*patterns: Any) -> Expr:
    """``EXISTS { ... }``."""
    return _Exists("EXISTS", _patterns(patterns))


def not_exists(*patterns: Any) -> Expr:
    """``NOT EXISTS { ... }``."""
    return _Exists("NOT EXISTS", _patterns(patterns))


# ---------------------------------------------------------- aggregates


class Aggregate(Expr):
    pass


def _agg(name: str):
    def make(expr: Any, *, distinct: bool = False) -> Aggregate:
        d = "DISTINCT " if distinct else ""
        return Aggregate(lambda t: f"{name}({d}{t[0]})", [_obj(expr)])

    make.__doc__ = f"``{name}([DISTINCT] expr)``"
    return make


sum_ = _agg("SUM")
avg = _agg("AVG")
min_ = _agg("MIN")
max_ = _agg("MAX")
sample = _agg("SAMPLE")


def count(expr: Any = None, *, distinct: bool = False) -> Aggregate:
    """``COUNT(*)`` with no argument, else ``COUNT([DISTINCT] expr)``."""
    d = "DISTINCT " if distinct else ""
    if expr is None:
        return Aggregate(lambda _t: f"COUNT({d}*)", [])
    return Aggregate(lambda t: f"COUNT({d}{t[0]})", [_obj(expr)])


def group_concat(expr: Any, *, separator: str | None = None, distinct: bool = False) -> Aggregate:
    """``GROUP_CONCAT([DISTINCT] expr; SEPARATOR="...")``."""
    d = "DISTINCT " if distinct else ""
    sep = "" if separator is None else f'; SEPARATOR="{_escape(separator)}"'
    return Aggregate(lambda t: f"GROUP_CONCAT({d}{t[0]}{sep})", [_obj(expr)])


class _Order:
    def __init__(self, direction: str, expr: Any):
        self.direction = direction
        self.expr = _obj(expr)

    def to_sparql(self) -> str:
        return f"{self.direction}({self.expr.to_sparql()})"


def asc(expr: Any) -> _Order:
    return _Order("ASC", expr)


def desc(expr: Any) -> _Order:
    return _Order("DESC", expr)


# ------------------------------------------------------------- patterns


class Pattern:
    """One element of a group graph pattern."""

    def render(self, depth: int) -> str:  # pragma: no cover - abstract
        raise NotImplementedError

    def _walk(self) -> Iterable[Term]:  # pragma: no cover - abstract
        raise NotImplementedError


def _ind(depth: int) -> str:
    return "  " * depth


def _group(patterns: list[Pattern], depth: int) -> str:
    """``{ ... }`` — always with whitespace after the brace, so SPARQL group
    braces can never look like a ``{placeholder}`` to the server."""
    if not patterns:
        return "{ }"
    body = "\n".join(p.render(depth + 1) for p in patterns)
    return "{\n" + body + "\n" + _ind(depth) + "}"


def _patterns(items: Iterable[Any]) -> list[Pattern]:
    out = []
    for p in items:
        if not isinstance(p, Pattern):
            raise BuilderError(f"expected a pattern (triple, optional, ...), got {p!r}")
        out.append(p)
    return out


class _Triple(Pattern):
    def __init__(self, s: Any, p: Any, o: Any):
        self.s = _node(s)
        self.p = p if isinstance(p, Path) else _iri_like(p)
        self.o = _obj(o)
        if isinstance(self.s, (Literal, Expr)):
            raise BuilderError("a literal cannot be a triple subject")

    def render(self, depth: int) -> str:
        return f"{_ind(depth)}{self.s.to_sparql()} {self.p.to_sparql()} {self.o.to_sparql()} ."

    def _walk(self) -> Iterable[Term]:
        for t in (self.s, self.p, self.o):
            yield from t._walk()


def triple(s: Any, p: Any, o: Any) -> Pattern:
    """``s p o .`` — ``p`` may be a :class:`Path`."""
    return _Triple(s, p, o)


class _Keyworded(Pattern):
    def __init__(self, keyword: str, patterns: list[Pattern], head: Term | None = None):
        self.keyword = keyword
        self.patterns = patterns
        self.head = head

    def render(self, depth: int) -> str:
        head = f" {self.head.to_sparql()}" if self.head is not None else ""
        return f"{_ind(depth)}{self.keyword}{head} {_group(self.patterns, depth)}"

    def _walk(self) -> Iterable[Term]:
        if self.head is not None:
            yield from self.head._walk()
        for p in self.patterns:
            yield from p._walk()


def optional(*patterns: Any) -> Pattern:
    """``OPTIONAL { ... }``."""
    return _Keyworded("OPTIONAL", _patterns(patterns))


def minus(*patterns: Any) -> Pattern:
    """``MINUS { ... }``."""
    return _Keyworded("MINUS", _patterns(patterns))


def graph(name: Any, *patterns: Any) -> Pattern:
    """``GRAPH <g> { ... }`` (or ``GRAPH ?g``)."""
    return _Keyworded("GRAPH", _patterns(patterns), _iri_like(name))


class _Union(Pattern):
    def __init__(self, branches: list[list[Pattern]]):
        if len(branches) < 2:
            raise BuilderError("union needs at least two branches")
        self.branches = branches

    def render(self, depth: int) -> str:
        return _ind(depth) + " UNION ".join(_group(b, depth) for b in self.branches)

    def _walk(self) -> Iterable[Term]:
        for b in self.branches:
            for p in b:
                yield from p._walk()


def union(*branches: Any) -> Pattern:
    """``{ A } UNION { B }``. Each branch is a pattern or a list of them."""
    return _Union([_patterns(b if isinstance(b, (list, tuple)) else [b]) for b in branches])


class _Filter(Pattern):
    def __init__(self, expr: Term):
        self.expr = expr

    def render(self, depth: int) -> str:
        text = self.expr.to_sparql()
        if not text.startswith("("):
            text = f"({text})"
        return f"{_ind(depth)}FILTER{text}"

    def _walk(self) -> Iterable[Term]:
        yield from self.expr._walk()


def filter(expr: Any) -> Pattern:  # noqa: A001 - mirrors the SPARQL keyword
    """``FILTER(expr)``."""
    return _Filter(_obj(expr))


class _Bind(Pattern):
    def __init__(self, expr: Term, var: Var):
        self.expr = expr
        self.var = var

    def render(self, depth: int) -> str:
        return f"{_ind(depth)}BIND({self.expr.to_sparql()} AS {self.var.to_sparql()})"

    def _walk(self) -> Iterable[Term]:
        yield from self.expr._walk()
        yield self.var


def bind(expr: Any, var: Any) -> Pattern:
    """``BIND(expr AS ?var)``."""
    return _Bind(_obj(expr), var if isinstance(var, Var) else Var(var))


class _Values(Pattern):
    def __init__(self, var: Var, options: list[Term]):
        self.var = var
        self.options = options

    def render(self, depth: int) -> str:
        opts = " ".join(o.to_sparql() for o in self.options)
        return f"{_ind(depth)}VALUES {self.var.to_sparql()} {{ {opts} }}"

    def _walk(self) -> Iterable[Term]:
        yield self.var
        for o in self.options:
            yield from o._walk()


def values(var: Any, options: Iterable[Any]) -> Pattern:
    """``VALUES ?var { a b c }`` (one variable)."""
    opts = [_obj(o) for o in options]
    if any(isinstance(o, (Expr, Param, Path)) for o in opts):
        raise BuilderError("VALUES takes IRIs and literals only")
    return _Values(var if isinstance(var, Var) else Var(var), opts)


# ---------------------------------------------------------------- queries


Projection = Union[Var, Aliased, str]


class _Query:
    """Shared WHERE-clause, prefix and temporal-pin handling."""

    def __init__(self) -> None:
        self._prefixes: dict[str, str] = {}
        self._where: list[Pattern] = []
        self._valid_at: str | None = None
        self._as_of_tx: int | None = None
        self._graph: str | None = None

    # -- composition (each returns self, so calls chain)

    def prefix(self, name: str, iri: str) -> "_Query":
        if not _PREFIX_RE.match(name):
            raise BuilderError(f"invalid prefix {name!r}")
        self._prefixes[name] = IRI(iri).value
        return self

    def where(self, *patterns: Any) -> "_Query":
        self._where.extend(_patterns(patterns))
        return self

    def optional(self, *patterns: Any) -> "_Query":
        return self.where(optional(*patterns))

    def union(self, *branches: Any) -> "_Query":
        return self.where(union(*branches))

    def filter(self, expr: Any) -> "_Query":
        return self.where(filter(expr))

    # -- temporal and scope pins: request fields, never SPARQL text

    def valid_at(self, timestamp: str) -> "_Query":
        """Pin VALID time (``valid_at`` on ``POST /query``): only facts true
        of the world at this ISO-8601 instant."""
        if not isinstance(timestamp, str) or not timestamp.strip():
            raise BuilderError("valid_at must be a non-empty ISO-8601 string")
        self._valid_at = timestamp
        return self

    def as_of_tx(self, tx: int) -> "_Query":
        """Pin TRANSACTION time (``tx`` on ``POST /query``): what the store
        believed as of this transaction id."""
        if isinstance(tx, bool) or not isinstance(tx, int) or tx < 0:
            raise BuilderError("as_of_tx must be a non-negative int")
        self._as_of_tx = tx
        return self

    def in_graph(self, graph: str) -> "_Query":
        """Scope the default graph (``graph`` on ``POST /query``)."""
        self._graph = graph
        return self

    def request_fields(self) -> dict[str, Any]:
        """The ``POST /query`` pins this query carries (``valid_at``, ``tx``,
        ``graph``); empty when none are set."""
        out: dict[str, Any] = {}
        if self._valid_at is not None:
            out["valid_at"] = self._valid_at
        if self._as_of_tx is not None:
            out["tx"] = self._as_of_tx
        if self._graph is not None:
            out["graph"] = self._graph
        return out

    # -- serialization

    def _body(self) -> list[str]:  # pragma: no cover - abstract
        raise NotImplementedError

    def _terms(self) -> Iterable[Term]:
        for p in self._where:
            yield from p._walk()

    def params(self) -> list[Param]:
        """Every distinct :class:`Param` used, in first-use order."""
        seen: dict[str, Param] = {}
        for t in self._terms():
            if isinstance(t, Param):
                prior = seen.get(t.name)
                if prior is None:
                    seen[t.name] = t
                elif prior.spec() != t.spec():
                    raise BuilderError(f"param {t.name!r} is declared twice with different specs")
        return list(seen.values())

    def _prefix_lines(self) -> list[str]:
        used = []
        for t in self._terms():
            if isinstance(t, PName) and t.prefix not in used:
                used.append(t.prefix)
        lines = []
        for name in used:
            iri = self._prefixes.get(name, STANDARD_PREFIXES.get(name))
            if iri is None:
                raise BuilderError(f"prefix {name!r} is used but not declared; call .prefix()")
            lines.append(f"PREFIX {name}: <{iri}>")
        return lines

    def template(self) -> str:
        """The SPARQL text with any :class:`Param` placeholders left in."""
        return "\n".join(self._prefix_lines() + self._body())

    def to_sparql(self, **params: Any) -> str:
        """Executable SPARQL. Params are substituted exactly as the server's
        stored-query renderer would; an optional param falls back to its
        default, a missing required one is an error."""
        text = self.template()
        for p in self.params():
            if p.name in params:
                raw = params.pop(p.name)
            elif p.default is not None:
                raw = p.default
            else:
                raise BuilderError(f"param {p.name!r} is required")
            text = text.replace(p.to_sparql(), p.render(raw))
        if params:
            raise BuilderError(f"unknown params: {sorted(params)}")
        return text

    def __str__(self) -> str:
        return self.template()

    def to_stored_query(
        self,
        name: str,
        *,
        description: str = "",
        dataset: str | None = None,
    ) -> dict[str, Any]:
        """The ``POST /queries`` / ``quipu_queries`` ``load`` payload.

        Refuses a query that carries ``valid_at``/``as_of_tx``/graph pins:
        those are request fields, ``/ask`` does not take them, and storing the
        text alone would silently drop the pin. Scope a stored query with
        ``dataset=`` instead."""
        pins = self.request_fields()
        if pins:
            raise BuilderError(
                f"cannot store a query pinned with {sorted(pins)}: stored queries "
                "carry SPARQL text only and /ask takes no temporal pins. Drop the "
                "pin (and scope with dataset= for a graph)."
            )
        params = self.params()
        template = self.template()
        # The server finds placeholders by scanning for `{word}`; one that is
        # not a Param (inside a literal, say) would fail load as an undeclared
        # placeholder. Say so here, naming the real cause.
        stray = set(_PLACEHOLDER_RE.findall(template)) - {
            "{" + p.name + "}" for p in params
        }
        if stray:
            raise BuilderError(
                f"template text contains {sorted(stray)}, which the server would "
                "read as undeclared placeholders"
            )
        payload: dict[str, Any] = {
            "action": "load",
            "name": name,
            "description": description,
            "template": template,
            "params": [p.spec() for p in params],
        }
        if dataset is not None:
            payload["dataset"] = dataset
        return payload


class Select(_Query):
    """``SELECT`` — the projection is variables (``"?x"`` or ``Var``) and
    ``expr.as_("name")`` aliases; no projection means ``SELECT *``."""

    def __init__(self, *projection: Projection, distinct: bool = False):
        super().__init__()
        self._projection: list[Var | Aliased] = [
            p if isinstance(p, (Var, Aliased)) else Var(p) for p in projection
        ]
        self._distinct = distinct
        self._group_by: list[Term] = []
        self._having: list[Term] = []
        self._order_by: list[_Order] = []
        self._limit: int | Param | None = None
        self._offset: int | Param | None = None

    def group_by(self, *exprs: Any) -> "Select":
        self._group_by.extend(_obj(e) for e in exprs)
        return self

    def having(self, expr: Any) -> "Select":
        self._having.append(_obj(expr))
        return self

    def order_by(self, *keys: Any) -> "Select":
        self._order_by.extend(k if isinstance(k, _Order) else asc(k) for k in keys)
        return self

    def limit(self, n: int | Param) -> "Select":
        self._limit = _count_arg(n, "limit")
        return self

    def offset(self, n: int | Param) -> "Select":
        self._offset = _count_arg(n, "offset")
        return self

    def _terms(self) -> Iterable[Term]:
        for p in self._projection:
            if isinstance(p, Aliased):
                yield from p.expr._walk()
        yield from super()._terms()
        for t in self._group_by + self._having:
            yield from t._walk()
        for o in self._order_by:
            yield from o.expr._walk()
        for n in (self._limit, self._offset):
            if isinstance(n, Param):
                yield n

    def _body(self) -> list[str]:
        head = "SELECT " + ("DISTINCT " if self._distinct else "")
        head += " ".join(p.to_sparql() for p in self._projection) if self._projection else "*"
        lines = [head, "WHERE " + _group(self._where, 0)]
        if self._group_by:
            lines.append("GROUP BY " + " ".join(_group_key(g) for g in self._group_by))
        if self._having:
            lines.append("HAVING " + " ".join(_paren(h) for h in self._having))
        if self._order_by:
            lines.append("ORDER BY " + " ".join(o.to_sparql() for o in self._order_by))
        if self._limit is not None:
            lines.append(f"LIMIT {_count_text(self._limit)}")
        if self._offset is not None:
            lines.append(f"OFFSET {_count_text(self._offset)}")
        return lines


class Ask(_Query):
    """``ASK { ... }``."""

    def _body(self) -> list[str]:
        return ["ASK " + _group(self._where, 0)]


def _count_arg(n: Any, what: str) -> int | Param:
    if isinstance(n, Param):
        if n.kind != "int":
            raise BuilderError(f"{what} param must have kind 'int'")
        return n
    if isinstance(n, bool) or not isinstance(n, int) or n < 0:
        raise BuilderError(f"{what} must be a non-negative int or an int Param")
    return n


def _count_text(n: int | Param) -> str:
    return n.to_sparql() if isinstance(n, Param) else str(n)


def _paren(t: Term) -> str:
    s = t.to_sparql()
    return s if s.startswith("(") else f"({s})"


def _group_key(t: Term) -> str:
    return t.to_sparql() if isinstance(t, Var) else _paren(t)
