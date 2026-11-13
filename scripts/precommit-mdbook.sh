#!/usr/bin/env bash
set -euo pipefail

script_dir=${BASH_SOURCE[0]%/*}
"$script_dir/preflight-doc-tools.sh"
exec mdbook build docs/book
