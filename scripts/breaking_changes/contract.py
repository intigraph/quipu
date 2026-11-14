"""Strict local-schema validation and structural release readiness (not approval)."""
import hashlib
import json
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[2]
MAX_BYTES = 1024 * 1024
OPERATIONS = {'managed-reader-first-writer-upgrade/v1'}


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('duplicate JSON key')
        value[key] = item
    return value


def load(path):
    path = Path(path)
    if path.is_symlink() or not path.is_file():
        raise ValueError('input must be a regular, non-symlink JSON file')
    with path.open('rb') as stream:
        raw = stream.read(MAX_BYTES + 1)
    if len(raw) > MAX_BYTES:
        raise ValueError('JSON input exceeds one MiB')
    try:
        value = json.loads(raw, object_pairs_hook=unique_object)
    except RecursionError:
        raise ValueError('JSON nesting exceeds supported depth') from None
    return value, hashlib.sha256(raw).hexdigest()


def validate(record):
    schema, _ = load(ROOT / 'releases/schemas/breaking-change-v1.schema.json')
    Draft202012Validator.check_schema(schema)
    errors = list(Draft202012Validator(schema, format_checker=FormatChecker()).iter_errors(record))
    if errors:
        # Never echo untrusted field values or an entire supplied observation.
        raise ValueError('record violates breaking-change/v1: ' + errors[0].validator)
    detection = record['detection']
    if set(detection['canonical_terms']) & set(detection['legacy_terms']):
        raise ValueError('canonical and historical term sets overlap')
    tools = [scope['tool'] for scope in record['affected']]
    if len(tools) != len(set(tools)):
        raise ValueError('duplicate affected tool')
    if record['correction']['operation'] not in OPERATIONS:
        raise ValueError('unsupported correction operation')
    keys = [(t['tool'], t['version'], p) for t in record['targets'] for p in t['platforms']]
    if len(keys) != len(set(keys)):
        raise ValueError('ambiguous installed target scope')
    return record


def readiness(record):
    """Check completeness only; external receipt authenticity belongs to promotion."""
    validate(record)
    missing = []
    for name in ['review_ref', 'approval_ref']:
        if not record[name]:
            missing.append(name)
    if record['status'] != 'ready':
        missing.append('status=ready')
    if not record['targets']:
        missing.append('targets')
    if any(not scope['versions'] for scope in record['affected']):
        missing.append('affected exact versions')
    declared = {(s['tool'], v) for s in record['affected'] for v in s['versions']}
    covered = {(t['tool'], t['version']) for t in record['targets']}
    if declared != covered:
        missing.append('target coverage for every affected version')
    if not record['correction']['packet_sha256'] or not record['correction']['packet_ref']:
        missing.append('reviewed correction packet')
    if not record['rollback']['packet_sha256'] or not record['rollback']['packet_ref']:
        missing.append('reviewed rollback packet')
    return missing


def records():
    paths = sorted((ROOT / 'releases/changes').glob('*.json'))
    if not paths:
        raise ValueError('no breaking-change records')
    result = [(path, validate(load(path)[0])) for path in paths]
    ids = [record['id'] for _, record in result]
    if len(set(ids)) != len(ids):
        raise ValueError('duplicate breaking-change ID')
    return result
