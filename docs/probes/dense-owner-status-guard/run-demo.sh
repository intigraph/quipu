#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' 'Synthetic fixture: 12,001 subjects in two named graphs, with dense literal/IRI ownership.' 'The preceding planner evaluates the status guard per selected subject; the candidate batches selected subjects.' 'Both paths must find the same deliberately invalid status (ASK true).'
"${CARGO:-cargo}" test --locked --offline --lib sparql::filter_join::tests::dense_tests::dense_owner_guard_demo -- --ignored --exact --nocapture --test-threads=1 2>&1 | sed -n -e '/DENSE_GUARD_DEMO/p' -e '/test result:/p' -e '/error:/p'
