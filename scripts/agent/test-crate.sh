#!/usr/bin/env bash
# test-crate.sh — cargo test one workspace crate, with its default features and
# the cached V8 archive.
#
# Usage: test-crate.sh CRATE [FILTER]
#   CRATE   turbo-surf-{core,page,view,render,transform,napi,py,mcp}
#   FILTER  Optional test name substring passed through to cargo test.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help|"") agent_print_help_and_exit "$0" 0 ;; esac

crate="$1"
shift || true
filter="${1:-}"

cd "$AGENT_REPO_ROOT/rust"
agent_require_v8_archive
export RUSTY_V8_ARCHIVE="$AGENT_V8_ARCHIVE"
export CARGO_TARGET_DIR="$(agent_cargo_target_dir)"
mkdir -p "$CARGO_TARGET_DIR"

if [ -n "$filter" ]; then
  exec cargo test --locked -p "$crate" -- "$filter"
else
  exec cargo test --locked -p "$crate"
fi
