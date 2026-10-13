# Publishing turbo-surf

turbo-surf ships from two git-tag families across **three registries**. The engine
is a standalone Rust binary; the npm package is a thin launcher that spawns it (no
napi, no Node hosting Rust — same model as turbo-test).

| Workflow | Publishes | Where | Trigger tag | Auth secret |
|---|---|---|---|---|
| `release.yml` | `turbo-surf` — builds the `turbo-surf-mcp` binary for every platform, drops them in `bin/`, publishes the launcher (`cli.js`/`index.js` + `bin/`) | npm | `v*` | `NPM_TOKEN` |
| `rust-crates-publish.yml` | the Rust crates, in dep order: `core → view → page → render → raster → mcp` | crates.io | `v*` | `CARGO_REGISTRY_TOKEN` |
| `release-py.yml` | `turbo-surf` — maturin abi3 wheels (CPython 3.8+) per platform + sdist, built from `rust/crates/turbo-surf-py/` | PyPI | `pyv*` | `PYPI_TOKEN` |

The `v*` tag fires npm + crates; the **PyPI wheel ships on a SEPARATE `pyv*` tag**
(e.g. `pyv0.4.1`) so it never fires on the npm/crates release, and it skips cleanly
until `PYPI_TOKEN` is set. Cut BOTH tags at the same version for a full release.
(The `turbo-surf-napi` cdylib + `turbo-surf-transform` crate are not published — napi
is dev/harness-only now.)

## The one rule: every version string must match the tag

No single source of truth, so the bump must be applied everywhere before tagging,
or a workflow ships a mismatched version. Bump to the SAME `X.Y.Z`:

- `package.json` → `"version"`
- `rust/Cargo.toml` → `[workspace.package] version` **and** every crate's path-dep
  `version = "X.Y.Z"` (core/view/page/render/mcp/napi Cargo.toml)
- `rust/crates/turbo-surf-napi/package.json`
- `rust/crates/turbo-surf-py/pyproject.toml` → `version` (**maturin builds the PyPI
  wheel from THIS, not the workspace version** — if it lags, the publish job
  `--skip-existing`s the old wheel and goes green without publishing anything)
- `README.md` status line
- Root npm lockfile version and its root package version

Compiled core/MCP constants and NAPI/Python version functions use
`env!("CARGO_PKG_VERSION")`; they follow the workspace version automatically.

Check manifests and lockfile roots for the old version with `rg`; historical
changelog entries and unrelated dependency versions must retain their own versions.

## Cut a release

1. Bump all the version strings above to `X.Y.Z` (keep them identical).
2. Green gate locally: `cd rust && cargo test --workspace && cargo clippy --workspace
   --all-targets && cargo fmt --check`; from the root `npm run lint && npm run
   format:check` (the launcher JS).
3. Add the new version's entry to `CHANGELOG.md`.
4. Commit (`chore(release): vX.Y.Z`), then cut BOTH tags at the same commit:
   - `git tag -a vX.Y.Z -m vX.Y.Z` — npm + crates.
   - `git tag -a pyvX.Y.Z -m pyvX.Y.Z` — PyPI wheels.
   Push the commit **and** both tags: `git push origin <branch> && git push origin
   vX.Y.Z pyvX.Y.Z`. (Skip `pyvX.Y.Z` only if you deliberately aren't shipping Python.)
5. The workflows build + publish. Verify after CI:
   - `npm view turbo-surf version` (and that `bin/` shipped:
     `npm pack turbo-surf --dry-run`)
   - the crate pages on crates.io (`turbo-surf-core`, …)
   - `pip index versions turbo-surf` (or the PyPI project page)

Publishing is **outward-facing + irreversible** (npm + crates.io + PyPI versions
can't be reused) — only cut a tag when a release is intended.

## Notes

- The launcher's `files` allowlist ships `bin/` (the per-platform binaries CI
  builds), `cli.js`, `index.js`, `LICENSE`, `README.md`, `CHANGELOG.md`. `rust/`,
  `harness/`, `docs/` are excluded. Verify with `npm pack --dry-run`.
- `cli.js`/`index.js` resolve `bin/turbo-surf-mcp-<platform>-<arch>` at runtime
  (musl-detected on Linux), with a dev fallback to a local
  `rust/target/release/turbo-surf-mcp` build.
- crates publish in dependency order so each dependent resolves its just-published
  dep; path deps carry an explicit `version` so crates.io accepts them.
- `playwright` is a committed root dev dependency for the shim/tests; `patchright`
  is declared in `scripts/browser-sidecar/package.json`. Other comparison crawlers
  are installed ad-hoc. None is an engine runtime dependency.

## Current development transport and crate publishing

The Chrome 155 target profile uses the measured headers/client hints and the
pinned official Chrome 154 transport recipe from Git revisions of `wreq`,
`wreq-util` and `btls` in the workspace. It supplies Chromium root certificates,
real trust-anchor identifiers and signature-algorithm GREASE directly upstream;
the old local `rust/vendor/wreq` fork has been removed. `trust-anchors` is retained
as a compatibility feature alias; impersonation already includes real anchors.

Build development binaries with Rust 1.98.0 and the committed workspace lockfile
(`cargo build --locked`). This PR does not authorize a turbo-surf release.

Crates.io publishing remains blocked: workspace patches do not propagate to
library consumers, and registry `wreq` 0.16.1 / `wreq-util` 0.2.0 do not contain the
Chrome 154 APIs used here. Before a new crates release, require compatible
upstream registry releases or a properly packaged dependency fork, then verify
packaged crates from a clean consumer project without workspace patches. An npm
binary built from this workspace and a crates.io library have different dependency
resolution; successful binary builds do not establish registry compatibility.

The DOM API dependency is now published as turbo-dom 0.5.2 (PR #8, tagged merge
commit `87c2077`) and no longer needs a workspace Git patch. The transport API
requirement above still blocks clean crate consumers until compatible registry
versions or reviewed distributable forks are available. A local
`npm pack --dry-run` checks the launcher allowlist; it does not verify per-platform
binaries staged by release CI.
