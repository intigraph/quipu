"""Seeded edit ledger: expected facts come from declared operations, not merge output."""
import json
import random

NS = "https://example.test/merge-load/"
TYPE = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"
LABEL = "http://www.w3.org/2000/01/rdf-schema#label"
STATUS, TAG, ENTITY = NS + "status", NS + "tag", NS + "Entity"
SSSOM = "https://w3id.org/sssom/"
AXIOM = "http://www.w3.org/2002/07/owl#Axiom"
SHAPES = f'''@prefix sh: <http://www.w3.org/ns/shacl#> .
<{NS}entity-shape> a sh:NodeShape ; sh:targetClass <{ENTITY}> ;
 sh:property [ sh:path <{STATUS}> ; sh:maxCount 1 ] .
<{NS}mapping-shape> a sh:NodeShape ; sh:targetClass <{AXIOM}> ;
 sh:property [ sh:path <{SSSOM}subject_id> ; sh:maxCount 1 ] ;
 sh:property [ sh:path <{SSSOM}object_id> ; sh:maxCount 1 ] .
'''


def iri(s):
    return f"<{s}>"


def literal(s):
    return json.dumps(s, ensure_ascii=True)


def render(facts):
    return "".join(f"<{s}> <{p}> {o} .\n" for s, p, o in sorted(facts))


def scenario(n, replicas, seed):
    rng = random.Random(seed)
    conflicts = max(1, n // 10)
    base, edits = set(), []
    for side in range(replicas):
        operations = []
        kinds = ["add", "retract", "replace"] * ((n + 2) // 3)
        rng.shuffle(kinds)
        for i in range(n):
            subject = NS + (f"shared/{i}" if i < conflicts else f"replica/{side}/{i}")
            base.add((subject, TYPE, iri(ENTITY)))
            base.add((subject, LABEL, literal(subject.removeprefix(NS))))
            base.add((subject, STATUS, literal("base")))
            base.add((subject, TAG, literal("old")))
            kind = "conflict" if i < conflicts else kinds[i]
            remove, add = set(), set()
            if kind in ("replace", "conflict"):
                remove.add((subject, STATUS, literal("base")))
                add.add((subject, STATUS, literal(f"side-{side}")))
            elif kind == "retract":
                remove.add((subject, TAG, literal("old")))
            else:
                add.add((subject, TAG, literal(f"new-{side}")))
                # Existing SSSOM RDF predicates; synthetic mappings, never a
                # claim of approved real repository identity or inference.
                m = NS + f"mapping/{side}/{i}"
                add.update({(m, TYPE, iri(AXIOM)),
                            (m, SSSOM + "subject_id", iri(subject)),
                            (m, SSSOM + "object_id", iri(NS + f"other-repo/{side}/{i}")),
                            (m, SSSOM + "predicate_id", iri("http://www.w3.org/2004/02/skos/core#closeMatch")),
                            (m, SSSOM + "mapping_justification", iri("https://w3id.org/semapv/vocab/ManualMappingCuration"))})
            operations.append({"kind": kind, "remove": remove, "add": add})
        edits.append(operations)
    return base, edits, {(NS + f"shared/{i}", STATUS) for i in range(conflicts)}


def apply(facts, operations):
    result = set(facts)
    for op in operations:
        result.difference_update(op["remove"])
        result.update(op["add"])
    return result


def oracle(base, sides, hold=False):
    # The workload deliberately overlaps ONLY declared conflict operations.
    # The reference policy holds these at base or picks replica zero; all
    # other explicitly disjoint edits execute once, including pure removals.
    chosen = [op for side, ops in enumerate(sides) for op in ops
              if op["kind"] != "conflict" or (side == 0 and not hold)]
    return apply(base, chosen)


def edit_request(db, operations):
    return {"op": "edit", "db": str(db),
            "remove": sorted({(s, p) for op in operations for s, p, _ in op["remove"]}),
            "insert": render({f for op in operations for f in op["add"]})}
