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
- `rust/crates/turbo-surf-core/src/lib.rs` + `turbo-surf-mcp/src/lib.rs` → `VERSION`
- `rust/crates/turbo-surf-napi/src/lib.rs` → `version()` and its `package.json`
- `rust/crates/turbo-surf-py/pyproject.toml` → `version` (**maturin builds the PyPI
  wheel from THIS, not the workspace version** — if it lags, the publish job
  `--skip-existing`s the old wheel and goes green without publishing anything)
- `rust/crates/turbo-surf-py/src/lib.rs` → the `version()` pyfunction's returned
  string (the Python module's self-reported version)
- `README.md` status line

Sanity check (should print nothing): `grep -rn "<old-version>" package.json rust/
README.md | grep -v /target/`.

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
- Browser/crawler packages used only by the harness (`playwright`, `crawlee`, …) are
  not committed deps — install ad-hoc to run the benchmarks.

## Caveat: the `trust-anchors` feature + vendored wreq

`turbo-surf-core`'s optional `trust-anchors` feature depends on a **vendored fork of
`wreq`** (`rust/vendor/wreq`) wired via `[patch.crates-io]` in `rust/Cargo.toml`. A
`[patch]` is **workspace-local — it is NOT carried into a published crate**. So:

- The shipped `turbo-surf-mcp` binary (built from this workspace) gets the patch and
  the feature works — including if you build the binary with `--features trust-anchors`.
- A crates.io **library** consumer of `turbo-surf-core` who enables `trust-anchors`
  resolves **upstream** `wreq` (no `trust_anchors` field) and **fails to compile**.
  The default (feature-off) crate is unaffected and publishes/builds normally.
- `cargo publish` of `turbo-surf-core` still succeeds (its default-feature verify build
  never references the patched field). The feature is simply binary-only until `wreq`
  ships `trust_anchors` upstream, at which point drop the vendored fork + `[patch]`.
