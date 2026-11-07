#!/usr/bin/env bash
# oracle.sh — run the frozen Chrome 156 chrome-headless-shell as the oracle.
#
# Usage: oracle.sh [--help] [ARGS...]
#   Prepends localhost-only flags and the mock-keychain / basic-password-store
#   switches, then execs chrome-headless-shell with the supplied ARGS. Every
#   other recorder / probe calls this script so the launch flags stay in one
#   place. Path: $TURBO_CHROME_HEADLESS (default: the cache described in
#   docs/AGENTS.md).
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac

agent_require_oracle

exec "$AGENT_CHROME_HEADLESS" \
  --headless=new \
  --use-mock-keychain \
  --password-store=basic \
  --host-resolver-rules="MAP * ~NOTFOUND , EXCLUDE 127.0.0.1, EXCLUDE localhost, EXCLUDE ::1" \
  --no-default-browser-check \
  --no-first-run \
  --disable-sync \
  "$@"
