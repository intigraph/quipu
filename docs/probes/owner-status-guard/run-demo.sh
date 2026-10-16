#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' 'Synthetic fixture: 12,000 Actions, two named graphs, four owned subjects per graph.' 'Baseline uses the retained independent-join path; candidate binds existing subject variables.'
cargo test --locked --offline --lib sparql::filter_join::tests::owner_guard_demo_12000_actions -- --ignored --exact --nocapture --test-threads=1 2>&1 | sed -n -e '/GUARD_DEMO/p' -e '/test result:/p'
