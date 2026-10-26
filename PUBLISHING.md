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
(e.g. `pyv0.5.4`) so it never fires on the npm/crates release. A tagged PyPI
release fails if `PYPI_TOKEN` is absent; manual dispatch builds and tests without
publishing. Cut BOTH tags at the same version for a full release.
(The `turbo-surf-napi` cdylib + `turbo-surf-transform` crate are not published — napi
is dev/harness-only now.)

## The one rule: every version string must match the tag

No single source of truth, so the bump must be applied everywhere before tagging,
or a workflow ships a mismatched version. Bump to the SAME `X.Y.Z`:

- `package.json` → `"version"`
- `rust/Cargo.toml` → `[workspace.package] version` **and** every crate's path-dep
  `version = "X.Y.Z"` across all nine member manifests
- `rust/crates/turbo-surf-napi/package.json`
- `rust/crates/turbo-surf-py/pyproject.toml` → `version` (**maturin builds the PyPI
  wheel from THIS, not the workspace version** — if it lags, the publish job
  `--skip-existing`s the old wheel and goes green without publishing anything)
- `README.md` status line
- Root npm lockfile version and its root package version
- All nine workspace package versions in `rust/Cargo.lock`
- The standalone `rust/release-deps/surf-consumer/Cargo.toml` package and its
  six exact Surf dependency requirements

Compiled core/MCP constants and NAPI/Python version functions use
`env!("CARGO_PKG_VERSION")`; they follow the workspace version automatically.

Check manifests and lockfile roots for the old version with `rg`; historical
changelog entries and unrelated dependency versions must retain their own versions.

## Cut a release

1. Bump all the version strings above to `X.Y.Z` (keep them identical).
2. Add the new version's entry to `CHANGELOG.md`, then run `npm run check` from
   the root. This includes version consistency, dependency preparation controls,
   notices/audio source checks, workspace tests, strict Clippy, formatting, a
   rebuilt addon, shim/E2E tests and offline detection/Python controls. It also
   tests the release smoke supervisor and Deno source reproducer, then checks all
   28 embedded modules against the actual locked archives after Cargo's gate.
   Source verification uses the normal `CARGO_HOME` archive cache; the render
   crate's [provenance guide](./rust/crates/turbo-surf-render/DENO-EXTENSIONS.md)
   documents explicit archive-directory verification. Keep
   default/stock and optional native feature gates and their artifact identities
   separate. When reusing a prior result, retain a bridge proving its actual
   compiled inputs and resolved feature graph still match.
3. Complete the [actual packaged-consumer procedure](./rust/release-deps/surf-consumer/README.md).
   Publish and verify the named dependencies first (turbo-dom-v8 from turbo-dom,
   then the twelve here: seven Deno-family 0.x.1 + five transport); the Surf product
   tags do not publish those dependencies. Then verify all six Surf distributions
   in one normal invocation, with Cargo compilation verification enabled and no
   directory-source replacement:

   ```sh
   CARGO_HOME="$RELEASE_CARGO_HOME" CARGO_TARGET_DIR="$PACKAGE_TARGET" \
     cargo package --locked --manifest-path rust/Cargo.toml \
     -p turbo-surf-core -p turbo-surf-view -p turbo-surf-page \
     -p turbo-surf-render -p turbo-surf-raster -p turbo-surf-mcp
   ```

   Use a fresh Cargo home and the integrated registry dependency graph, retaining
   actual archive hashes. Run the actual-archive consumer and actual Python
   wheel/sdist consumers described there. A source-tree build or
   `cargo package --no-verify` does not substitute for these checks.
4. Commit the release preparation (`chore(release): vX.Y.Z`), pass the PR gates,
   then merge with a merge commit. Verify its tree contains the tested release
   changes. Verify its exact PR checks are green, both tag names are unused, and
   all versions agree with `python3 scripts/check-versions.py`. Set
   `FINAL_MERGE_SHA` to that verified full merge commit, then cut both annotated
   tags on it:

   ```sh
   git tag -a vX.Y.Z "$FINAL_MERGE_SHA" -m vX.Y.Z
   git tag -a pyvX.Y.Z "$FINAL_MERGE_SHA" -m pyvX.Y.Z
   git rev-parse 'vX.Y.Z^{commit}' 'pyvX.Y.Z^{commit}'
   git push --atomic origin vX.Y.Z pyvX.Y.Z
   ```

   The two peeled commits must equal `FINAL_MERGE_SHA`. Skip `pyvX.Y.Z` only if
   deliberately shipping without Python; never move a published tag for recovery.
