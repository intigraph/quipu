#!/usr/bin/env bash
set -euo pipefail
: "${QUIPU_DEMO_BASE:?select the installed pre-change CLI}"
: "${QUIPU_DEMO_CANDIDATE:?select the compiled candidate CLI}"
echo '$ installed baseline: quipu version'
"$QUIPU_DEMO_BASE" version
echo '$ installed baseline: quipu report --public'
before=0
env -u QUIPU_REPORT_PROMETHEUS_URL "$QUIPU_DEMO_BASE" report --public || before=$?
echo "exit: $before"
[[ "$before" == 2 ]]
echo '$ candidate: quipu report --public (private authenticated synthetic Prometheus fixture)'
python3 scripts/test-public-slo-report.py "$QUIPU_DEMO_CANDIDATE" --demo
