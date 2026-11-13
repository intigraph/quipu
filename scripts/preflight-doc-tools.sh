#!/usr/bin/env bash
# Check prerequisites only; never installs tools or changes the caller's PATH.
set -euo pipefail

failed=0
for tool in mdbook mdbook-mermaid; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'ERROR: %s not found on PATH.\n' "$tool" >&2
        failed=1
    elif ! "$tool" --version >/dev/null 2>&1; then
        printf 'ERROR: %s is on PATH but cannot run --version. Reinstall it for this machine.\n' "$tool" >&2
        failed=1
    fi
done

if [ "$failed" -ne 0 ]; then
    cat >&2 <<'HELP'
The book requires both mdbook and the mdbook-mermaid preprocessor.
If already installed with Cargo, expose its bin directory in this shell:
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
Otherwise install the required tools (also supported on macOS):
  cargo install --locked mdbook mdbook-mermaid
For a broken executable, reinstall the named tool for this machine with --force.
Then rerun: just docs preflight
See CONTRIBUTING.md (Documentation prerequisites). The book build is not skipped.
HELP
    exit 1
fi
