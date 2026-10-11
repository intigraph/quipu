"""Schema-first controls; run with uv --with rdflib --with pyshacl."""
import unittest
from pathlib import Path
from rdflib import Graph, Literal, Namespace, URIRef
from rdflib.namespace import RDF, RDFS
from pyshacl import validate

A = Namespace("http://aegis.gastown.local/ontology/")
N = URIRef("urn:test:instruction-fragment")
SHAPES = Graph().parse(Path(__file__).with_name("aegis-ontology.shapes.ttl"))

def record(scope="fleet", selector=""):
    g = Graph()
    for p, o in [(RDF.type, A.InstructionFragment), (RDFS.label, Literal("Fixture")),
                 (A.instructionScope, Literal(scope)), (A.instructionSelector, Literal(selector)),
                 (RDF.value, Literal("Read current work before acting."))]:
        g.add((N, p, o))
    return g

def conforms(g):
    return validate(g, shacl_graph=SHAPES)[0]

class InstructionFragmentTests(unittest.TestCase):
    def test_four_scopes(self):
        for scope, selector in [("fleet", ""), ("role", "keeper"), ("agent", "fixture"), ("harness", "codex")]:
            with self.subTest(scope=scope):
                self.assertTrue(conforms(record(scope, selector)))

    def test_required_fields(self):
        for p in [RDFS.label, A.instructionScope, A.instructionSelector, RDF.value]:
            with self.subTest(predicate=p):
                g = record(); g.remove((N, p, None))
                self.assertFalse(conforms(g))

    def test_scalar_cardinality_and_datatype(self):
        for p in [A.instructionScope, A.instructionSelector, RDF.value]:
            with self.subTest(predicate=p):
                g = record(); g.add((N, p, Literal("extra")))
                self.assertFalse(conforms(g))
                g = record(); g.remove((N, p, None)); g.add((N, p, URIRef("urn:wrong:datatype")))
                self.assertFalse(conforms(g))

    def test_scope_selector_contract(self):
        for scope, selector in [("fleet", "\n"), ("fleet", " "), ("fleet", "role"),
                                ("role", ""), ("agent", ""), ("harness", ""), ("unknown", "fixture")]:
            with self.subTest(scope=scope, selector=selector):
                self.assertFalse(conforms(record(scope, selector)))

    def test_empty_body(self):
        g = record(); g.remove((N, RDF.value, None)); g.add((N, RDF.value, Literal("")))
        self.assertFalse(conforms(g))

    def test_other_class_not_targeted(self):
        g = Graph(); g.add((N, RDF.type, A.ConfigFile)); g.add((N, RDFS.label, Literal("Control")))
        self.assertTrue(conforms(g))

if __name__ == "__main__":
    unittest.main()
