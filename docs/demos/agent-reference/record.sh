#!/usr/bin/env bash
# arming: agent-invoked; source fixtures and private internal transfer only.
set -euo pipefail
: "${AGENT_REF_QUIPU_BIN:?matching native CLI/server pair}"
: "${AGENT_REF_BOBBIN_BIN:?native indexer with its compatible CPU runtime}"
bash docs/demos/agent-reference/probe.sh
python3 docs/demos/agent-reference/index-probe.py "$PWD" "$AGENT_REF_QUIPU_BIN" "$AGENT_REF_BOBBIN_BIN"
