#!/usr/bin/env python3
"""Run the agent-branches benchmark end to end and write the results table.

    python3 benchmark/agent-branches/run.py                  # all arms, seeds 1 7 42 99 2026
    python3 benchmark/agent-branches/run.py --no-terminusdb  # quipu + model arms only

Steps: build and run examples/agentbranches once per seed (model arms, the
real-store quipu arms, and the scenario hand-off); start
terminusdb/terminusdb-server:<version> in Docker, replay each scenario through
it, and remove the container (and the image, when this run pulled it); re-score
every Rust arm with scoring.py and refuse to publish if the scorers disagree;
write results/RESULTS.md and results/summary.json.

No LLM is called anywhere. Every agent is a deterministic script.
"""
import argparse
import collections
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import scoring  # noqa: E402
import terminusdb_arm  # noqa: E402

IMAGE = 'terminusdb/terminusdb-server'
# Pinned by digest, not tag: a re-pushed tag must not change the arm silently.
# This is the v12.0.8 image the published results were produced with.
TDB_VERSION = '12.0.8'
TDB_DIGEST = 'sha256:43061044d1857bb6f1a2f41d7a59ad06ba5f91c03cbc90128d7c0415f32a0916'
PROBES = ('valid-before', 'valid-after', 'current', 'belief-before-landing')
ARM_ORDER = [
    'quipu-promote-3way', 'terminusdb-apply', 'shape-aware', 'git-canonical',
    'context-merge', 'triple-3way', 'lww-theirs', 'union', 'two-way',
    'quipu-promote-2way-pre888', 'quipu-direct-root']
ARM_NOTES = {
    'quipu-promote-3way': 'shipped `quipu fork promote` (#888), real store',
    'terminusdb-apply': 'TerminusDB v{tdb}: branch per agent, `/api/apply` from the base commit',
    'shape-aware': 'merge paper operator on model graphs (control: must equal `quipu-promote-3way`)',
    'git-canonical': 'positional: `git merge-file` on sorted N-Triples',
    'context-merge': 'node-overlap heuristic (Quit Context Merge)',
    'triple-3way': 'set-algebraic 3-way, no schema',
    'lww-theirs': 'last writer wins per slot',
    'union': 'set union',
    'two-way': 'pre-#888 promote on model graphs (ROOT := fork)',
    'quipu-promote-2way-pre888': 'NEGATIVE CONTROL: pre-#888 promote, real store',
    'quipu-direct-root': 'as-of control: corrections written straight to ROOT, no fork',
}


def sh(cmd, **kw):
    return subprocess.run(cmd, check=True, **kw)


def build_and_run(seeds, params, out):
    sh(['cargo', 'build', '--quiet', '--release', '--example', 'agentbranches',
        '--features', 'shacl'], cwd=ROOT)
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
    exe = target / 'release' / 'examples' / 'agentbranches'
    results = {}
    for seed in seeds:
        sh([str(exe), '--seed', str(seed), '--out', str(out), *params], cwd=ROOT)
        results[seed] = json.loads((out / f'agentbranches-seed{seed}.json').read_text())
    return results


class TerminusDb:
    """A throwaway TerminusDB container; removes what it created."""

    def __init__(self, digest, port):
        self.image = f'{IMAGE}@{digest}'
        self.port = port
        self.name = f'agentbranches-tdb-{os.getpid()}'
        self.pulled = False

    def __enter__(self):
        have = subprocess.run(['docker', 'image', 'inspect', self.image],
                              capture_output=True).returncode == 0
        if not have:
            sh(['docker', 'pull', '-q', self.image])
            self.pulled = True
        sh(['docker', 'run', '-d', '--rm', '--name', self.name,
            '-p', f'127.0.0.1:{self.port}:6363', '-e', 'TERMINUSDB_ADMIN_PASS=agentbranches',
            self.image], capture_output=True)
        url = f'http://127.0.0.1:{self.port}'
        for _ in range(60):
            try:
                urllib.request.urlopen(url + '/api/ok', timeout=2)
                return url
            except OSError:
                time.sleep(1)
        raise RuntimeError('TerminusDB did not come up')

    def __exit__(self, *exc):
        subprocess.run(['docker', 'rm', '-f', self.name], capture_output=True)
        if self.pulled:
            subprocess.run(['docker', 'rmi', self.image], capture_output=True)


