"""Read-only assessment of bounded, supplied native-reader evidence; no transport."""
from datetime import datetime, timezone
import re

from .contract import readiness, validate

MAX_AGE_SECONDS = 300
PLAN = ['quiesce-scoped-writer', 'restore-test-backup', 'install-alias-readers-and-shapes',
        'prove-old-and-canonical-fixtures', 'deploy-content-bound-writer',
        'verify-one-native-canary', 'resume-original-schedule']


def timestamp(value):
    result = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if result.tzinfo is None:
        raise ValueError('timezone is required')
    return result


def assess(record, digest, evidence, candidate_id=None, artifact_sha256=None, now=None):
    result = {'schema_version': 'breaking-doctor/v1', 'record_id': record.get('id') if isinstance(record, dict) else None,
              'record_sha256': digest, 'outcome': 'UNKNOWN', 'reason': '', 'plan': [],
              'evidence_trust': 'supplied-unverified', 'release_authorized': False}
    try:
        validate(record)
        missing = readiness(record)
        if missing:
            raise ValueError('record incomplete: ' + ', '.join(missing))
        if not isinstance(candidate_id, str) or not re.fullmatch('[0-9a-f]{64}', candidate_id):
            raise ValueError('selected candidate digest required')
        if not isinstance(artifact_sha256, str) or not re.fullmatch('[0-9a-f]{64}', artifact_sha256):
            raise ValueError('selected installed artifact digest required')
        keys = {'schema_version', 'record_id', 'record_sha256', 'candidate_id', 'observed_at',
                'scope', 'read_status', 'complete', 'truncated', 'positive_controls',
                'expected_event_id', 'reader_terms', 'rows'}
        if not isinstance(evidence, dict) or set(evidence) != keys:
            raise ValueError('unsupported observation shape')
        if (evidence['schema_version'] != 'breaking-observation/v1'
                or evidence['record_id'] != record['id'] or evidence['record_sha256'] != digest
                or evidence['candidate_id'] != candidate_id):
            raise ValueError('record/candidate evidence binding mismatch')
        try:
            observed = timestamp(evidence['observed_at'])
        except (ValueError, TypeError, AttributeError):
            raise ValueError('invalid observation timestamp') from None
        at = now or datetime.now(timezone.utc)
        age = (at - observed).total_seconds()
        if not 0 <= age <= MAX_AGE_SECONDS:
            raise ValueError('observation is stale or in the future')
        if (evidence['read_status'] != 'success' or evidence['complete'] is not True
                or evidence['truncated'] is not False):
            raise ValueError('native read/control/scope incomplete')
        scope = evidence['scope']
        fields = {'tool', 'installed_version', 'artifact_sha256', 'platform', 'datasets', 'subject_iri'}
        if not isinstance(scope, dict) or set(scope) != fields:
            raise ValueError('unknown installed scope')
        if (not isinstance(scope['datasets'], list) or not scope['datasets']
                or any(not isinstance(v, str) or not v for v in scope['datasets'])
                or len(set(scope['datasets'])) != len(scope['datasets'])):
            raise ValueError('explicit unique datasets required')
        if not isinstance(scope['subject_iri'], str) or not scope['subject_iri']:
            raise ValueError('known subject identity required')
        controls = evidence['positive_controls']
        if (not isinstance(controls, list) or len(controls) != len(scope['datasets'])
                or any(not isinstance(c, dict) or set(c) != {'dataset', 'count'}
                       or type(c['count']) is not int or c['count'] < 1 for c in controls)
                or {c['dataset'] for c in controls} != set(scope['datasets'])):
            raise ValueError('each selected dataset needs a populated control')
        matching = [s for s in record['affected'] if s['tool'] == scope['tool']]
        if len(matching) != 1 or scope['installed_version'] not in matching[0]['versions']:
            raise ValueError('installed version is outside declared detection coverage')
        targets = [t for t in record['targets'] if t['tool'] == scope['tool']
                   and t['version'] == scope['installed_version']
                   and scope['platform'] in t['platforms']]
        if len(targets) != 1 or scope['artifact_sha256'] != artifact_sha256:
            raise ValueError('installed artifact/platform is not content-bound')
        expected = evidence['expected_event_id']
        rows = evidence['rows']
        required = set(record['detection']['canonical_terms'] + record['detection']['legacy_terms'])
        if not isinstance(expected, str) or not expected:
            raise ValueError('known input event identity required')
        if not isinstance(rows, list) or len(rows) != 1:
            raise ValueError('expected exactly one identity binding; duplicates refuse')
        row = rows[0]
        if (not isinstance(row, dict) or set(row) != {'dataset', 'subject', 'predicate', 'value'}
                or row['dataset'] not in scope['datasets'] or row['subject'] != scope['subject_iri']
                or row['predicate'] not in required or row['value'] != expected):
            raise ValueError('native identity differs or uses an unknown term')
        terms = evidence['reader_terms']
        if (not isinstance(terms, list) or not terms
                or any(not isinstance(t, str) for t in terms)
                or len(set(terms)) != len(terms)):
            raise ValueError('reader term coverage is unknown')
        if required.issubset(terms):
            result.update(outcome='NOT_AFFECTED', reason='exact scoped identity and dual reader coverage')
        else:
            result.update(outcome='AFFECTED', reason='reader omits canonical or historical term coverage')
            result['plan'] = [{'operation': record['correction']['operation'],
                               'packet_ref': record['correction']['packet_ref'],
                               'packet_sha256': record['correction']['packet_sha256'],
                               'steps': PLAN, 'scope': scope,
                               'requires_separate_admission': True, 'executable': False}]
    except (ValueError, TypeError, KeyError, OverflowError) as error:
        result['reason'] = str(error) if isinstance(error, ValueError) else 'malformed evidence'
    return result
