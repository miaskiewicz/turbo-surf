# Exact V8 engine experiment

The published `deno_core 0.412.0` facade selects V8 15.0. The original Chrome 154.0.8037.98 oracle reported V8 `15.4.80.20`; installed Chrome has since updated to 155. Every run records its actual browser/V8 version. This isolated source build uses that actual official V8 tag; it does not change the version reported by an older engine. The C++/Rust SDK is ported from published rusty_v8 152.2.0 and records both versions separately.

This remains an experimental macOS arm64 profile. On 2026-10-13 its native smoke, both SDK tests, all-feature render/MCP gate (330 tests) and release-addon gates (83 shim tests with one skip, 20 E2E tests) passed. Production Cargo defaults still use the published SDK. No engine artifact is published by these scripts.

The build also runs the isolated `native_window_proxy.rs` regressions before recording validation. These use genuine V8 global detachment, one reused global proxy, and native ObjectTemplate access checks. They cover same-origin and cross-origin replacements, retained document isolation, retired Function access, and cross-origin reads/writes. The two SDK regressions pass, including denial of precompiled document-reading functions after cross-origin replacement. The browser runtime adopts these APIs when built by this helper (`TURBO_SURF_ENGINE=154`); 23 frame regressions pass, including stable proxy identity, fresh document ownership, cached caller-realm cross-origin methods/getters, restricted Location navigation/reflection, native ancestorOrigins lists, protected prototype writes, retained-function checks, iframe moves and sandbox/data origin policies. The independent localhost Chrome collectors are `../frame-location.mjs` and `../frame-policy.mjs`. Full renderer/MCP and release-addon gates have passed; live comparisons still need their own outcome and instrumentation checks. The published SDK fallback cannot use the newer access-check APIs.

```sh
python3 harness/detection/engine-154/prepare.py .native-builds/sdk154
python3 harness/detection/engine-154/build.py .native-builds/sdk154 --jobs 4
python3 harness/detection/engine-154/build.py .native-builds/sdk154 --surf test --manifest-path rust/Cargo.toml -p turbo-surf-render
```

Use a new persistent destination; the ignored `.native-builds/` directory avoids temporary-storage cleanup of extracted source files. Preparation checks downloaded crate SHA-256 hashes, checks out the exact engine revision, applies checked-in SDK/deno_core patches and writes a Cargo override plus provenance. The build checks the actual native version before permitting a surf command. Outputs and the several-gigabyte compiler/toolchain caches stay outside Git. A full source build requires network access, Python 3.12+, Rust 1.98+, a C++ toolchain and substantial free disk space. Tested platform is macOS arm64; other platform support is not claimed.

Apple Command Line Tools builds report their actual SDK metadata and use its real headers. They do not fabricate an Xcode installation. The compatibility patch omits the Xcode symlink bookkeeping action, since CLT headers are referenced directly.

The port replaces now-unsafe `Global::open` uses with scoped `Local::new`, uses the V8 154 Boolean accessor setter ABI, supplies the required embedder-data type tag, updates renamed prototype/holder methods, handles continuation data's new `Data` return type, and removes the deleted CppHeap termination API. Deno startup no longer supplies removed asm.js and finalized JavaScript feature flags. Bindgen uses the modern libclang bundled with the pinned Chromium Rust toolchain. Native exceptions and setter success must be checked by the regression suite; source compilation alone is insufficient evidence.

On 2026-10-10 the source build and native smoke completed and reported `15.4.80.20-rusty`. The native smoke and both WindowProxy SDK regressions passed, producing `validated.json`. Eighteen renderer API regressions passed on the matching engine before stable-proxy integration; the integrated navigation regression subsequently passed. Validation records the actual engine revision, SDK source patch hashes and native archive/binding hashes. This historical checkpoint preceded the completed 2026-10-13 renderer and release-addon gates; it has not changed the published Cargo default.

The first build saves its actual archive, generated bindings and source/platform hashes in `native-cache/`. Subsequent runs verify integrity and reuse that artifact, avoiding a repeat source compile. A native C++ source change invalidates the cache. To seed an existing source-built artifact explicitly, supply all three arguments:

```sh
python3 harness/detection/engine-154/build.py .native-builds/sdk154 \
  --archive /path/to/librusty_v8.a --bindings /path/to/src_binding.rs \
  --archive-sha256 ACTUAL_SHA256
```

