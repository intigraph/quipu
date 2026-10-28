"""Immutable SPARQL terms and expressions; no raw-expression escape hatch."""

from __future__ import annotations

from dataclasses import dataclass
import json
import re


class Expr:
    def sparql(self) -> str:
        raise NotImplementedError


def text(value: str) -> str:
    if not isinstance(value, str) or any(0xD800 <= ord(c) <= 0xDFFF for c in value):
        raise ValueError("expected Unicode scalar text")
    return value


def name(value: str) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", value):
        raise ValueError("expected an ASCII variable name without ?")
    return value


@dataclass(frozen=True)
class Var(Expr):
    name: str

    def __post_init__(self):
        name(self.name)

    def sparql(self) -> str:
        return "?" + self.name


@dataclass(frozen=True)
class Iri(Expr):
    value: str

    def __post_init__(self):
        text(self.value)
        if not re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", self.value) or any(
            c.isspace() or ord(c) < 33 or c in '<>"{}|^`\\' for c in self.value
        ):
            raise ValueError(
                "expected an absolute IRI, not a prefix or SPARQL fragment"
            )

    def sparql(self) -> str:
        return "<" + self.value + ">"


@dataclass(frozen=True)
class Literal(Expr):
    value: str | int | bool
    datatype: Iri | None = None
    language: str | None = None

    def __post_init__(self):
        if type(self.value) not in (str, int, bool):
            raise TypeError(
                "literal values are text, integers or booleans; use a typed lexical string for other datatypes"
            )
        if isinstance(self.value, str):
            text(self.value)
        if self.datatype is not None and not isinstance(self.datatype, Iri):
            raise TypeError("datatype must be an Iri")
        if self.language is not None and (
            not isinstance(self.language, str)
            or not re.fullmatch(r"[A-Za-z]+(?:-[A-Za-z0-9]+)*", self.language)
        ):
            raise ValueError("invalid language tag")
        if self.datatype is not None and self.language is not None:
            raise ValueError("datatype and language are mutually exclusive")
        if type(self.value) is not str and (
            self.datatype is not None or self.language is not None
        ):
            raise ValueError("datatype/language requires a lexical string")

    def sparql(self) -> str:
        if type(self.value) is bool:
            return "true" if self.value else "false"
        if type(self.value) is int:
            return str(self.value)
        # SPARQL accepts these JSON escapes. Escape braces too: the stored
        # registry scans {ident} placeholders even inside string literals.
        out = (
            json.dumps(self.value, ensure_ascii=False)
            .replace("{", r"\u007B")
            .replace("}", r"\u007D")
        )
        if self.datatype is not None:
            out += "^^" + self.datatype.sparql()
        if self.language is not None:
            out += "@" + self.language
        return out


def expression(value: Expr) -> Expr:
    if not isinstance(value, Expr):
        raise TypeError("expected a typed SPARQL expression")
    return value


@dataclass(frozen=True)
class Compare(Expr):
    left: Expr
    op: str
    right: Expr

    def __post_init__(self):
        expression(self.left)
        expression(self.right)
        if self.op not in (
            "=",
            "!=",
            "<",
            ">",
            "<=",
            ">=",
            "&&",
            "||",
            "+",
            "-",
            "*",
            "/",
        ):
            raise ValueError("unsupported expression operator")

    def sparql(self) -> str:
        return f"({self.left.sparql()} {self.op} {self.right.sparql()})"


@dataclass(frozen=True)
class Call(Expr):
    function: str
    args: tuple[Expr, ...]

    def __post_init__(self):
        object.__setattr__(self, "args", tuple(self.args))
        arities = {
            "STR": (1,),
            "LANG": (1,),
            "DATATYPE": (1,),
            "BOUND": (1,),
            "CONTAINS": (2,),
            "STRSTARTS": (2,),
            "STRENDS": (2,),
            "LCASE": (1,),
            "UCASE": (1,),
            "ISIRI": (1,),
            "ISLITERAL": (1,),
            "REGEX": (2, 3),
        }
        if self.function not in arities or len(self.args) not in arities[self.function]:
            raise ValueError("unsupported function or wrong arity")
        for arg in self.args:
            expression(arg)
        if self.function == "BOUND" and not isinstance(self.args[0], Var):
            raise TypeError("BOUND requires a Var")

    def sparql(self) -> str:
        return self.function + "(" + ", ".join(a.sparql() for a in self.args) + ")"


@dataclass(frozen=True)
class Path:
    op: str
    args: tuple[Iri | Path, ...]

    def __post_init__(self):
        object.__setattr__(self, "args", tuple(self.args))
        if self.op not in ("/", "|", "^", "*", "+", "?"):
            raise ValueError("unsupported path operator")
        if len(self.args) < 2 if self.op in ("/", "|") else len(self.args) != 1:
            raise ValueError("wrong path arity")
        if not all(isinstance(a, (Iri, Path)) for a in self.args):
            raise TypeError("paths contain only Iri or Path nodes")

    def sparql(self) -> str:
        if self.op in ("/", "|"):
            return "(" + self.op.join(a.sparql() for a in self.args) + ")"
        inner = "(" + self.args[0].sparql() + ")"
        return "^" + inner if self.op == "^" else inner + self.op


@dataclass(frozen=True)
class Aggregate:
    function: str
    value: Expr | None
    alias: Var
    distinct: bool = False

    def __post_init__(self):
        if self.function not in ("COUNT", "SUM", "AVG", "MIN", "MAX", "SAMPLE"):
            raise ValueError("unsupported aggregate")
        if self.value is None and self.function != "COUNT":
            raise ValueError("only COUNT accepts *")
        if self.value is not None:
            expression(self.value)
        if not isinstance(self.alias, Var) or type(self.distinct) is not bool:
            raise TypeError("aggregate alias must be Var and distinct must be bool")

    def sparql(self) -> str:
        inner = "*" if self.value is None else self.value.sparql()
        return f"({self.function}({'DISTINCT ' if self.distinct else ''}{inner}) AS {self.alias.sparql()})"