5. The workflows build and publish. The npm and Python workflows reject tags
   that differ from release metadata. Each of the four npm build hosts sends an
   offline stdio `initialize`, loopback navigation and `evaluate` requests to its
   actual staged binary, with a 60-second execution deadline and private cwd
   without `.env`. It requires the exact release version and real DOM, native
   random-byte and stream behavior, with private Deno globals hidden. Initialization
   alone cannot establish that installed JavaScript modules are usable.
   Before publication, npm packs the assembled package, verifies its integrity,
   all four binaries, launchers and license files, extracts that exact tarball
   outside the checkout and repeats the same three-request JavaScript smoke through
   its packaged Node CLI using `scripts/smoke-mcp-release.py`.
   Only a tag-push event passes the verified tarball to `npm publish`; receipts
   retain hashes, actual artifact paths and subprocess results. Manual dispatch
   runs the same four builds and assembled-tarball smoke without publishing,
   including when dispatched against a tag. The Rust crates workflow instead
   treats manual dispatch as publication: select the exact matching `v*` tag;
   it checks release metadata and uses `cargo publish --locked` for all six crates.
   Do not dispatch that workflow as a dry run. Every Python platform job installs its
   exact wheel outside the checkout and requires all ten existing offline smoke
   tests to pass without skips. The sdist job builds a wheel from the actual
   source tarball with an empty Cargo home and the normal registry, then installs
   and tests that wheel before uploading the sdist. Retain artifact hashes,
   installed extension paths and the uploaded smoke receipts; a successful build
   alone is insufficient. Linux Python wheels explicitly target
   `manylinux_2_28` on both native x86_64 and aarch64 hosts, requiring glibc 2.28
   or later. This Linux baseline is separate from the unchanged CPython 3.8+ abi3
   contract. Before shipping, require actual manylinux symbol-compliance checks
   and installed-wheel tests on both Linux architectures. Retain `linux-elf.json`
   from each wheel-smoke artifact: it records the actual wheel/ELF hashes, raw
   bounded `readelf` output, required GLIBC/GLIBCXX/CXXABI versions and DT_NEEDED
   entries for the extension and any bundled shared libraries. Changing the image
   alone does not prove the prebuilt V8 archive or resulting wheel is compatible.
6. Verify actual registry availability and payloads after CI, keeping accepted
   uploads and propagation delays separate from completed verification:
   - Fetch `turbo-surf@X.Y.Z` from npm, verify its integrity and all four platform
     binaries, and smoke the installed launcher on a compatible host. A local
     `npm pack --dry-run` or `npm view ... version` alone is insufficient.
   - Fetch all six `X.Y.Z` crate archives and checksums and repeat the consumer
     test using a new empty Cargo home, no root patches or source replacement.
   - Fetch the exact PyPI wheels and sdist, verify their hashes and versions,
     and install/test the matching-platform wheel. PyPI's `--skip-existing` is
     retry behavior, not proof that an existing file has the reviewed payload.

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
all-feature artifact. Standalone Python wheels do not select core's
`impersonate` feature: their resource client uses reqwest/rustls, with software
rasterization and the stock V8 backend. A whole-workspace build can unify core
features through MCP/NAPI, so it does not establish the standalone wheel profile.
Verify the actual wheel graph separately. The actual sdist must contain every
required source/path dependency and build against
the published registry graph; missing workspace patches must not silently select
unhardened upstream dependencies.

## Current development transport and crate publishing

The Chrome155 target profile uses the measured headers/client hints and the
pinned official Chrome154 transport recipe. The reviewed source provenance is
retained at Git revisions of `wreq`, `wreq-util` and `btls`; release consumers
must use the corresponding named registry distributions. It supplies Chromium root certificates,
real trust-anchor identifiers and signature-algorithm GREASE directly upstream;
the old local `rust/vendor/wreq` fork has been removed. `trust-anchors` is retained
as a compatibility feature alias; impersonation already includes real anchors.

