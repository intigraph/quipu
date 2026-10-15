"""Behavioral controls for portable tools on the native MCP acceptance store.

# arming: library — invoked by test-native-mcp.py on isolated fixture stores.
"""
import hashlib
import json
from pathlib import Path
import time


def exercise(call, tool_body, unpack, token):
    def good(name, args=None, authenticated=False):
        response = call(name, args or {}, token if authenticated else None)
        assert unpack(response).get('isError') is not True, (name, response)
        return tool_body(response)

    def refused(name, args, authenticated=False):
        response = call(name, args, token if authenticated else None)
        assert 'error' in response or unpack(response).get('isError') is True, (name, response)

    graph = 'urn:fixture:portable'
    good('quipu_graph_create', {'graph': graph}, True)
    update = {'update': 'INSERT DATA { GRAPH <' + graph + '> { <urn:portable> <urn:p> "a+b&c=é" ; <urn:link> <urn:target> } }',
              'actor': 'urn:declared:workflow', 'source': 'workflow+fixture'}
    refused('quipu_update', update)
    good('quipu_update', update, True)
    query = {'query': 'SELECT ?v WHERE { <urn:portable> <urn:p> ?v }', 'graph': graph}
    assert good('quipu_query', query)['count'] == 1
    assert good('quipu_query', {'query': query['query']})['count'] == 0
    refused('quipu_reason', {'graph': False}, True)
    refused('quipu_share', {'scope': {'kind': 'graph', 'value': graph}, 'destination': 'internal'})
    refused('quipu_share', {})
    assert good('quipu_subscriptions', {'action':'list'}, True)['count'] == 0
    refused('quipu_subscriptions', {'action':'create','consumer_id':'fixture','webhook_url':'https://example.org/never-delivered','sparql_ask':'ASK {}'}, True)
    assert good('quipu_subscriptions', {'action':'list'}, True)['count'] == 0
    assert good('quipu_health')['status'] == 'ok'
    assert good('quipu_version')['git_sha']
    assert good('quipu_stats')['store_id']
    capability = good('quipu_workflow_capabilities')
    assert isinstance(capability['enabled'], bool)

    events = good('quipu_events', {'since': 0, 'limit': 2})
    assert 0 < len(events['events']) <= 2 and events['next_offset'] > 0
    refused('quipu_events_commit', {'consumer_id': 'portable', 'offset': events['next_offset']})
    good('quipu_events_commit', {'consumer_id': 'portable', 'offset': events['next_offset']}, True)
    assert good('quipu_events', {'consumer': 'portable', 'limit': 1})['committed_offset'] == events['next_offset']
    assert good('quipu_events', {'consumer': 'portable', 'since': 0, 'limit': 2})['next_offset'] == events['next_offset']
    changes = good('quipu_changes', {'graph': graph, 'since': 0, 'limit': 10})
    assert changes['records'], changes
    assert good('quipu_transactions', {'since': 0, 'limit': 1})['transactions']
    good('quipu_entity', {'iri': 'urn:one', 'expanded': True})
    assert good('quipu_entity_history', {'iri': 'urn:one'})['count'] > 0
    good('quipu_fragments', {'subject': 'urn:one', 'page': 1, 'pageSize': 2})
    assert good('quipu_reconcile')
    good('quipu_spotlight', {'text': 'no registered labels here', 'confidence': 0.5})

    rules = ('@prefix rule: <http://quipu.local/rule#> . '
             '<urn:fixture:copy> a rule:Rule ; rule:id "portable-copy" ; '
             'rule:head "<urn:q>(?a, ?b)" ; rule:body "<urn:link>(?a, ?b)" .')
    reason = good('quipu_reason', {'rules': rules, 'graph': graph}, True)
    assert reason['asserted'] == 1, reason
    assert good('quipu_query', {'query': 'SELECT ?v WHERE { <urn:portable> <urn:q> ?v }', 'graph': graph + '#inferred'})['count'] == 1
    assert good('quipu_query', {'query': 'SELECT ?v WHERE { <urn:portable> <urn:q> ?v }'})['count'] == 0
    # Outward sharing must refuse without a loaded identifier-policy catalogue.
    refused('quipu_share', {'scope': {'kind':'graph','value':graph}, 'no_shapes':True})
    good('quipu_shapes', {'action':'load','name':'identifier-policy',
         'turtle':'@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:policy:Shape> a sh:NodeShape ; sh:targetClass <http://aegis.gastown.local/ontology/InternalIdentifierPattern> .'}, True)
    good('quipu_graph_create', {'graph':'urn:fixture:catalogue'}, True)
    catalogue = (Path(__file__).resolve().parents[1] / 'tests/fixtures/share-catalogue.ttl').read_text()
    good('quipu_knot', {'graph':'urn:fixture:catalogue','turtle':catalogue}, True)
    share = good('quipu_share', {'scope': {'kind': 'graph', 'value': graph}, 'no_shapes': True})
    assert share['manifest']['scope']['value'] == graph
    assert '<urn:portable>' in share['files']['export.nt']
    acquisition = good('quipu_acquire', {'source_store_id': share['manifest']['store_id'], 'graphs': [graph]})
    assert acquisition['source_receipt']['complete'] is True
    assert acquisition['graphs'][graph]
    refused('quipu_acquire', {'source_store_id': 'wrong-lineage', 'graphs': [graph]})
    # This private store has no composed providers: check the documented refusal,
    # not a fabricated composed receipt or a ROOT fallback.
    refused('quipu_acquire_composed', {'source_store_id': 'wrong-lineage', 'graphs': [graph]})
    staged = good('quipu_import', {'manifest': share['manifest'], 'export_ntriples': share['files']['export.nt'],
                                  'shapes_turtle': share['files']['shapes.ttl'], **({'queries_turtle':share['files']['queries.ttl']} if 'queries.ttl' in share['files'] else {}), 'source': 'portable-fixture'}, True)
    assert staged['share_id'] == share['manifest']['share_id']
    # Promotion of unknown identity must refuse; no bypass of staged review.
    refused('quipu_import_promote', {'share_id': 'unknown-share'}, True)
    refused('quipu_search_query', {'mode': 'invalid', 'sparql': 'SELECT ?s WHERE {?s ?p ?o}'})
    # Backfill requires configured embedding authority; no network provider is
    # configured in these fixtures. Check the real response, never call it success.
    assert good('quipu_embed_backfill', {}, True)['status'] == 'error'
    missing_provider = call('quipu_embed_backfill_graph', {'graph': graph, 'max_entities': 1}, token)
    assert unpack(missing_provider).get('isError') is True
    assert 'no embedding provider' in tool_body(missing_provider)['error']

    snapshot_graph = 'urn:fixture:snapshot'
    good('quipu_graph_create', {'graph': snapshot_graph}, True)
    payload = '<urn:staged> <urn:p> "snapshot" .'
    content_hash = 'sha256:' + hashlib.sha256(payload.encode()).hexdigest()
    snapshot = 'urn:fixture:snapshot-source'
    upload_id = 'sha256:' + hashlib.sha256((snapshot + '\n' + content_hash).encode()).hexdigest()
    part = {'upload_id': upload_id, 'snapshot': snapshot, 'content_hash': content_hash,
            'part_hash': content_hash, 'payload': payload, 'total_parts': 1,
            'total_bytes': len(payload.encode()), 'part_number': 0, 'graph': snapshot_graph}
    good('quipu_knot_stage', part, True)
    good('quipu_knot_stage', part, True)  # exact retry
    for _ in range(100):
        result = good('quipu_knot_promote', {'upload_id': upload_id}, True)
        if not result.get('pending'):
            break
        time.sleep(0.05)
    else:
        raise AssertionError('snapshot promotion did not finish')
    assert good('quipu_query', {'graph': snapshot_graph, 'query': 'SELECT ?v WHERE {<urn:staged> <urn:p> ?v}'})['count'] == 1
    print('MCP workflows: scoped update/reason and ROOT controls, encoded GETs, cursor commit/replay, portable share/import, fenced acquisition, snapshot stage/promote and provider refusals PASS')
