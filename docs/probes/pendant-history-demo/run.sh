#!/bin/sh
set -eu
# Preserve LF output in both the recording and its byte-count timing track.
stty -onlcr
exec python3 docs/probes/pendant-history-demo/record.py "$DEMO_QUIPU"
