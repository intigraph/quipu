"""One deterministic correction body for the changelog and book."""
import tomllib

from .contract import ROOT, readiness, records

OUTPUTS = [ROOT / 'releases/BREAKING-CHANGES.md',
           ROOT / 'docs/book/src/reference/breaking-changes.md']


def text(value):
    # Supplied Markdown/HTML is prose, never raw directives or executable fences.
    return (value.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')
            .replace('{', '&#123;').replace('}', '&#125;').replace('`', '&#96;').replace('[', '&#91;').replace(']', '&#93;')
            .replace('\r', ' ').replace('\n', ' '))


def render():
    lines = ['# Breaking changes and running-system corrections', '',
             '<!-- Generated from releases/changes/*.json; use just breaking render. -->', '',
             'Draft records are incomplete and cannot satisfy release readiness. '
             'Doctor produces read-only plans; a plan never grants migration authority.', '']
    for _, record, _ in records():
        missing = readiness(record)
        lines.extend(['## ' + text(record['title']), '',
                      '**ID:** ' + record['id'] + ' — **Status:** ' + record['status'], '',
                      '**Affected:** ' + '; '.join(s['tool'] + ' ' + ', '.join(s['versions'])
                                                 if s['versions'] else s['tool'] + ' (versions unknown)'
                                                 for s in record['affected']), '',
                      '**Target platforms:** ' + ('; '.join(t['tool'] + ' ' + t['version'] + ': ' + ', '.join(t['platforms']) for t in record['targets']) or 'unknown (draft)'), '',
                      text(record['symptom']), '',
                      '**Source:** `' + text(record['source_ref']) + '`', '', '### Detect', '',
                      text(record['detection']['description']), '',
                      '**Canonical predicates:** ' + ', '.join('`' + text(t) + '`' for t in record['detection']['canonical_terms']), '',
                      '**Historical predicates:** ' + ', '.join('`' + text(t) + '`' for t in record['detection']['legacy_terms']), '', '### Correct', '',
                      'Operation: ' + record['correction']['operation'] + '. Separate admission required.', '',
                      '**Correction packet:** `' + text(record['correction']['packet_ref'] or 'not supplied (draft)') + '`'
                      + ' — SHA256: ' + (record['correction']['packet_sha256'] or 'unknown') + '.', ''])
        for i, step in enumerate(record['correction']['steps'], 1):
            lines.append(str(i) + '. ' + text(step))
        for title, values in [('Prerequisites', record['prerequisites']),
                              ('Validate', record['validation'])]:
            lines.extend(['', '### ' + title, ''])
            lines.extend('- ' + text(value) for value in values)
        lines.extend(['', '### Roll back', '', text(record['rollback']['before_new_writes']), '',
                      text(record['rollback']['after_new_writes']), '',
                      '**Rollback packet:** `' + text(record['rollback']['packet_ref'] or 'not supplied (draft)') + '`'
                      + ' — SHA256: ' + (record['rollback']['packet_sha256'] or 'unknown') + '.', '',
                      '**Data-loss limit:** ' + text(record['rollback']['data_loss_limit']), '',
                      '**Release readiness:** ' + ('incomplete: ' + ', '.join(missing) if missing
                                                   else 'structurally complete; approval unverified'), '',
                      '**Required release approver:** ' + record['required_approver'] + '.', ''])
    return '\n'.join(lines)


def check_references():
    link = '[Breaking changes and running-system corrections](releases/BREAKING-CHANGES.md)'
    header = tomllib.loads((ROOT / 'cliff.toml').read_text())['changelog']['header']
    if link not in header or link not in (ROOT / 'CHANGELOG.md').read_text():
        raise ValueError('correction link missing from changelog or its generator header')
