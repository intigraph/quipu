#!/usr/bin/env python3
"""Validate/render release corrections or print a read-only, evidence-bound plan."""
import argparse
import json
import sys

sys.dont_write_bytecode = True

from breaking_changes.contract import ROOT, load, readiness, records, validate
from breaking_changes.doctor import assess, timestamp
from breaking_changes.render import OUTPUTS, check_references, render


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('test')
    sub.add_parser('demo')
    check = sub.add_parser('validate')
    check.add_argument('record', nargs='?')
    check.add_argument('--expected-id', help='ID from the selected candidate binding')
    check.add_argument('--expected-sha256', help='file SHA256 from the selected candidate binding')
    check.add_argument('--release', action='store_true', help='require structural readiness, not authorization')
    view = sub.add_parser('render')
    mode = view.add_mutually_exclusive_group()
    mode.add_argument('--check', action='store_true')
    mode.add_argument('--write', action='store_true', help='author the two repository documentation outputs')
    doctor = sub.add_parser('doctor', help='read local record/evidence and print a non-executable plan')
    doctor.add_argument('record')
    doctor.add_argument('evidence')
    doctor.add_argument('--candidate-id', required=True, help='selected candidate digest; externally verified')
    doctor.add_argument('--artifact-sha256', required=True, help='externally verified installed artifact digest')
    doctor.add_argument('--at', help='explicit UTC replay time; otherwise current time')
    args = parser.parse_args()
    try:
        if args.command == 'demo':
            import runpy
            runpy.run_path(str(ROOT / 'docs/demos/breaking-changes/demo.py'), run_name='__main__')
            return 0
        if args.command == 'test':
            import unittest
            suite = unittest.defaultTestLoader.loadTestsFromName('test_breaking_changes')
            return 0 if unittest.TextTestRunner().run(suite).wasSuccessful() else 1
        if args.command == 'validate':
            if (args.expected_id is None) != (args.expected_sha256 is None):
                raise ValueError('expected ID and SHA256 must be supplied together')
            if args.expected_id is not None and not args.record:
                raise ValueError('candidate binding requires one selected record')
            if args.record:
                record, digest = load(args.record)
                selected = [(args.record, validate(record), digest)]
            else:
                selected = records()
            results = []
            for _, record, digest in selected:
                if args.expected_id is not None and (record['id'] != args.expected_id
                                                      or digest != args.expected_sha256):
                    raise ValueError('candidate record ID/digest mismatch')
                missing = readiness(record)
                results.append({'id': record['id'], 'sha256': digest, 'missing': missing,
                                'approval_verified': False, 'release_authorized': False})
            print(json.dumps(results, indent=2))
            return 1 if args.release and any(r['missing'] for r in results) else 0
        if args.command == 'render':
            body = render()
            if args.check:
                check_references()
                if any(p.is_symlink() or not p.is_file() or p.read_text() != body for p in OUTPUTS):
                    raise ValueError('generated breaking-change documentation is stale')
            elif args.write:
                for path in OUTPUTS:
                    if path.is_symlink() or not path.resolve().is_relative_to(ROOT.resolve()):
                        raise ValueError('documentation output escapes the repository')
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(body)
            else:
                print(body, end='')
            return 0
        record, digest = load(args.record)
        evidence, _ = load(args.evidence)
        result = assess(record, digest, evidence, args.candidate_id, args.artifact_sha256,
                        timestamp(args.at) if args.at else None)
        print(json.dumps(result, indent=2))
        return {'NOT_AFFECTED': 0, 'AFFECTED': 1, 'UNKNOWN': 2}[result['outcome']]
    except (ValueError, OSError, TypeError, KeyError) as error:
        if args.command == 'doctor':
            # Missing/unreadable inputs must produce UNKNOWN, not a zero or a raw dump.
            print(json.dumps({'schema_version': 'breaking-doctor/v1', 'outcome': 'UNKNOWN',
                              'reason': 'invalid or unavailable record/evidence',
                              'plan': [], 'release_authorized': False}))
            return 2
        print('ERROR: ' + str(error) if isinstance(error, ValueError) else 'ERROR: input unavailable',
              file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
