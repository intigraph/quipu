#!/usr/bin/env bash
set -euo pipefail
: "${MIGRATION_TEST_BIN:?Set MIGRATION_TEST_BIN to the compiled SHACL library test binary}"
printf '%s\n' 'BEFORE: retained baseline output, not a live replay of the old executable.'
cat before.txt
printf '%s\n' 'AFTER: current candidate; four materialized duplicates refuse at three and accept at four.'
"$MIGRATION_TEST_BIN" --exact proposal_migration::tests::repeated_template_facts_are_bounded_before_netting --nocapture --test-threads=1
printf '%s\n' 'FILTER CONTROL: ten intermediates refuse at three, succeed at ten; stricter native ceiling wins.'
"$MIGRATION_TEST_BIN" --exact proposal_migration::tests::filtered_where_intermediates_share_the_migration_ceiling --nocapture --test-threads=1