Native HTTP/2 stream-upload admission additionally requires the existing
connection metadata trait to be public. The named `turbo-surf-wreq` 0.16.1 distribution preserves the minimal additive
fork [`greg-flux/wreq` at `4ed3cf0cd4c14c58c557fdd3f3ff6afa4ae24762`](https://github.com/greg-flux/wreq/commit/4ed3cf0cd4c14c58c557fdd3f3ff6afa4ae24762),
based exactly on upstream `c6fd8b1acfa96bc0175e2825c88ccda28e23d9b5`, while
[upstream PR #1327](https://github.com/0x676e67/wreq/pull/1327) proposes that API.
Only the public re-export and its external regression differ; TLS, ALPN and HTTP
implementation are unchanged. The unchanged upstream package version identifies
that baseline, not a new registry release. The reviewed distributable transport packages preserve this
source under the named `turbo-surf-*` dependency family. Core and render consume
those published packages through seven explicit Cargo aliases; there are no root
workspace patches. The original source revisions remain provenance, rather than
downstream dependency overrides.

Build development binaries with Rust 1.98.0 and the committed workspace lockfile
(`cargo build --locked`). Release authorization does not establish registry
compatibility; resolve the concrete prerequisites below before tagging.

The October21 core package preflight (`cargo package -p turbo-surf-core
--no-verify --locked`) fails before compilation: registry wreq0.16.1 does not
provide the requested `chromium-roots` feature. `--no-verify` here only skips
compilation for the diagnostic; it is not a passing package-consumer gate.

That diagnostic predates the renamed dependency integration and is not a verdict
on the final registry graph. Registry `wreq` 0.16.1 / `wreq-util` 0.2.0 do not
contain the APIs used here. Verify the integrated renamed packages through actual
Surf distributions and a clean consumer without workspace patches. An npm binary
built from a patched workspace does not establish registry compatibility.

The reviewed `deno_crypto`0.273.0 source includes the licensed, reproducible
hardening patch in [`rust/vendor/DENO_CRYPTO.md`](./rust/vendor/DENO_CRYPTO.md).
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
respectively. The initial DOM/layout-only update changed these two records.
The subsequent named dependency integration replaces twelve runtime packages at
their existing versions and preserves all other lock records apart from the
core/render dependency references. The previously tested Git candidates remain
dated provenance, not current dependency metadata.

The reviewed stock RustyV8150.4.0 source includes native weak-handle teardown
hardening. That ownership fix is required by the retained regression; silently
resolving the unpatched upstream crate would lose it. The release graph must use
the reviewed named V8 distribution and its matching transitive deno_v8/deno_core
packages.
See [rust/vendor/RUSTY_V8.md](./rust/vendor/RUSTY_V8.md). Workspace binary tests
cannot substitute for clean downstream package tests.

All thirteen named dependency distributions are now published and payload-verified
by [the exact reviewed dependency run](https://github.com/miaskiewicz/turbo-surf/actions/runs/39590311206).
Its thirteen normal package verifications and eleven fresh registry consumer
tests pass without ignored tests. An independent audit compares every actual
archive with that run's own verified plan, including runtime/license bytes and
complete normalized manifest semantics. The integrated lockfile uses their real
registry checksums. Actual Surf archives, wheels and source-tarball consumers
remain separate product release gates below.

Source preparation, publication and successful consumers are separate checks.
Require the actual named dependency versions to be available and verified before
product publication. A local `npm pack --dry-run` checks the launcher allowlist;
it does not verify per-platform binaries staged by release CI.

## Distributable dependency preparation

[`rust/release-deps/`](./rust/release-deps/README.md) prepares five renamed
transport packages and the seven-package Deno/crypto closure (0.x.1, upstream + 1
patch), including the benchmark dependency needed to retain test type identity.
The native V8 binding is the external prerequisite `turbo-dom-v8` 150.4.x,
published from the turbo-dom repository before the Deno family; `turbo-surf-v8`
is retired (its published versions stay untouched). Before release, consumer PRs
`[patch.crates-io]` the candidates by Git `rev` from the orphan branch
`consolidation/deno-candidates` (`rust/release-deps/push-candidates.sh`). Source and license
checks, actual normalized archives, compilation/runtime controls and real
published-consumer checks are distinct prerequisites. Preparing an archive with
`--no-verify` is not a passing compile or registry verdict. None of these
preparation tools silently publishes packages.

`release-dependencies.yml` verifies all twelve distributions before its
separate publish step; its verify step stays red until turbo-dom-v8 is on
crates.io (documented pre-publication exception). It prepares hash-pinned sources, produces and compiles
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

All nine Rust packages inherit SPDX `MIT` metadata and contain local `LICENSE`
files. `sync-third-party-notices.py` keeps those files and `rust/LICENSE`
byte-identical to the root `LICENSE`. Cargo discovers the local files inside
its workspace without duplicate license-field warnings. Python packaging uses
PEP 639 `license = "MIT"` and explicit `license-files = ["LICENSE"]`, with
maturin 1.9.3 or later (the workflow pins the tested 1.15.0). The explicit file
survives maturin moving `pyproject.toml` to an sdist workspace root; implicit
crate-local discovery alone did not. Both direct and sdist-rebuilt wheel jobs
verify the full MIT and third-party notice bytes against the tagged source,
alongside installed tests. Actual wheel/sdist and crate audits remain required.

The native audio kernels, XPath provider, RustFFT and hardened crypto provider
retain their source licenses. `THIRD_PARTY_NOTICES.md` also accompanies the npm
launcher, standalone platform artifacts and Python extension package. After
editing the notices, run `python3 scripts/sync-third-party-notices.py` to update
the render-crate and Python package copies. `--check` runs before release builds
and in `npm test`; packaging must preserve these notices in the actual archives.
