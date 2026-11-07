#!/usr/bin/env bash
# release-prep.sh — bump every version string to X.Y.Z, then verify with
# scripts/check-versions.py. Does NOT tag or publish (see PUBLISHING.md).
#
# Usage: release-prep.sh X.Y.Z
#   Edits in place (git diff shows every change): package.json,
#   package-lock.json, rust/Cargo.toml (workspace + path-dep versions),
#   rust/crates/*/Cargo.toml, core/mcp VERSION, the napi crate's version(),
#   the napi package.json, README status line.
set -euo pipefail
. "$(dirname "$0")/_lib.sh"

case "${1:-}" in -h|--help|"") agent_print_help_and_exit "$0" 0 ;; esac
version="$1"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$ ]]; then
  agent_log "X.Y.Z required (optional -prerelease suffix); got: $version"
  exit 2
fi

cd "$AGENT_REPO_ROOT"

python3 - "$version" <<'PY'
import json, re, sys
from pathlib import Path
v = sys.argv[1]

# package.json / package-lock.json
for p in ("package.json", "package-lock.json",
          "rust/crates/turbo-surf-napi/package.json"):
    data = json.loads(Path(p).read_text())
    data["version"] = v
    if "packages" in data and "" in data["packages"]:
        data["packages"][""]["version"] = v
    Path(p).write_text(json.dumps(data, indent=2) + "\n")

# Every Cargo.toml: [package] version = "..." and every path-dep that carries
# an explicit version alongside path.
for toml in Path("rust").rglob("Cargo.toml"):
    text = toml.read_text()
    text = re.sub(r'^(version\s*=\s*)"[^"]+"', f'\\1"{v}"', text, count=1, flags=re.MULTILINE)
    text = re.sub(
        r'(\{\s*path\s*=\s*"[^"]+"\s*,\s*version\s*=\s*)"[^"]+"',
        f'\\1"{v}"', text,
    )
    toml.write_text(text)

# VERSION files
for p in ("rust/crates/turbo-surf-core/VERSION",
          "rust/crates/turbo-surf-mcp/VERSION"):
    path = Path(p)
    if path.exists():
        path.write_text(v + "\n")

# napi version() helper, if present.
for src in Path("rust/crates/turbo-surf-napi/src").rglob("*.rs"):
    s = src.read_text()
    new = re.sub(r'"(\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?)" /\* version \*/',
                 f'"{v}" /* version */', s)
    if new != s:
        src.write_text(new)

# README status line "turbo-surf vX.Y.Z"
readme = Path("README.md")
if readme.exists():
    s = readme.read_text()
    s = re.sub(r'turbo-surf v\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?',
               f'turbo-surf v{v}', s, count=1)
    readme.write_text(s)

print(f"release-prep: bumped to {v}")
PY

agent_log "verify with scripts/check-versions.py"
exec python3 scripts/check-versions.py
