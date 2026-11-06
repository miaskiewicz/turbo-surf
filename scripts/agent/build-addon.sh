#!/usr/bin/env bash
# build-addon.sh — install deps + build the napi addon with the cached V8 and a
# per-run CARGO_TARGET_DIR in the scratchpad (so rust/target, a symlink owned by
# the root checkout, is left alone).
#
# Usage: build-addon.sh [--help]
#   Honors RUSTY_V8_ARCHIVE (default: the simdutf archive in
#   ~/.cache/turbo-consolidation/v8cache/, see docs/AGENTS.md).
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac

cd "$AGENT_REPO_ROOT"
agent_require_v8_archive

export RUSTY_V8_ARCHIVE="$AGENT_V8_ARCHIVE"
export CARGO_TARGET_DIR="$(agent_cargo_target_dir)"
mkdir -p "$CARGO_TARGET_DIR"

agent_log "npm ci"
npm ci

agent_log "build:addon (CARGO_TARGET_DIR=$CARGO_TARGET_DIR)"
npm run build:addon
