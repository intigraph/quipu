#!/usr/bin/env python3
"""Offline counterexamples for RDF changes and uncertain producer outcomes."""
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

from rdflib import Graph

spec = importlib.util.spec_from_file_location('propagation', Path(__file__).parent / 'check-shape-propagation.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
PREFIX = '''@prefix x: <https://example.org/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
'''
SHAPE = 'x:S a sh:NodeShape; sh:targetClass x:C; sh:property [sh:path x:p; sh:minCount 1] .'


def graph(body=''):
    return Graph().parse(data=PREFIX + body, format='turtle')


class Shapes(unittest.TestCase):
    def test_blank_nodes_and_prefix_spelling_do_not_create_change(self):
        self.assertEqual(m.changes(graph(SHAPE), graph(SHAPE)), [])
        other = graph('x:S sh:property _:different; a sh:NodeShape; sh:targetClass x:C . '
                      '_:different sh:minCount 1; sh:path x:p .')
        self.assertEqual(m.changes(graph(SHAPE), other), [])


    def test_literal_constraint_and_anonymous_shape_changes_are_detected(self):
        rows = m.changes(graph(SHAPE), graph(SHAPE.replace('minCount 1', 'minCount 2')))
        self.assertEqual({r['iri'] for r in rows}, {'https://example.org/S', 'https://example.org/C', 'https://example.org/p'})
        rows = m.changes(graph(SHAPE.replace('x:S', '[]')), graph(SHAPE.replace('x:S', '[]').replace('minCount 1', 'minCount 2')))
        self.assertTrue(rows)


    def test_add_remove_and_real_change_reverted(self):
        added = m.changes(graph(), graph(SHAPE))
        removed = m.changes(graph(SHAPE), graph())
        self.assertTrue(all(r['change'] == 'added' for r in added))
        self.assertTrue(all(r['change'] == 'removed' for r in removed))
        self.assertEqual(m.changes(graph(), graph()), [])

    def test_all_target_forms_detect_anonymous_and_named_constraint_drift(self):
        for target in ('targetClass', 'targetNode', 'targetSubjectsOf', 'targetObjectsOf'):
            for typed in ('a sh:NodeShape; ', ''):
                for subject in ('[]', 'x:S'):
                    with self.subTest(target=target, typed=typed, subject=subject):
                        before = f'{subject} {typed}sh:{target} x:record; sh:nodeKind sh:IRI .'
                        rows = m.changes(graph(before), graph(before.replace('sh:IRI', 'sh:BlankNode')))
                        self.assertTrue(rows)
                        self.assertIn('_:anonymous-NodeShapes' if subject == '[]' else
                                      'https://example.org/S', {r['iri'] for r in rows})

    def test_anonymous_collection_ignores_labels_but_preserves_multiplicity(self):
        first = '_:one a sh:NodeShape; sh:targetNode x:record; sh:property _:p . '
        prop = '_:p sh:nodeKind sh:IRI .'
        other = '_:different sh:property _:q; sh:targetNode x:record; a sh:NodeShape . '
        self.assertEqual(m.changes(graph(first + prop), graph('_:q sh:nodeKind sh:IRI . ' + other)), [])
        shape = '[] a sh:NodeShape; sh:targetNode x:record; sh:nodeKind sh:IRI . '
        rows = m.changes(graph(shape), graph(shape + shape))
        self.assertEqual([r['iri'] for r in rows], ['_:anonymous-NodeShapes'])
        self.assertEqual(rows[0]['change'], 'modified')
        untargeted = '[] a sh:NodeShape; sh:nodeKind sh:IRI .'
        self.assertTrue(m.changes(graph(untargeted), graph(untargeted.replace('sh:IRI', 'sh:BlankNode'))))

    def test_anonymous_collection_requires_exact_current_evidence(self):
        shape = '[] a sh:NodeShape; sh:targetNode x:record; sh:nodeKind sh:IRI .'
        delta = m.changes(graph(shape), graph(shape.replace('sh:IRI', 'sh:BlankNode')))
        self.assertEqual(len(delta), 1)
        row = {'iri': delta[0]['iri'], 'definition_sha256': delta[0]['after']['sha256'],
               'disposition': 'quipu-local', 'reason': 'fixture-only collection',
               'waiver': {'receipt': 'fixture-review', 'reviewer': 'independent', 'reason': 'fixture'}}
        self.assertEqual(m.audit(delta, {'entries': [row]}, graph(), Path('.'))[0]['gaps'], [])
        row['definition_sha256'] = delta[0]['before']['sha256']
        self.assertIn('missing-or-stale-definition-evidence',
                      m.audit(delta, {'entries': [row]}, graph(), Path('.'))[0]['gaps'])

    def test_targeted_property_shape_keeps_property_classification(self):
        shape = '[] a sh:PropertyShape; sh:targetNode x:record; sh:path x:p; sh:minCount 1 .'
        rows = m.changes(graph(shape), graph(shape.replace('minCount 1', 'minCount 2')))
        self.assertEqual([r['iri'] for r in rows], ['https://example.org/p'])
        self.assertEqual(rows[0]['after']['kinds'], ['property'])


    def test_exact_term_cq_and_reviewed_waiver(self):
        delta = m.changes(graph(), graph(SHAPE))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'query.json').write_text('{"query":"ASK { ?s ?p ?o }"}')
            digest = hashlib.sha256((root/'query.json').read_bytes()).hexdigest()
            terms = graph('x:Term a rdfs:Class, <http://www.w3.org/1999/02/22-rdf-syntax-ns#Property> .')
            rows = [{'iri': r['iri'], 'definition_sha256': r['after']['sha256'],
                     'disposition': 'mapped', 'term': 'https://example.org/Term',
                     'cq': {'path': 'query.json', 'sha256': digest}} for r in delta]
            self.assertTrue(all(not r['gaps'] for r in m.audit(delta, {'entries': rows}, terms, root)))
            (root/'query.json').write_text('changed')
            self.assertTrue(all('cq-missing-or-stale' in r['gaps'] for r in m.audit(delta, {'entries': rows}, terms, root)))
            for r in rows:
                r.pop('cq'); r['disposition'] = 'quipu-local'; r['reason'] = 'engine guarantee'
                r['waiver'] = {'receipt': 'fixture-review', 'reviewer': 'independent', 'reason': 'fixture only'}
            self.assertTrue(all(not r['gaps'] for r in m.audit(delta, {'entries': rows}, terms, root)))
            rows[0]['definition_sha256'] = 'stale'
            self.assertIn('missing-or-stale-definition-evidence', m.audit(delta, {'entries': rows}, terms, root)[0]['gaps'])


    def test_escape_and_duplicate_catalogue_refused(self):
        delta = m.changes(graph(), graph(SHAPE))
        row = {'iri': delta[0]['iri']}
        with self.assertRaises(ValueError):
            m.audit(delta, {'entries': [row, row]}, graph(), Path('.'))

    def test_class_cannot_borrow_property_only_term(self):
        delta = m.changes(graph(), graph('x:Class a rdfs:Class .'))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            query = root/'query.json'; query.write_text('{"query":"ASK { ?s ?p ?o }"}')
            row = {'iri': delta[0]['iri'], 'definition_sha256': delta[0]['after']['sha256'],
                   'disposition': 'mapped', 'term': 'https://example.org/Term',
                   'cq': {'path': 'query.json', 'sha256': hashlib.sha256(query.read_bytes()).hexdigest()}}
            wrong = graph('x:Term a <http://www.w3.org/1999/02/22-rdf-syntax-ns#Property> .')
            self.assertIn('mapped-term-wrong-kind', m.audit(delta, {'entries': [row]}, wrong, root)[0]['gaps'])
            right = graph('x:Term a rdfs:Class .')
            self.assertEqual(m.audit(delta, {'entries': [row]}, right, root)[0]['gaps'], [])


if __name__ == '__main__': unittest.main()
