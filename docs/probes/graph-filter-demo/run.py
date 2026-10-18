#!/usr/bin/env python3
"""Run the actual private library control, retaining only its public output."""
import subprocess
print('Synthetic named graphs; the filter is CONTAINS(STR(?g),"seeds").', flush=True)
print('Original control: the same filter OR false, disabling this optimization.', flush=True)
print('The test compares actual result bags and grouped counts, then leaf scans.', flush=True)
command = ['cargo', 'test', '--offline', '--features', 'shacl', '--lib',
           'sparql::graph_filter_tests::graph_filter_reduces_actual_leaf_scans_and_preserves_count_groups',
           '--', '--exact', '--nocapture']
print('$ ' + ' '.join(command), flush=True)
r = subprocess.run(command, capture_output=True, text=True)
for line in r.stdout.splitlines():
    if line.startswith('GRAPH_LEAF_SCANS') or line.startswith('test result:'):
        print(line, flush=True)
if r.returncode:
    raise SystemExit('Reproducer failed; inspect local Cargo output before recording again.')
