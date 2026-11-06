#!/usr/bin/env bash
# Shared helpers for scripts/agent/*. Sourced, not executed.
# All agent scripts expect a POSIX-ish bash on macOS or Linux.
set -euo pipefail

AGENT_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

AGENT_CACHE_ROOT="${TURBO_CACHE_ROOT:-$HOME/.cache/turbo-consolidation}"
AGENT_ORACLE_ROOT="${TURBO_ORACLE_ROOT:-$AGENT_CACHE_ROOT/chrome156}"
AGENT_V8_CACHE="${TURBO_V8_CACHE:-$AGENT_CACHE_ROOT/v8cache}"
AGENT_V8_ARCHIVE="${RUSTY_V8_ARCHIVE:-$AGENT_V8_CACHE/librusty_v8_simdutf_release_aarch64-apple-darwin.a.gz}"

# chrome-headless-shell is the oracle used by every recorder / probe.
AGENT_CHROME_HEADLESS="${TURBO_CHROME_HEADLESS:-$AGENT_ORACLE_ROOT/chrome-headless-shell-mac-arm64/chrome-headless-shell}"
# Full Chrome for Testing — for probes that need a real window (CDP) or
# resources from the .app bundle.
AGENT_CHROME_APP="${TURBO_SURF_CHROME_PATH:-$AGENT_ORACLE_ROOT/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing}"

# Per-run CARGO_TARGET_DIR under the scratchpad, if the host set one, so builds
# do not fight over rust/target (which is a symlink the root checkout owns).
AGENT_SCRATCH="${CLAUDE_SCRATCHPAD_DIR:-${TMPDIR:-/tmp}}"
AGENT_TARGET_DIR_DEFAULT="$AGENT_SCRATCH/turbo-surf-cargo-target"

agent_log() { printf '[agent] %s\n' "$*" >&2; }

agent_require_oracle() {
  if [ ! -x "$AGENT_CHROME_HEADLESS" ]; then
    agent_log "chrome-headless-shell not found at: $AGENT_CHROME_HEADLESS"
    agent_log "set TURBO_CHROME_HEADLESS or populate $AGENT_ORACLE_ROOT (see docs/AGENTS.md)."
    exit 2
  fi
}

agent_require_v8_archive() {
  if [ ! -f "$AGENT_V8_ARCHIVE" ]; then
    agent_log "rusty_v8 archive not found at: $AGENT_V8_ARCHIVE"
    agent_log "set RUSTY_V8_ARCHIVE or populate $AGENT_V8_CACHE (see docs/AGENTS.md)."
    exit 2
  fi
}

agent_cargo_target_dir() {
  echo "${CARGO_TARGET_DIR:-$AGENT_TARGET_DIR_DEFAULT}"
}

agent_print_help_and_exit() {
  # Print the header comment block (lines 2.. that start with `#`), stripping
  # the leading `# `. Stops at the first non-comment line.
  awk 'NR==1{next} /^#/{sub(/^# ?/,""); print; next} {exit}' "$1"
  exit "${2:-0}"
}