def cross_check(seed, doc):
    """scoring.py must agree with the Rust scorer on every Rust arm."""
    for arm in doc['arms']:
        losses, viol = scoring.rescore_rust_arm(doc['scenario'], arm)
        if (losses, viol) != (arm['silent_losses'], arm['violations']):
            raise SystemExit(
                f'seed {seed} arm {arm["arm"]}: python scorer says losses={losses} '
                f'violations={viol}, rust says {arm["silent_losses"]}/{arm["violations"]}')


def render(results, tdb_version, commit, params, seeds):
    totals = collections.OrderedDict()
    probe = collections.defaultdict(lambda: collections.Counter())
    controls = collections.Counter()
    tdb_requests = 0
    for seed, doc in results.items():
        for k, v in doc['controls'].items():
            controls[k] += int(v) if isinstance(v, bool) else v
        for arm in doc['arms']:
            t = totals.setdefault(arm['arm'], collections.Counter())
            for k in ('questions', 'needed', 'caught', 'missed', 'unnecessary',
                      'silent_losses', 'violations', 'rounds', 'writes'):
                t[k] += arm[k]
            t['manual_merges'] += arm.get('manual_merges', 0)
            t['promotions'] += len(doc['scenario']['agents'])
            tdb_requests += arm.get('http_requests', 0)
        for p in doc['probes']:
            probe[p['arm']][(p['probe'], 'n')] += 1
            probe[p['arm']][(p['probe'], 'ok')] += int(p['ok'])
    order = [a for a in ARM_ORDER if a in totals or a in probe]
    n = len(results)
    lines = [
        '# Agent-run branches: results',
        '',
        f'Generated by `python3 benchmark/agent-branches/run.py` at quipu `{commit}`. '
        f'Seeds {", ".join(map(str, seeds))}; per seed {params["agents"]} agents, '
        f'{params["entities"]} entities ({params["hot"]} contended), {params["edits"]} edits '
        f'+ 1 back-dated correction per agent, overlap {params["overlap"]}, '
        f'{params["root_writes"]} live ROOT writes. Numbers are sums over the {n} seeds. '
        'No LLM in the loop: every agent is a script.',
        '',
        '## Questions and losses',
        '',
        '`needed` is the merge paper\'s oracle evaluated on each arm\'s own inputs at each '
        'promotion, so it differs between arms whose earlier merges left ROOT different. '
        '`silent losses` are scripted writes that a promotion dropped from ROOT without asking '
        'about their slot. `violations` are final-ROOT slots over their `sh:maxCount`. '
        '`promote attempts` counts retries after a ruling.',
        '',
        '| arm | questions | needed | missed | unnecessary | silent losses | violations | promote attempts | what it is |',
        '|---|---:|---:|---:|---:|---:|---:|---:|---|',
    ]
    writes = None
    for arm in order:
        if arm not in totals:
            continue
        t = totals[arm]
        writes = t['writes']
        note = ARM_NOTES.get(arm, '').format(tdb=tdb_version)
        if t['manual_merges']:
            note += (f'; {t["manual_merges"]} of {t["promotions"]} merges landed by hand '
                     '(retry after the ruling still refused)')
        lines.append(
            f'| `{arm}` | {t["questions"]} | {t["needed"]} | {t["missed"]} | {t["unnecessary"]} '
            f'| {t["silent_losses"]} | {t["violations"]} | {t["rounds"]} | {note} |')
    lines += [
        '',
        f'Scripted writes across all seeds: {writes}.',
        '',
        '## As-of answers for back-dated corrections',
        '',
        'Each agent corrects one status "since" a past date. Four questions per correction: '
        'the value valid *before* the correction date, *after* it, *now*, and what ROOT '
        'believed about the after-date *before the correction landed* (bitemporal).',
        '',
        '| arm | ' + ' | '.join(PROBES) + ' | total |',
        '|---|' + '---:|' * (len(PROBES) + 1),
    ]
    for arm in order:
        if arm not in probe:
            continue
        c = probe[arm]
        cells = [f'{c[(p, "ok")]}/{c[(p, "n")]}' for p in PROBES]
        ok = sum(c[(p, 'ok')] for p in PROBES)
        total = sum(c[(p, 'n')] for p in PROBES)
        lines.append(f'| `{arm}` | ' + ' | '.join(cells) + f' | {ok}/{total} |')
    lines += [
        '',
        '## Instrument controls',
        '',
        f'- Real-store 3-way promote equals the model shape-aware operator (questions, oracle, '
        f'final ROOT): {controls["store_3way_equals_model_shape_aware"]}/{n} seeds.',
        f'- Real-store pre-#888 promote equals the model two-way arm: '
        f'{controls["store_2way_equals_model_two_way"]}/{n} seeds.',
        f'- SHACL refusals at promote: {controls["shacl_refusals_at_promote"]}.',
        '- `scoring.py` re-scored every Rust arm (silent losses, violations) and agreed.',
    ]
    if tdb_version:
        lines.append(f'- TerminusDB arm: {tdb_requests} HTTP requests to '
                     f'`{IMAGE}@{TDB_DIGEST}` (v{tdb_version}); container removed after the run.')
    return '\n'.join(lines) + '\n', {a: dict(totals[a]) for a in totals}, {
        a: {f'{p}:{k}': v for (p, k), v in probe[a].items()} for a in probe}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--seeds', default='1,7,42,99,2026')
    ap.add_argument('--no-terminusdb', action='store_true')
    ap.add_argument('--terminusdb-digest', default=TDB_DIGEST,
                    help='image digest to run (default: the pinned v12.0.8 digest)')
    ap.add_argument('--port', type=int, default=6366)
    ap.add_argument('--out', default=str(HERE / 'out'))
    ap.add_argument('--results', default=str(HERE / 'results'))
    ap.add_argument('harness_args', nargs='*', help='passed to the harness, after --')
    args = ap.parse_args()
    seeds = [int(s) for s in args.seeds.split(',')]
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    results = build_and_run(seeds, args.harness_args, out)
    for seed, doc in results.items():
        cross_check(seed, doc)

    tdb_version = None
    if not args.no_terminusdb:
        if not shutil.which('docker'):
            raise SystemExit('docker not found: pass --no-terminusdb to skip the TerminusDB arm')
        tdb_version = TDB_VERSION if args.terminusdb_digest == TDB_DIGEST else args.terminusdb_digest
        with TerminusDb(args.terminusdb_digest, args.port) as url:
            for seed, doc in results.items():
                metrics, probes = terminusdb_arm.run(
                    doc['scenario'], url, 'admin', 'agentbranches', f'agentbranches_{seed}')
                if metrics['errors']:
                    raise SystemExit(f'seed {seed}: TerminusDB apply failed: {metrics["errors"]}')
                doc['arms'].append(metrics)
                doc['probes'].extend(probes)
                print(f'seed {seed}: terminusdb questions={metrics["questions"]} '
                      f'needed={metrics["needed"]} lost={metrics["silent_losses"]}')

    commit = subprocess.run(['git', 'rev-parse', '--short=8', 'HEAD'], cwd=ROOT,
                            capture_output=True, text=True).stdout.strip() or 'unknown'
    params = next(iter(results.values()))['params']
    md, totals, probes = render(results, tdb_version, commit, params, seeds)
    res = Path(args.results)
    res.mkdir(parents=True, exist_ok=True)
    (res / 'RESULTS.md').write_text(md)
    (res / 'summary.json').write_text(json.dumps({
        'commit': commit, 'seeds': seeds, 'params': params,
        'terminusdb': tdb_version, 'terminusdb_image': f'{IMAGE}@{args.terminusdb_digest}', 'arms': totals, 'probes': probes}, indent=2, sort_keys=True) + '\n')
    print(md)


if __name__ == '__main__':
    main()
