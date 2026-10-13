#!/usr/bin/env bash
# arming: agent-invoked; isolated paper/benchmark demonstration, no installed service mutation.
set -euo pipefail
: "${MERGE_DEMO_SCRATCH:?Set a fresh, authorized local fixture root}"
: "${MERGE_DEMO_BASE:?Set the exact base revision}"
demo_tmp=$(mktemp -d)
trap 'rm -rf "$demo_tmp"' EXIT

git show "$MERGE_DEMO_BASE:docs/paper-merge/main.pdf" > "$demo_tmp/base.pdf"
pdftotext "$demo_tmp/base.pdf" "$demo_tmp/base.txt"
pdftotext docs/paper-merge/main.pdf "$demo_tmp/head.txt"
printf 'Base revision: %s\n' "$MERGE_DEMO_BASE"
printf 'Head revision: %s\n' "$(git rev-parse HEAD)"
printf 'Base PDF: '
pdfinfo "$demo_tmp/base.pdf" | awk '/^Pages:/ {print $2 " pages"}'
printf 'Head PDF: '
pdfinfo docs/paper-merge/main.pdf | awk '/^Pages:/ {print $2 " pages"}'
python3 - "$demo_tmp" <<'PY'
from pathlib import Path
import sys
import unicodedata
for label, name in (("Base", "base.txt"), ("Head", "head.txt")):
    text = " ".join(unicodedata.normalize("NFKC", (Path(sys.argv[1])/name).read_text()).split())
    print(label, "schema section:", "Divergent shape declarations (Arm E)" in text,
          "Memory section:", "Memory metadata granularity (Arm F)" in text)
PY
printf '\nReal Dolt fixture: 20 Memories, six true conflict slots; controls and cleanup must pass.\n'
printf '$ just bench memory --selftest --scratch <local-fixture-root>\n'
just bench memory --selftest --scratch "$MERGE_DEMO_SCRATCH" --output "$demo_tmp/result"
