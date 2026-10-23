#!/usr/bin/env bash
# Preserve the deployed legacy interface while admitting the copied owned job.
set -euo pipefail
if [[ ${1:-} == --trusted-release ]]; then
  exec python3 "$(dirname "$0")/publish_repository_share.py" "$@"
fi
exec bash "$(dirname "$0")/publish-legacy-repository-share.sh" "$@"
