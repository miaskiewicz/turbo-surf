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
4. Commit the release preparation (`chore(release): vX.Y.Z`), pass the PR gates,
   then merge with a merge commit. Verify its tree contains the tested release
   changes and cut BOTH tags on that final merge commit:
   - `git tag -a vX.Y.Z -m vX.Y.Z` — npm + crates.
   - `git tag -a pyvX.Y.Z -m pyvX.Y.Z` — PyPI wheels.
   Push the annotated tags together: `git push --atomic origin vX.Y.Z pyvX.Y.Z`.
   (Skip `pyvX.Y.Z` only if you deliberately aren't shipping Python.)
5. The workflows build + publish. Verify after CI:
   - `npm view turbo-surf version` (and that `bin/` shipped:
     `npm pack turbo-surf --dry-run`)
   - the crate pages on crates.io (`turbo-surf-core`, …)
   - `pip index versions turbo-surf` (or the PyPI project page)

Publishing is **outward-facing + irreversible** (npm + crates.io + PyPI versions
can't be reused) — only cut a tag when a release is intended.

## Notes

- The launcher's `files` allowlist ships `bin/` (the per-platform binaries CI
  builds), `cli.js`, `index.js`, `LICENSE`, `THIRD_PARTY_NOTICES.md`, `README.md`,
  `CHANGELOG.md`. Platform binary artifacts also carry both license files. `rust/`,
  `harness/`, `docs/` are excluded. Verify with `npm pack --dry-run`.
- `cli.js`/`index.js` resolve `bin/turbo-surf-mcp-<platform>-<arch>` at runtime
  (musl-detected on Linux), with a dev fallback to a local
  `rust/target/release/turbo-surf-mcp` build.
- crates publish in dependency order so each dependent resolves its just-published
  dep; path deps carry an explicit `version` so crates.io accepts them.
- `playwright` is a committed root dev dependency for the shim/tests; `patchright`
  is declared in `scripts/browser-sidecar/package.json`. Other comparison crawlers
  are installed ad-hoc. None is an engine runtime dependency.

## Shipping profiles

The current npm release workflow builds the stock workspace with
`cargo build --locked --release -p turbo-surf-mcp`. It enables default transport
impersonation, uses the patched stock V8 150 backend and does not enable the
experimental Chrome155 SDK, GPU bridge or CoreText feature union used in the
native parity captures. Do not describe release CI outputs as that measured
all-feature artifact. Python wheels use the PyO3 crate's default profile; unpack
and build the actual sdist to verify that its sources and required workspace
patches survive packaging.

## Current development transport and crate publishing

The Chrome 155 target profile uses the measured headers/client hints and the
pinned official Chrome 154 transport recipe from Git revisions of `wreq`,
`wreq-util` and `btls` in the workspace. It supplies Chromium root certificates,
real trust-anchor identifiers and signature-algorithm GREASE directly upstream;
the old local `rust/vendor/wreq` fork has been removed. `trust-anchors` is retained
as a compatibility feature alias; impersonation already includes real anchors.

Native HTTP/2 stream-upload admission additionally requires the existing
connection metadata trait to be public. The workspace pins the minimal additive
fork [`greg-flux/wreq` at `4ed3cf0cd4c14c58c557fdd3f3ff6afa4ae24762`](https://github.com/greg-flux/wreq/commit/4ed3cf0cd4c14c58c557fdd3f3ff6afa4ae24762),
based exactly on upstream `c6fd8b1acfa96bc0175e2825c88ccda28e23d9b5`, while
[upstream PR #1327](https://github.com/0x676e67/wreq/pull/1327) proposes that API.
Only the public re-export and its external regression differ; TLS, ALPN and HTTP
implementation are unchanged. The unchanged package version identifies that
upstream baseline, not a new registry release. The Git revision supports native
workspace binaries; publishing metadata strips Git pins and registry wreq lacks
this seam too. Do not publish Surf crates until a compatible registry dependency
path passes clean packaged-consumer checks, and do not infer authorization to
publish a differently named dependency crate.

Build development binaries with Rust 1.98.0 and the committed workspace lockfile
(`cargo build --locked`). Release authorization does not establish registry
compatibility; resolve the concrete prerequisites below before tagging.

The October21 core package preflight (`cargo package -p turbo-surf-core
--no-verify --locked`) fails before compilation: registry wreq0.16.1 does not
provide the requested `chromium-roots` feature. `--no-verify` here only skips
compilation for the diagnostic; it is not a passing package-consumer gate.

Crates.io publishing remains blocked: workspace patches do not propagate to
library consumers, and registry `wreq` 0.16.1 / `wreq-util` 0.2.0 do not contain the
Chrome 154 APIs used here. Before a new crates release, require compatible
upstream registry releases or a properly packaged dependency fork, then verify
packaged crates from a clean consumer project without workspace patches. An npm
binary built from this workspace and a crates.io library have different dependency
resolution; successful binary builds do not establish registry compatibility.

The render workspace also patches `deno_crypto` 0.273.0 with the licensed source
and reproducible hardening patch in [`rust/vendor/DENO_CRYPTO.md`](./rust/vendor/DENO_CRYPTO.md).
The upstream provider's inherited object setters can expose non-extractable key
bytes during construction. Registry consumers must resolve a distributable
hardened provider or an upstream release containing the fix before publishing
Surf crates; silently falling back to the unpatched registry crate is unsafe.
Check the source archive/patch with `python3 rust/vendor/update-deno-crypto.py --check`.

The DOM and layout release prerequisites are now satisfied. All six DOM consumers
resolve registry turbo-dom0.5.7, published from tagged merge
`5ec170654313bfdc8ab59db78b6e848e85f6f265` after all open producer PRs merged.
Render and raster resolve registry turbo-html2pdf-core0.3.8, published from
`9591fe69e304e89ba4899fcc54c637059f2b121d`, including composed layout and shared
font programs. The lockfile verifies checksums
`4c759ebdc434d90ce8ba1301ff94a5b17553b61c594de60118e0af8d337f25e4`
and `cbb57210ff0331d026dbd66a5122d778d48a034753088b50b3191e6b855a4eb9`
respectively. Only these two source/checksum records change in the dependency
update; other lock entries remain unchanged. The previously tested Git candidates
remain dated provenance, not current dependency metadata.

The default engine also patches Rusty V8 150.4.0 for native weak-handle teardown.
That ownership fix is required by the retained regression; silently resolving the
unpatched registry crate would lose it. A distributable dependency path or fixed
upstream release is required, including its transitive deno_v8/deno_core users.
See [rust/vendor/RUSTY_V8.md](./rust/vendor/RUSTY_V8.md). Workspace binary tests
cannot substitute for clean downstream package tests.

The transport API requirement above still blocks clean crate consumers until
compatible registry versions or reviewed distributable forks are available. A local
`npm pack --dry-run` checks the launcher allowlist; it does not verify per-platform
binaries staged by release CI.

## Distributable dependency preparation

[`rust/release-deps/`](./rust/release-deps/README.md) prepares five renamed
transport packages and the eight-package Deno/V8/crypto closure, including the
benchmark dependency needed to retain test type identity. Source and license
checks, actual normalized archives, compilation/runtime controls and real
published-consumer checks are distinct prerequisites. Preparing an archive with
`--no-verify` is not a passing compile or registry verdict. None of these
preparation tools silently publishes packages.

`release-dependencies.yml` verifies all thirteen distributions before its
separate publish step. It prepares hash-pinned sources, produces and compiles
actual normalized archives, runs both consumers and guards staged source bytes.
Publication keeps Cargo verification enabled, handles registry propagation and
quota responses, and compares existing versions against the reviewed payload.
Fresh downstream consumers use an empty Cargo home and the real registry.

The workflow does not run from product tags. Its explicit release branch is
`codex/release-dependencies-v054`; a push can publish only when repository
variable `DEPENDENCY_RELEASE_COMMIT` equals that full reviewed commit. The
maintainer sets that variable before pushing the exact commit to the release
branch. Manual dispatch defaults to verification and requires an exact source
commit; publishing additionally requires the explicit publish mode. Secrets are
read only by the publish step. Retain uploaded receipts on success or failure;
never treat an accepted upload alone as completed registry verification.

## Native third-party notices

The native audio kernels, XPath provider, RustFFT and hardened crypto provider
retain their source licenses. `THIRD_PARTY_NOTICES.md` also accompanies the npm
launcher, standalone platform artifacts and Python extension package. After
editing the notices, run `python3 scripts/sync-third-party-notices.py` to update
the render-crate and Python package copies. `--check` runs before release builds
and in `npm test`; packaging must preserve these notices in the actual archives.
