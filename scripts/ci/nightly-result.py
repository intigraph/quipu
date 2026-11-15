#!/usr/bin/env python3
"""Fail-closed nightly gate, exact-package prerelease and one failure tracker."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile

SUITES = {'fmt', 'query-builders', 'release-correctness', 'extended-correctness', 'bundled-runtime',
          'wasm-explorer', 'nightly-wheels', 'nightly-merge-load', 'repository-pendant-source'}
TITLE = 'Nightly correctness failure'
ASSET = 'quipu-nightly-linux-x86_64.tar.gz'


def green(needs):
    return (set(needs) == SUITES
            and all(value.get('result') == 'success' for value in needs.values()))


def identity(env):
    sha = env['GITHUB_SHA']
    repo = env['GITHUB_REPOSITORY']
    if not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('invalid exact source SHA')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo):
        raise ValueError('invalid repository')
    return sha, repo


def trusted_schedule(env):
    if env.get('GITHUB_EVENT_NAME') != 'schedule' or env.get('GITHUB_REF') != 'refs/heads/main':
        raise ValueError('publication/tracking requires the trusted main schedule')


def package_identity(directory, sha):
    archive = directory / ASSET
    checksum = (directory / (ASSET + '.sha256')).read_text().split()
    if len(checksum) != 2 or checksum[1] != ASSET:
        raise ValueError('unexpected checksum asset')
    if hashlib.sha256(archive.read_bytes()).hexdigest() != checksum[0]:
        raise ValueError('nightly package checksum mismatch')
    with tarfile.open(archive, 'r:gz') as package:
        members = package.getmembers()
        files = {}
        for member in members:
            name = member.name.removeprefix('./')
            if member.isdir() and name in ('', '.'):
                continue
            if not member.isfile() or name in files or '/' in name:
                raise ValueError('unsafe or duplicate nightly archive member')
            files[name] = member
        if set(files) != {'quipu', 'quipu-server', 'SOURCE_SHA', 'LICENSE', 'FEATURES.txt'}:
            raise ValueError('unexpected nightly package contents')
        source = package.extractfile(files['SOURCE_SHA']).read().decode().strip()
        if source != sha:
            raise ValueError('package source differs from green run')
        if any(files[name].size == 0 for name in ('quipu', 'quipu-server')):
            raise ValueError('empty nightly binary')
    return archive


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


def publish(directory, env):
    trusted_schedule(env)
    sha, repo = identity(env)
    if env.get('GATE_RESULT') != 'success' or env.get('GATE_SHA') != sha:
        raise ValueError('no exact-SHA green nightly gate')
    archive = package_identity(directory, sha)
    # A commit-specific prerelease is immutable. No moving nightly tag, no stable
    # version, and no registry credentials or automatic stable promotion.
    run_id = env['GITHUB_RUN_ID']
    if not re.fullmatch(r'[0-9]+', run_id):
        raise ValueError('invalid nightly run identity')
    tag = 'nightly-' + run_id + '-' + sha
    notes = ('Experimental Quipu nightly; compatibility and breakage may change.\n\n'
             f'Source: {sha}\nGate: https://github.com/{repo}/actions/runs/{env["GITHUB_RUN_ID"]}\n'
             'Linux x86_64 full-feature CLI/server. Verify the adjacent SHA256.\n'
             'Opt in by downloading this prerelease; keep the previous installation and a store backup.\n'
             'Stable promotion requires a separate seven-day soak and release review.\n')
    releases = json.loads(gh('api', f'repos/{repo}/releases?per_page=100'))
    existing = next((release for release in releases if release['tag_name'] == tag), None)
    if existing:
        if not existing['prerelease'] or existing['target_commitish'] != sha:
            raise ValueError('existing nightly identity disagrees')
        with tempfile.TemporaryDirectory(prefix='nightly-existing-') as scratch:
            gh('release', 'download', tag, '--repo', repo, '--pattern', ASSET,
               '--dir', scratch)
            if (Path(scratch) / ASSET).read_bytes() != archive.read_bytes():
                raise ValueError('existing nightly asset differs; refusing overwrite')
        return
    with tempfile.TemporaryDirectory(prefix='nightly-notes-') as scratch:
        body = Path(scratch) / 'notes.md'
        body.write_text(notes)
        gh('release', 'create', tag, '--repo', repo, '--target', sha, '--prerelease',
           '--latest=false', '--title', 'Experimental nightly ' + sha[:12],
           '--notes-file', str(body), str(archive), str(archive) + '.sha256')


def track(env):
    trusted_schedule(env)
    sha, repo = identity(env)
    issues = json.loads(gh('issue', 'list', '--repo', repo, '--state', 'all',
                           '--search', 'in:title "Nightly correctness failure"',
                           '--limit', '100', '--json', 'number,title,state'))
    matches = [issue for issue in issues if issue['title'] == TITLE]
    if len(matches) > 1:
        raise ValueError('multiple nightly trackers; refusing another')
    with tempfile.TemporaryDirectory(prefix='nightly-failure-') as scratch:
        body = Path(scratch) / 'failure.md'
        body.write_text(f'Nightly source {sha} did not publish successfully.\n\n'
                        f'https://github.com/{repo}/actions/runs/{env["GITHUB_RUN_ID"]}\n\n'
                        '```json\n' + json.dumps(json.loads(env['NEEDS_JSON']), indent=2) + '\n```\n')
        if matches:
            number = str(matches[0]['number'])
            if matches[0]['state'] == 'CLOSED':
                gh('issue', 'reopen', number, '--repo', repo)
            gh('issue', 'edit', number, '--repo', repo, '--body-file', str(body))
        else:
            gh('issue', 'create', '--repo', repo, '--title', TITLE, '--body-file', str(body))


def main():
    if sys.argv[1] == 'gate':
        if not green(json.loads(os.environ['NEEDS_JSON'])):
            raise SystemExit('nightly suite missing, failed, cancelled or skipped')
        sha, _ = identity(os.environ)
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write('sha=' + sha + '\n')
    elif sys.argv[1] == 'publish':
        publish(Path(sys.argv[2]), os.environ)
    elif sys.argv[1] == 'track':
        track(os.environ)
    else:
        raise SystemExit('usage: nightly-result.py gate|publish ASSET_DIR|track')


if __name__ == '__main__':
    main()
