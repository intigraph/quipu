#!/bin/sh
set -eu
stty -onlcr 2>/dev/null || true
python3 docs/probes/resolution-cache-demo/run.py
