#!/usr/bin/env python3
"""Show effective workflow configuration from immutable Git objects; no dispatch."""
import subprocess
import yaml

BASE = 'd66142d417a185597415010b4669e998bea27c6c'
HEAD = '9c497d70'


def workflow(rev, path):
    result = subprocess.run(['git', 'show', f'{rev}:.github/workflows/{path}'],
                            check=False, capture_output=True, text=True)
    if result.returncode:
        return None
    value = yaml.safe_load(result.stdout)
    value['on'] = value.pop(True, value.get('on'))
    return value


def show(rev):
    sha = subprocess.check_output(['git', 'rev-parse', rev], text=True).strip()
    publication = workflow(rev, 'release.yml')
    steps = publication['jobs']['release-plz']['steps']
    commands = [s.get('with', {}).get('command', '(action default)')
                for s in steps if 'release-plz-action@' in s.get('uses', '')]
    proposal = workflow(rev, 'release-pr-daily.yml')
    print('revision:', sha)
    print('push release action commands:', ', '.join(commands))
    print('separate proposal workflow:', 'present' if proposal else 'absent')
    if proposal:
        print('proposal triggers:', proposal['on'])
        print('proposal ready gate:', proposal['jobs']['release-pr']['if'])
        print('proposal jobs:', ', '.join(proposal['jobs']))
    print()


print('Effective source configuration only; no workflow or release is executed.\n')
show(BASE)
show(HEAD)
