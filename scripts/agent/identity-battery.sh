#!/usr/bin/env bash
# identity-battery.sh — run the Chrome identity battery against the committed
# fixture (harness/detection/fixtures/chrome-identity-<major>.json).
#
# Usage: identity-battery.sh [--help]
#   Uses oracle.sh's Chrome 156 full-app binary. See
#   harness/detection/CHROME-REVERSION.md for the battery schema.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac

cd "$AGENT_REPO_ROOT"
agent_require_oracle

# --check: measures, diffs vs the committed fixture, writes nothing to the
# registry. Non-zero exit = a delta a human must resolve.
exec node harness/detection/reversion-chrome.mjs \
  --chrome "$AGENT_CHROME_APP" \
  --check
