#!/usr/bin/env python3
"""Classify a diff by ci/change-classes.yml and gate on the class's required jobs.

  change-class.py [--base SHA --head SHA] [--head-ref REF] [--paths P ...] [--json]
      Prints the classification. Default output is GITHUB_OUTPUT lines:
      classes=..., full=..., required=..., and one <stem>_<job>=true|false per
      job. --json prints the same as one JSON object (for the admission lane).
      Base/head default to PR_BASE/PR_HEAD; head ref to PR_HEAD_REF.

  change-class.py gate --workflow ci --required ID,ID,...|all
      The aggregate gate. Reads toJSON(needs) from NEEDS_JSON. Passes when every
      required job succeeded and every other job succeeded or was skipped.
      A failed or cancelled job always fails, required or not.

Every uncertainty fails toward MORE coverage: no base/head, a git error, an
unreadable map or an unmatched path all classify as `full`.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
MAP = ROOT / 'ci' / 'change-classes.yml'
SHA = re.compile(r'[0-9a-f]{40}')
VERSION = re.compile(r'"\d+\.\d+\.\d+[^"]*"')
LONE_VERSION = re.compile(r'^\s*version\s*=\s*"V"\s*$')
PATH_DEP_VERSION = re.compile(r'^.*\bpath\s*=.*\bversion\s*=\s*"V".*$')


def load_map(path=MAP):
    import yaml  # imported late so a missing module fails toward `full`
    doc = yaml.safe_load(Path(path).read_text())
    if doc.get('version') != 1:
        raise ValueError('unsupported change-class map version')
    return doc


def glob_regex(glob):
    """`**` crosses directories, `*` stays within one path segment."""
    out, i = [], 0
    while i < len(glob):
        if glob.startswith('**', i):
            out.append('.*')
            i += 2
        elif glob[i] == '*':
            out.append('[^/]*')
            i += 1
        else:
            out.append(re.escape(glob[i]))
            i += 1
    return re.compile(''.join(out) + r'\Z')


def path_class(cmap, path):
    for rule in cmap['rules']:
        if any(glob_regex(g).match(path) for g in rule['globs']):
            return rule['class']
    return 'full'


def required_for(cmap, cls):
    req = cmap['classes'][cls]['required']
    return list(cmap['jobs']) if req == 'all' else list(req)


def version_only(diff_text):
    """True when the manifest hunks change nothing but version strings."""
    removed, added = Counter(), Counter()
    for line in diff_text.splitlines():
        if line.startswith(('---', '+++')) or line[:1] not in ('-', '+'):
            continue
        body = VERSION.sub('"V"', line[1:])
        if not (LONE_VERSION.match(body) or PATH_DEP_VERSION.match(body)):
            return False
        (removed if line[0] == '-' else added)[body] += 1
    return removed == added


def is_release(cmap, paths, head_ref, manifest_diff):
    spec = cmap['classes'].get('release')
    if not spec or not head_ref or not head_ref.startswith(spec['head_ref_prefix']):
        return False
    if not paths or not set(paths) <= set(spec['paths']):
        return False
    if manifest_diff is None:
        return False
    return version_only(manifest_diff)


def classify(cmap, paths, head_ref=None, manifest_diff=None):
    paths = [p for p in paths if p]
    if not paths:
        classes = ['full']  # an empty diff is missing evidence, not a free pass
    elif is_release(cmap, paths, head_ref, manifest_diff):
        classes = ['release']
    else:
        classes = sorted({path_class(cmap, p) for p in paths})
        if 'full' in classes:
            classes = ['full']
    required = set()
    for cls in classes:
        required.update(required_for(cmap, cls))
    ordered = [job for job in cmap['jobs'] if job in required]
    return {'classes': classes, 'full': classes == ['full'], 'required': ordered,
            'bypassed': [job for job in cmap['jobs'] if job not in required]}


def full_result(cmap=None):
    jobs = list(cmap['jobs']) if cmap else []
    return {'classes': ['full'], 'full': True, 'required': jobs, 'bypassed': []}


def git(*args):
    result = subprocess.run(['git', *args], capture_output=True, timeout=30, cwd=ROOT)
    if result.returncode != 0:
        raise OSError('git ' + args[0] + ' failed')
    return result.stdout.decode('utf-8', errors='strict')


def diff_evidence(cmap, base, head):
    paths = git('diff', '--name-only', '-z', base, head).split('\0')
    manifests = [p for p in cmap['classes'].get('release', {}).get('paths', [])
                 if p.endswith(('.toml', '.lock'))]
    manifest_diff = git('diff', '-U0', base, head, '--', *manifests) if manifests else ''
    return paths, manifest_diff


def for_event(cmap, result, event):
    """Drop jobs the event defers to the other event from the gate's required set."""
    deferred = set((cmap.get('events') or {}).get(event, {}).get('deferred', [])) if cmap else set()
    if not deferred:
        return result
    return dict(result, required=[j for j in result['required'] if j not in deferred],
                deferred=sorted(deferred))


