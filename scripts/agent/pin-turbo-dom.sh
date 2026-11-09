#!/usr/bin/env bash
# pin-turbo-dom.sh — pin every turbo-dom* git dep in rust/Cargo.toml to REV.
#
# Usage: pin-turbo-dom.sh REV
#   REV  A full 40-char git sha from miaskiewicz/turbo-dom. Rewrites the four
#        turbo-dom* lines (parser, layout, raster, browser-env); turbo-dom-v8
#        has its own candidate rev and is NOT touched.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help|"") agent_print_help_and_exit "$0" 0 ;; esac

rev="$1"
if ! [[ "$rev" =~ ^[0-9a-f]{40}$ ]]; then
  agent_log "REV must be a 40-char lowercase git sha; got: $rev"
  exit 2
fi

cd "$AGENT_REPO_ROOT"
toml=rust/Cargo.toml

# Rewrite every turbo-dom / turbo-dom-{layout,raster,browser-env} dependency in
# place. turbo-dom-v8 keeps its separate rev on turbo-dom.git.
python3 - "$toml" "$rev" <<'PY'
import re, sys
path, rev = sys.argv[1], sys.argv[2]
text = open(path).read()
pat = re.compile(
    r'^(turbo-dom(?:-(?:layout|raster|browser-env))?\s*=\s*\{\s*git\s*=\s*"https://github\.com/miaskiewicz/turbo-dom",\s*rev\s*=\s*)"[0-9a-f]+"',
    re.MULTILINE,
)
new, n = pat.subn(lambda m: f'{m.group(1)}"{rev}"', text)
if n == 0:
    sys.exit("pin-turbo-dom.sh: no turbo-dom git deps found")
open(path, "w").write(new)
print(f"pinned {n} turbo-dom dep(s) → {rev[:12]}")
PY

agent_log "run: cargo update -p turbo-dom -p turbo-dom-layout -p turbo-dom-raster -p turbo-dom-browser-env"
