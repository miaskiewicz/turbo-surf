#!/bin/sh
# Re-vendor the turbo-test DOM binding into turbo-surf-render.
#
# turbo-surf's JS-render tier reuses turbo-test's rtdom↔V8 browser binding
# (`browser_env.{rs,js}`). There is intentionally NO cross-repo crate dependency
# (turbo-test is a test runner, not a published lib), so the copy is kept in sync
# MANUALLY — by running this script whenever turbo-test's binding gains coverage.
#
# It is the entire sync story:
#   - browser_env.js          → copied byte-for-byte.
#   - browser_env_upstream.rs → copied with ONE mechanical transform: leading
#       inner-doc lines `//!` become `//`, because src/browser_env.rs `include!`s
#       this file and `include!` forbids a file beginning with inner attributes.
#       No code is changed.
#   - cssom_parser.rs → copied byte-for-byte from canonical upstream.
#   - html_documents.rs + browser_sanitizer_default.json → copied byte-for-byte.
#   - browser_env.rs → generated from browser-env-adapter.rs.in, revision embedded.
#
# Usage (from this crate's dir, with the turbo-test checkout beside turbo-surf):
#   sh scripts/vendor-browser-env.sh [path-to-turbo-test-repo]
set -eu

# Vendor from the turbo-test repo's LAST COMMIT (HEAD), never the dirty working
# tree — so a half-finished local edit in turbo-test can't leak into our vendored
# copy. Pass the turbo-test repo root (default: sibling of turbo-surf).
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo="${1:-$script_dir/../../../../../turbo-test}"
dest="$(CDPATH= cd -- "$(dirname -- "$0")/../src" && pwd)"

git -C "$repo" rev-parse --short HEAD >/dev/null 2>&1 || {
  echo "not a git repo (need turbo-test's HEAD): $repo" >&2
  exit 1
}
rev="$(git -C "$repo" rev-parse --short HEAD)"

git -C "$repo" show HEAD:src/browser_env.js > "$dest/browser_env.js"
git -C "$repo" show HEAD:src/cssom_parser.rs > "$dest/cssom_parser.rs"
git -C "$repo" show HEAD:src/css_policy.rs > "$dest/css_policy.rs"
git -C "$repo" show HEAD:src/event_handlers.rs > "$dest/event_handlers.rs"
git -C "$repo" show HEAD:src/html_documents.rs > "$dest/html_documents.rs"
git -C "$repo" show HEAD:src/document_basics.rs > "$dest/document_basics.rs"
git -C "$repo" show HEAD:src/collection_records.rs > "$dest/collection_records.rs"
git -C "$repo" show HEAD:src/document_basics.js > "$dest/document_basics.js"
git -C "$repo" show HEAD:src/element_tokens.js > "$dest/element_tokens.js"
git -C "$repo" show HEAD:src/element_relations.js > "$dest/element_relations.js"
git -C "$repo" show HEAD:src/element_relations.rs > "$dest/element_relations.rs"
git -C "$repo" show HEAD:src/html_text.rs > "$dest/html_text.rs"
git -C "$repo" show HEAD:src/html_text.js > "$dest/html_text.js"
git -C "$repo" show HEAD:src/html_popover.js > "$dest/html_popover.js"
git -C "$repo" show HEAD:src/dom_mutations.rs > "$dest/dom_mutations.rs"
git -C "$repo" show HEAD:src/browser_sanitizer_default.json > "$dest/browser_sanitizer_default.json"
git -C "$repo" show HEAD:src/browser_env.rs | sed 's#^//!#//#' > "$dest/browser_env_upstream.rs"

sed "s/@TURBO_TEST_REV@/$rev/g" "$(dirname -- "$0")/browser-env-adapter.rs.in" > "$dest/browser_env.rs"

echo "vendored browser_env.{js,rs} ← $repo @ $rev (committed HEAD)."
echo "Adapter generated from template. Verify: cargo test -p turbo-surf-render"