def evaluate(base, head, head_ref, paths=None):
    try:
        cmap = load_map()
    except Exception:  # noqa: BLE001 - any map failure keeps full coverage
        return None, full_result()
    if paths is not None:
        return cmap, classify(cmap, paths, head_ref, manifest_diff=None)
    if not all(SHA.fullmatch(ref or '') for ref in (base, head)):
        return cmap, full_result(cmap)
    try:
        found, manifest_diff = diff_evidence(cmap, base, head)
    except (OSError, subprocess.TimeoutExpired, UnicodeError):
        return cmap, full_result(cmap)
    return cmap, classify(cmap, found, head_ref, manifest_diff)


def output_key(job):
    return re.sub(r'[^A-Za-z0-9]', '_', job)


def render(cmap, result):
    lines = ['classes=' + ','.join(result['classes']),
             'full=' + str(result['full']).lower(),
             # `all`, not a list, so a map that failed to load still gates everything.
             'required=' + ('all' if cmap is None else ','.join(result['required']))]
    for job in (cmap['jobs'] if cmap else []):
        lines.append(output_key(job) + '=' + str(result['full'] or job in result['required']).lower())
    return '\n'.join(lines) + '\n'


def gate(needs, workflow, required):
    """Return a list of refusals; empty means the aggregate passes."""
    refusals = []
    if required == ['all']:
        wanted = set(needs)
    else:
        wanted = {job.split(':', 1)[1] for job in required if job.startswith(workflow + ':')}
    for name in sorted(wanted - set(needs)):
        refusals.append(f'{name}: required by the change class but not a need of the gate')
    for name, need in sorted(needs.items()):
        result = need.get('result')
        if result == 'success':
            continue
        if result == 'skipped' and name not in wanted:
            continue
        why = 'required by the change class' if name in wanted else 'ran and did not succeed'
        refusals.append(f'{name}: {result} ({why})')
    return refusals


def check_name_prefixes(cmap, workflows=ROOT / '.github' / 'workflows'):
    """Map each job id to the check-run name prefix GitHub shows for it."""
    import yaml
    prefixes = {}
    for job in cmap['jobs']:
        stem, name = job.split(':', 1)
        doc = yaml.safe_load((Path(workflows) / f'{stem}.yml').read_text())
        title = doc['jobs'][name].get('name', name)
        prefixes[job] = title.split(' (${{', 1)[0]
    return prefixes


def lane_skipped_is_green(checks, result, prefixes):
    """Admission-lane complete-green over check runs, separate from the lane's
    required-context check (aegis#736 owns that one).

    `checks` is {check-run name: conclusion}. A skipped check is green ONLY when
    the change class does not require the job it belongs to; a check this map
    does not know keeps the lane's strict rule (skipped is not green). Returns
    the list of checks still blocking; empty means complete-green.
    """
    required = set(result['required'])
    blocking = []
    for check, conclusion in sorted(checks.items()):
        if conclusion in ('success', 'neutral'):
            continue
        owners = [job for job, prefix in prefixes.items()
                  if check == prefix or check.startswith(prefix + ' (')]
        bypassable = owners and not result['full'] and not (set(owners) & required)
        if conclusion == 'skipped' and bypassable:
            continue
        blocking.append(f'{check}: {conclusion}')
    return blocking


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    sub = parser.add_subparsers(dest='cmd')
    gate_cmd = sub.add_parser('gate')
    gate_cmd.add_argument('--workflow', required=True)
    gate_cmd.add_argument('--required', required=True)
    lane_cmd = sub.add_parser('lane', help='complete-green over CHECKS_JSON for this diff')
    lane_cmd.add_argument('--checks-json', required=True)
    parser.add_argument('--base', default=os.environ.get('PR_BASE', ''))
    parser.add_argument('--head', default=os.environ.get('PR_HEAD', ''))
    parser.add_argument('--head-ref', default=os.environ.get('PR_HEAD_REF', ''))
    parser.add_argument('--paths', nargs='*')
    parser.add_argument('--event', default=os.environ.get('GITHUB_EVENT_NAME', ''))
    parser.add_argument('--json', action='store_true')
    args = parser.parse_args(argv)
    if args.cmd == 'gate':
        try:
            needs = json.loads(os.environ.get('NEEDS_JSON', ''))
        except ValueError:
            print('refused: NEEDS_JSON is missing or not JSON')
            return 1
        refusals = gate(needs, args.workflow, [j for j in args.required.split(',') if j])
        for line in refusals:
            print('refused: ' + line)
        if not refusals:
            print('every job the change class requires passed')
        return 1 if refusals else 0
    cmap, result = evaluate(args.base, args.head, args.head_ref, args.paths)
    result = for_event(cmap, result, args.event)
    if args.cmd == 'lane':
        checks = json.loads(Path(args.checks_json).read_text())
        blocking = lane_skipped_is_green(checks, result, check_name_prefixes(cmap) if cmap else {})
        print(json.dumps({'classes': result['classes'], 'blocking': blocking}))
        return 1 if blocking else 0
    sys.stdout.write(json.dumps(result) + '\n' if args.json else render(cmap, result))
    return 0


if __name__ == '__main__':
    sys.exit(main())
