#!/usr/bin/env python3
"""Private corpus controls for Bobbin's copied pointer-only release query.

# arming: agent-invoked; run before publishing the repository-pendant source.
"""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from rdflib import Graph, Literal, Namespace, RDF, RDFS, URIRef

A = Namespace("http://aegis.gastown.local/ontology/")
CODE = Namespace(str(A) + "code/quipu/")
DOC = Namespace(str(A) + "doc/quipu/")
QUERY = (Path(__file__).resolve().parents[1] / "queries/repository-share-quipu.rq").read_text()
OWL = Namespace("http://www.w3.org/2002/07/owl#")
W3ID = "https://w3id.org/intigraph/"
IDEA = "https://quipu.dev/knowledge/"


def public(node):
    """The public name the release query gives an internal entity IRI."""
    text = str(node)
    for kind in ("code", "doc", "chunk"):
        prefix = f"{A}{kind}/"
        if text.startswith(prefix):
            repo, rest = text[len(prefix):].split("/", 1)
            return URIRef(f"{W3ID}{repo}/id/{kind}/{rest}")
    if text.startswith(f"{A}repo_"):
        return URIRef(f"{W3ID}{text[len(str(A)) + 5:]}/id/repo")
    if text.startswith(IDEA) and not text.startswith(IDEA + "publication-policy/"):
        return URIRef(f"{W3ID}quipu/id/idea/{text[len(IDEA):]}")
    return node


def published(triples):
    """Rename subjects and non-class objects, and link each renamed subject back."""
    out = set()
    for s, p, o in triples:
        new = public(s)
        obj = o if p == RDF.type or not isinstance(o, URIRef) else public(o)
        out.add((new, p, obj))
        if new != s:
            out.add((new, OWL.sameAs, s))
    return out


