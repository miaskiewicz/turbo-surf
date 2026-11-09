#!/usr/bin/env bash
# parity-check.sh — run turbo-dom's parity tool against surf's expected file
# (harness/detection/API-SHAPE.json, the committed per-member surface).
#
# Usage: parity-check.sh [--help]
#   Runs the offline api-shape parity test; agents re-run it after any change
#   that may shift the exposed Web-API surface. For the Chrome identity
#   parity, see identity-battery.sh.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac

cd "$AGENT_REPO_ROOT"
exec node --test --test-concurrency=1 harness/detection/api-shape.test.mjs
