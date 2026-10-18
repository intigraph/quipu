#!/usr/bin/env python3
"""Run the actual offline library experiment and print its measured output."""
import subprocess

print('Private synthetic fixture: 268000 labels, no embedding provider or attachments.', flush=True)
print('Same resolve input and one-node episode body; cache_labels=false then true.', flush=True)
print('Cold/stale requests retain exact scans. This is not a production measurement.', flush=True)
command = ['cargo', 'test', '--offline', '--release', '--features', 'shacl', '--lib',
           'resolution::perf_tests::large_label_fixture', '--', '--ignored', '--exact', '--nocapture']
print('$ ' + ' '.join(command), flush=True)
result = subprocess.run(command, capture_output=True, text=True)
for line in result.stdout.splitlines():
    if line.startswith('QYTJUG_') or line.startswith('test result:'):
        print(line, flush=True)
if result.returncode:
    raise SystemExit('Experiment failed; inspect the local Cargo output before recording again.')
