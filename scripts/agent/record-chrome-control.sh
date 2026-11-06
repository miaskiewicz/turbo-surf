#!/usr/bin/env bash
# record-chrome-control.sh — format a Chrome probe, then record its control.
#
# Usage: record-chrome-control.sh PROBE OUT
#   PROBE  Path to the probe .js (e.g. harness/detection/foo-probe.js). The
#          script biome-formats it BEFORE recording so the pre-commit hook's
#          source hash matches the recorded control.
#   OUT    Fixture file to write (e.g.
#          harness/detection/fixtures/foo-chrome156.json).
#
# Uses harness/detection/web-apis-chrome-control.mjs with the frozen Chrome 156
# oracle. Paths must be absolute or repo-relative.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac
if [ $# -ne 2 ]; then
  agent_print_help_and_exit "$0" 2
fi

probe="$1"
out="$2"
cd "$AGENT_REPO_ROOT"

agent_require_oracle

# Step 1 — format the probe; a later hook must not rewrite its bytes and drift
# the recorded hash.
agent_log "biome format $probe"
npx --no-install biome format --write "$probe"

# Step 2 — record the control. The runner argument is the shared tracer.
agent_log "record control → $out"
exec node harness/detection/web-apis-chrome-control.mjs \
  --runner "$AGENT_REPO_ROOT/harness/detection/chrome-cdp-trace.mjs" \
  --probes "$(basename "$probe")" \
  --chrome "$AGENT_CHROME_APP" \
  --out "$out"
