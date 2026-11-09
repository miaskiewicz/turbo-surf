#!/usr/bin/env bash
# drop-turbo-dom-pin.sh — restore turbo-dom* path deps in rust/Cargo.toml to a
# published version range. The pre-publish git rev goes away; crates come from
# crates.io at a semver range (default "0.8").
#
# Usage: drop-turbo-dom-pin.sh [VERSION_REQ]
#   VERSION_REQ  cargo semver req (default: "0.8").
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help) agent_print_help_and_exit "$0" ;; esac
req="${1:-0.8}"

cd "$AGENT_REPO_ROOT"
toml=rust/Cargo.toml

python3 - "$toml" "$req" <<'PY'
import re, sys
path, req = sys.argv[1], sys.argv[2]
text = open(path).read()
pat = re.compile(
    r'^(turbo-dom(?:-(?:layout|raster|browser-env))?)\s*=\s*\{\s*git\s*=.+?\}\s*$',
    re.MULTILINE,
)
def repl(m):
    return f'{m.group(1)} = {{ version = "{req}" }}'
new, n = pat.subn(repl, text)
if n == 0:
    sys.exit("drop-turbo-dom-pin.sh: no turbo-dom git deps to drop")
open(path, "w").write(new)
print(f"dropped pin on {n} turbo-dom dep(s) → version = \"{req}\"")
PY
