#!/usr/bin/env bash
set -euo pipefail
# Keep LF output byte-identical in the replay log and ordinary text tooling.
stty -onlcr
exec "${C1_DEMO_BINARY:?Set C1_DEMO_BINARY to the compiled shape_mapping example}"
