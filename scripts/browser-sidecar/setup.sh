#!/usr/bin/env bash
# Build the hardened-Chrome browser sidecar that powers turbo-surf's
# `web_search { browser:true }` (and the google engine, which requires it).
#
# What it does — all local, all reproducible:
#   1. installs patchright (a CDP-hardened, drop-in playwright) into THIS dir's
#      node_modules (gitignored),
#   2. verifies a real Google Chrome is available (patchright drives it via
#      channel:'chrome' — genuine surface + no CDP Runtime.enable tell),
#   3. prints the TURBO_SURF_BROWSER_FETCH_CMD to export.
#
# Chrome + node_modules stay OUT of git; only this script + fetch-serp.mjs +
# package.json are committed, so any user runs `bash scripts/browser-sidecar/setup.sh`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$HERE"

echo "[sidecar] installing patchright (CDP-hardened playwright) …"
if command -v npm >/dev/null 2>&1; then
  npm install --no-audit --no-fund
else
  echo "  ! npm not found — install Node.js first" >&2
  exit 1
fi

echo "[sidecar] ensuring a Chrome browser for patchright …"
# Prefer the system Google Chrome (best stealth). If absent, install patchright's
# bundled chromium as a fallback.
if [ -d "/Applications/Google Chrome.app" ] || command -v google-chrome >/dev/null 2>&1 || command -v google-chrome-stable >/dev/null 2>&1; then
  echo "  ✓ system Google Chrome found (channel:'chrome')"
else
  echo "  · no system Chrome — installing patchright's bundled chromium"
  npx patchright install chromium || true
fi

echo
echo "[sidecar] done. Enable browser search by exporting:"
echo
echo "  export TURBO_SURF_BROWSER_FETCH_CMD=\"node $HERE/fetch-serp.mjs\""
echo
echo "then: web_search { query:\"…\", engine:\"google\" }  (or any engine with browser:true)"