The helper runs the version smoke and SDK context tests even when the archive is reused. Its `--surf` command sets the exact-engine runtime configuration automatically; that runtime refuses an older actual native version. SDK version numbers and browser User-Agent strings do not substitute for this check.

On 2026-10-13 temporary storage removed older extracted SDK files while retaining
recent edits. Missing files were restored from SHA-verified official archives without
overwriting modified sources. The persistent profile passed strict patch verification,
the real engine smoke and both context tests with the same B05 native archive. The
Cargo override is passed after the subcommand so external plugins such as Clippy
forward it to their inner build. Baseline and instrumented browser runs must still
record their actual versions: current installed Chrome is 155, while this profile is 154.

A fractional narrow-rectangle raster assertion exposed by the debug addon was
repaired with the antialiased path renderer and native pixel/PNG regressions.
The rebuilt debug addon completed two unmodified shim runs (83 passes, one skip).
An earlier isolated final-test hang remains unclassified; the repeated passing
runs do not establish that the intermittent hang was fixed.

The shared helpers also support the pinned [Chrome 155 profile](../engine-155/README.md). `prepare.py --profile 155` selects its manifest; the build helper derives the engine requirement from prepared provenance. The 154 profile and its existing artifacts remain available for regression comparisons.

### Registry-named Surf graphs (release aliases)

When Surf's render crate names the published distributions
(`turbo-surf-deno-core =0.412.1`, facade `turbo-surf-deno-v8 0.4.1`, native binding
`turbo-dom-v8 ~150.4`), `release_aliases.py` writes metadata-only alias manifests
in `<workspace>/sdk-release-aliases/`: the verified SDK originals (`deno_core`
0.412.0, `deno_v8` 0.4.0, `v8-exact` = rusty_v8 152.2.0) are checked at their own
versions and exposed as `turbo-surf-deno-core` 0.412.1, `turbo-surf-deno-v8` 0.4.1
and `turbo-dom-v8` 150.4.0 (library `v8`, `links = "rusty_v8"`). Source files are
symlinks; only manifests differ. The deno_core alias pins `turbo-surf-deno-v8
=0.4.1` / `turbo-surf-serde-v8 =0.321.1`; the facade alias points straight at the
`v8-exact` alias and, like the 0.4.1 distribution, drops the unused `quickjs`/
`v8x` option. The renamed native binding's build script uses `CARGO_PKG_VERSION`
only for download URLs, which `RUSTY_V8_ARCHIVE`/`RUSTY_V8_SRC_BINDING_PATH`
bypass. The generated `[patch.crates-io]` table is passed with `--config`, which
takes precedence over the workspace's own (e.g. PR Git-rev) patches for those three
names. A render requirement other than `=0.412.1` is refused rather than silently
resolving the stock registry engine.

This SDK lacks turbo-dom-v8's ShouldThrowOnError accessor fix, so the build helper
adds `--features native-sdk-frame-read` whenever a selected workspace member
declares that forwarding feature (render/mcp/napi/py forward it to
turbo-dom-browser-env, whose frame-read path then replaces the compile-time
`FORK_SHOULD_THROW_FIX` assertion). It refuses to run if no member declares it.
This path is exercised only by this manual macOS overlay.

SDK destination, with current source/fixtures linked from the repository. Cargo's
experimental SDK resolution cannot overwrite the production lockfile. The
`engine-workspace.test.mjs` offline gate verifies actual workspace resolution and
preserves source/manifest updates without resetting the profile-owned lock.


### Native code-generation callback

`code_generation.patch` adds a private per-context native V8 Function callback
for actual eval/Function admission, with original source and lexical semantics
preserved. Runtime registration forces callback routing after trusted bootstrap;
CSP parser/meta updates keep that routing while the private callback exists.
The code-generation smoke test verifies lexical context, ordinary object identity,
denial, public-symbol forgery and rejection of foreign-realm, bound or proxy
callbacks. The build helper records its successful gate.
Both new and existing SDK patches are checked with reverse application at zero
fuzz before an experiment is accepted. An older prepared SDK must receive the
new patch and a matching native archive; the previous archive cannot supply the
added C++ symbol. Production's stock SDK150 remains a separate unsupported mode.

The source-pinned Chrome CSP collector disables DevTools' default unsafe-eval
bypass. Exact native controls cover enforced/report-only policies, direct and
indirect eval, Function/AsyncFunction/GeneratorFunction, sample truncation,
directive fallback/intersection, live meta changes and actual task-quota recovery.