class RepositoryQuery(unittest.TestCase):
    def test_pointer_corpus_and_private_boundaries(self):
        source = Graph()
        doc, chunk = DOC["README.md"], URIRef(str(A) + "chunk/quipu/README.md#C1")
        expected = {
            (doc, RDF.type, A.Document),
            (doc, A.filePath, Literal("README.md")),
            (chunk, RDF.type, A.Chunk),
            (chunk, A.inDocument, doc),
            (chunk, A.chunkOrder, Literal(0)),
            (chunk, A.mentions, Literal("Store")),
            (CODE["store.rs"], A.filePath, Literal("src/store.rs")),
        }
        for triple in expected:
            source.add(triple)
        blocked = {
            (chunk, A.content, Literal("body must not be exported")),
            (chunk, A.startLine, Literal(1)),
            (chunk, A.endLine, Literal(30)),
            (chunk, A.mentions, URIRef("https://example.org/resolved")),
            (DOC["private"], RDFS.label, Literal("aegis-private123")),
            (DOC["private"], RDF.type, A.Document),
            (URIRef(str(A) + "doc/other/README.md"), A.filePath, Literal("foreign")),
            (URIRef("urn:quipu:memory:private"), RDFS.label, Literal("memory")),
        }
        for triple in blocked:
            source.add(triple)
        result = set(source.query(QUERY).graph)
        self.assertEqual(result, published(expected))
        # No internal entity name is published as a subject or object.
        for s, p, o in result:
            self.assertFalse(str(s).startswith(str(A)), s)
            if p != OWL.sameAs and isinstance(o, URIRef) and p != RDF.type:
                self.assertFalse(str(o).startswith(str(A)), o)

    def test_ideas_and_sssom_records_survive_without_identity_knots(self):
        source = Graph()
        S = Namespace("https://w3id.org/sssom/")
        record = URIRef("https://example.org/fixture/mapping")
        pointer = DOC["guide.md#idea"]
        other = URIRef("https://example.org/other/idea")
        expected = {
            (pointer, RDF.type, A.Concept),
            (pointer, RDFS.comment, Literal("Curated high-level idea")),
            (pointer, A.reviewAfter, Literal("2026-11-01T00:00:00Z")),
            (record, RDF.type, OWL.Axiom),
            (record, S.subject_id, pointer),
            (record, S.object_id, other),
            (record, S.predicate_id, URIRef("http://www.w3.org/2004/02/skos/core#closeMatch")),
            (record, S.mapping_justification, URIRef("https://w3id.org/semapv/vocab/LexicalMatching")),
        }
        for triple in expected:
            source.add(triple)
        # A source identity knot stays out; only the generated rename link ships.
        source.add((pointer, OWL.sameAs, other))
        self.assertEqual(set(source.query(QUERY).graph), published(expected))

    def test_empty_is_empty_with_populated_positive(self):
        self.assertEqual(len(Graph().query(QUERY).graph), 0)
        source = Graph()
        triple = (DOC["README.md"], A.filePath, Literal("README.md"))
        source.add(triple)
        self.assertEqual(set(source.query(QUERY).graph), published({triple}))

    def test_public_names_are_the_same_in_every_repository(self):
        # Cross-repository references must land on the name the other
        # repository's pendant publishes, and context nodes keep one identity.
        source = Graph()
        Q = Namespace(IDEA)
        foreign = URIRef(str(A) + "doc/bobbin/README.md")
        directive = A["repo-embedded-knowledge-databases-directive-2026-09-03"]
        policy = URIRef(IDEA + "publication-policy/private-keys")
        for triple in [
            (DOC["README.md"], RDFS.seeAlso, foreign),
            (A.repo_quipu, RDF.type, A.GitRepo),
            (directive, A.applies_to, A.repo_quipu),
            (Q.vision, RDF.type, Q.Vision),
            (Q.vision, Q.guides, Q["single-writer"]),
            (Q["single-writer"], Q.governs, CODE["src%2Fstore.rs"]),
            (policy, RDF.type, A.InternalIdentifierPattern),
            (policy, RDFS.label, Literal("Private key headers")),
            (CODE["src%2Fstore.rs"], A.mentions, Literal(str(A) + "doc/quipu/x")),
        ]:
            source.add(triple)
        result = set(source.query(QUERY).graph)
        new_directive = URIRef(W3ID + "stack/id/repo-embedded-knowledge-databases-directive-2026-09-03")
        repo = URIRef(W3ID + "quipu/id/repo")
        self.assertIn((URIRef(W3ID + "quipu/id/doc/README.md"), RDFS.seeAlso,
                       URIRef(W3ID + "bobbin/id/doc/README.md")), result)
        self.assertIn((repo, OWL.sameAs, A.repo_quipu), result)
        self.assertIn((new_directive, A.applies_to, repo), result)
        self.assertIn((new_directive, OWL.sameAs, directive), result)
        vision = URIRef(W3ID + "quipu/id/idea/vision")
        self.assertIn((vision, RDF.type, Q.Vision), result)
        self.assertIn((vision, Q.guides, URIRef(W3ID + "quipu/id/idea/single-writer")), result)
        self.assertIn((URIRef(W3ID + "quipu/id/idea/single-writer"), Q.governs,
                       URIRef(W3ID + "quipu/id/code/src%2Fstore.rs")), result)
        # Review-pinned receiver policy names and literals are not renamed.
        self.assertIn((policy, RDFS.label, Literal("Private key headers")), result)
        self.assertIn((URIRef(W3ID + "quipu/id/code/src%2Fstore.rs"), A.mentions,
                       Literal(str(A) + "doc/quipu/x")), result)


    def test_build_admits_only_exact_former_name_links(self):
        spec = importlib.util.spec_from_file_location(
            "share_builder", Path(__file__).resolve().parents[1] / "scripts/build-repository-share.py")
        builder = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(builder)
        own = URIRef(str(A) + "doc/quipu/README.md")
        source = Graph()
        source.add((own, RDF.type, A.Document))
        source.add((own, A.filePath, Literal("README.md")))
        new = "https://w3id.org/intigraph/quipu/id/doc/README.md"
        same = "<http://www.w3.org/2002/07/owl#sameAs>"
        with tempfile.TemporaryDirectory() as work:
            export = Path(work) / "export.nt"
            export.write_bytes(source.query(QUERY).graph.serialize(format="nt", encoding="utf-8"))
            self.assertEqual(builder.verify_former_names(export), 1)
            refused = {
                "arbitrary": f"<{new}> {same} <https://example.org/other> .",
                "mismatched path": f"<{new}> {same} <{A}doc/quipu/OTHER.md> .",
                "other repository": f"<{new}> {same} <{A}doc/quipu/README.md> .",
                "reverse": f"<{own}> {same} <{new}> .",
                "public to public": f"<{new}> {same} <https://w3id.org/intigraph/quipu/id/doc/README.md> .",
                "second link": f"<{new}> {same} <{own}> .\n<{new}> {same} <{own}> .",
            }
            for name, line in refused.items():
                with self.subTest(name=name):
                    export.write_text(line + "\n")
                    with self.assertRaises(ValueError):
                        builder.verify_former_names(export)

if __name__ == "__main__":
    unittest.main()
