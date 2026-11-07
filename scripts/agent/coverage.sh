#!/usr/bin/env bash
# coverage.sh — run one tarpaulin coverage shard, honoring rust/coverage-floors.json.
#
# Usage: coverage.sh [CRATE]
#   CRATE  One of the measured crates (turbo-surf-core / -mcp / -page / -render
#          / -transform / -view). With no arg, runs every measured crate as a
#          single shard (slower; prefer one crate at a time in CI).
#
# Writes to $CARGO_TARGET_DIR/tarpaulin-<shard>/ so a shard never overwrites
# another.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac

crate="${1:-}"
cd "$AGENT_REPO_ROOT/rust"
agent_require_v8_archive
export RUSTY_V8_ARCHIVE="$AGENT_V8_ARCHIVE"
export CARGO_TARGET_DIR="$(agent_cargo_target_dir)/tarpaulin"
mkdir -p "$CARGO_TARGET_DIR"

if [ -z "$crate" ]; then
  crate=$(python3 -c '
import json
print(" ".join(json.load(open("coverage-floors.json"))["floors"].keys()))
')
fi

shard="${crate// /_}"
agent_log "tarpaulin shard=$shard crate=$crate"
exec python3 ../scripts/coverage-shard.py "$shard" $crate
