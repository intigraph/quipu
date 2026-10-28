"""Native builder case emitter used by cross-language and engine controls."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from quipu_client import (
    Aggregate,
    Compare,
    Filter,
    Group,
    Iri,
    Literal,
    Optional,
    Path as PropertyPath,
    Select,
    Triple,
    Union,
    Var,
)


def cases():
    s, label, n, target, kind = map(Var, ("s", "label", "n", "target", "type"))
    return {
        "simple": Select((s, label)).where(Triple(s, Iri("urn:label"), label)),
        "optional": Select((s, Var("next"))).where(
            Triple(s, Iri("urn:type"), Iri("urn:Thing")),
            Optional(Group((Triple(s, Iri("urn:link"), Var("next")),))),
        ),
        "union": Select((s,)).where(
            Union(
                (
                    Group((Triple(s, Iri("urn:label"), Literal("Alpha")),)),
                    Group((Triple(s, Iri("urn:label"), Literal("Beta")),)),
                )
            )
        ),
        "filter": Select((s,)).where(
            Triple(s, Iri("urn:score"), n), Filter(Compare(n, ">", Literal(1)))
        ),
        "path": Select((s, target)).where(
            Triple(
                s,
                PropertyPath(
                    "/", (Iri("urn:link"), PropertyPath("*", (Iri("urn:link"),)))
                ),
                target,
            )
        ),
        "aggregate": Select((kind, Aggregate("COUNT", s, Var("total"))))
        .where(Triple(s, Iri("urn:type"), kind))
        .group_by(kind),
        "scoped": Select((s,))
        .where(Triple(s, Iri("urn:type"), Iri("urn:Thing")))
        .from_graph(Iri("urn:missing")),
    }


def literal_query(value):
    s, v = Var("s"), Var("value")
    return Select((v,)).where(
        Triple(s, Iri("urn:value"), v), Filter(Compare(v, "=", Literal(value)))
    )


if __name__ == "__main__":
    if "--literal" in sys.argv:
        print(
            json.dumps(
                literal_query(json.load(sys.stdin)).to_request(), ensure_ascii=False
            )
        )
    else:
        print(json.dumps({name: q.to_request() for name, q in cases().items()}))
