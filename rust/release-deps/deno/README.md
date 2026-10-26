# Distributable Deno dependency closure

These are preparation tools for seven explicitly named registry distributions.
They do not modify Cargo's cache, publish packages or replace the Surf workspace's
dependencies. Production consumers require six packages; the seventh preserves
the upstream web/WebIDL benchmark and test graph without a second Deno/V8 runtime.
The native binding is the external prerequisite `turbo-dom-v8` 150.4.x, published
from the turbo-dom repository (it carries the reviewed teardown and
ShouldThrowOnError fixes formerly prepared here as `turbo-surf-v8`).

| Publish order | Distribution | Version (upstream) | Internal dependency aliases |
| --- | --- | --- | --- |
| — | `turbo-dom-v8` (turbo-dom) | 150.4.x | prerequisite, not prepared here |
| 1 | `turbo-surf-deno-v8` | 0.4.1 (0.4.0) | `rusty_v8` → registry `turbo-dom-v8 ~150.4` |
| 2 | `turbo-surf-serde-v8` | 0.321.1 (0.321.0) | `v8` → forked facade |
| 3 | `turbo-surf-deno-core` | 0.412.1 (0.412.0) | `v8`, `serde_v8` → forks |
| 4 | `turbo-surf-deno-bench-util` | 0.253.1 (0.253.0) | `deno_core` → fork |
| 5 | `turbo-surf-deno-crypto` | 0.273.1 (0.273.0) | `deno_core` → fork |
| 6 | `turbo-surf-deno-webidl` | 0.259.1 (0.259.0) | `deno_core`, benchmark utility → forks |
| 7 | `turbo-surf-deno-web` | 0.290.1 (0.290.0) | core, WebIDL and benchmark utility → forks |

Keep the original library names and dependency aliases. Rust handles, op macros,
serde conversions and native CryptoKey snapshots must share one runtime type
identity. All internal requirements are exact versions; the facade's native requirement is
`~150.4` so a turbo-dom-v8 patch release needs no facade release. Local candidate paths
are preparation conveniences; Cargo's actual packaged manifests must retain the
named registry dependencies and remove paths. No root `[patch]` may be required.

The four extension/benchmark leaves (crypto, WebIDL, web and benchmark utility)
explicitly default to their own `v8` feature, forwarding `deno_core/v8`. Their
core dependency still disables defaults and retains its upstream feature list.
This repairs standalone default package builds, which previously depended on an
enclosing consumer to select V8. Callers can disable a leaf's defaults rather than
having V8 forced unconditionally. This is not validation of a QuickJS backend.

The new package names distinguish these distributions from the upstream releases.
Their versions are upstream + 1 patch; each manifest records the upstream name and
version in `[package.metadata.turbo-surf-upstream]`, and preparation verifies the
downloaded archive at that exact upstream identity. Native engine identity remains
V8 150 (turbo-dom-v8); these packages do not contain the experimental V8 155 SDK.

The 0.4.1 facade drops upstream's unused optional `v8x` second backend and its
`quickjs` feature, so no lock file lists a second native engine; serde_v8 and
deno_core lose the `quickjs` features that only forwarded to it (Cargo rejects a
feature naming a dependency feature that no longer exists, even when unused). The
facade keeps its `v8`, `simdutf`, `use_custom_libcxx` and `v8_enable_*`
passthroughs, now forwarding only to turbo-dom-v8.

### Embedded extension sources

`turbo-surf-deno-web`, `-deno-webidl` and `-deno-crypto` add one generated module,
`embedded_sources.rs`, declared by one appended line in `lib.rs`
(`pub mod embedded_sources;`). It exposes every JS file the extension declares, in
declaration order, compiled into the binary:

```rust
pub enum SourceKind { Js, Esm, LazyJs, LazyEsm }
pub struct EmbeddedSource {
    pub extension: &'static str,             // e.g. "deno_web"
    pub kind: SourceKind,                    // which Extension source list
    pub specifier: &'static str,             // "ext:deno_web/00_infra.js"
    pub code: deno_core::FastStaticString,   // ascii_str_include!("00_infra.js")
    pub sha256: &'static str,
    pub bytes: usize,
}
impl EmbeddedSource { pub const fn file_source(&self) -> deno_core::ExtensionFileSource }
pub const EXTENSION: &str;
pub const LICENSE: &str;                     // include_str!("LICENSE.md")
pub const LICENSE_SHA256: &str;
pub const SOURCES: &[EmbeddedSource];
```

Preparation checks the official `lib.rs` declaration hash, the source inventory
(kind and file, in order), every file's SHA-256/length/ASCII and the license
against `embeddedSources` in `source-pins.json`; these pins equal the bytes
turbo-surf-render vendors today (a unit test cross-checks them while that copy
exists). Embedders can therefore substitute these bytes for upstream's build-path
sources without vendoring them.

## Reproduce source candidates

`source-pins.json` binds exact published archive hashes, every reviewed local
crypto source file and the embedded-source inventories. The local vendor source
(`rust/vendor/deno_crypto`) contains the existing crypto security/clone fixes.
Original archive packaging records are retained under `UPSTREAM-*` names. MIT
license files are preserved or copied from the reviewed Deno license.

From the repository root, choose a fresh private destination:

```sh
python3 rust/release-deps/deno/prepare.py \
  --archive-dir .native-builds/release-deno-upstream-archives --download \
  --output .native-builds/release-deno-candidates
python3 rust/release-deps/deno/prepare.py \
  --archive-dir .native-builds/release-deno-upstream-archives \
  --output .native-builds/release-deno-candidates --check
python3 -m unittest discover -s rust/release-deps/deno -p 'test_*.py'
```

An existing verified Cargo archive may be read or copied into the private archive
directory. CI can fetch missing exact HTTPS archives there using `--download`.
Every archive is checked against its committed SHA-256 before extraction; changed
archives, source links and unexpected vendor changes are rejected. No archive
is written into Cargo's cache.

`rust/release-deps/push-candidates.sh` publishes nothing: it commits the prepared
candidates (registry-form manifests, see `candidate_tree.py`) to the orphan branch
`consolidation/deno-candidates` so a consumer PR can `[patch.crates-io]` them by
Git `rev` before release.

## Package and validate before publishing

Create actual archives with `cargo package --no-verify` against a private bootstrap
source stage, then stage the resulting `.crate` archives using
`rust/release-deps/stage.py --candidate-archive`. Bootstrap source-path checks are
not packaged-consumer proof. Audit those archives against the candidate identity:

```sh
python3 rust/release-deps/deno/check_archives.py \
  --prepared .native-builds/release-deno-candidates \
  --archives .native-builds/release-deno-packages \
  --receipt .native-builds/release-deno-archive-audit.json
```

The archive check permits Cargo's removed dependency paths and materialized
workspace resolver, checks actual source and license bytes, requires every
Rust/embedded JavaScript file and rejects any edge to a retired native V8
(`v8`, `v8x`, `turbo-surf-v8`). A fresh consumer must use only those normalized
archives, no candidate paths or workspace patches, and demonstrate shared facade/
core/serde types, JsRuntime execution and the patched native CryptoKey snapshot API.
The reusable `consumer/` fixture exercises those actual public types, benchmark
runtime construction, arithmetic and native AES-key snapshot/restore with an
inherited private-key setter trap. Copy it to a fresh private directory and run
its Cargo tests using only the actual-archive source stage configuration.
Run the actual V8 weak-slot regression for normal and snapshot teardown paths,
and the appropriate upstream/native crypto regressions. Check staged source
checksums before and after execution, including linked registry inputs.

`consumer/tests/v8_teardown.rs` retains the two reviewed slot/weak regression
bodies and assertions, with a separate binary's serialized platform setup; they now
exercise turbo-dom-v8 through the facade (the full upstream V8 test targets live
with turbo-dom-v8). `consumer/tests/embedded_sources.rs` checks every
`embedded_sources::SOURCES` table against its extension's declared source lists.

Prepublication directory-source success proves the actual archives work together;
it does not prove crates.io serves them. Publish only after release authorization
and required gates. Publish turbo-dom-v8 from turbo-dom first, then these in dependency order, then
repeat a clean consumer against the real registry without the directory-source
replacement; it must resolve exactly one registry `turbo-dom-v8` 150.4.x and no
`v8`, `v8x` or `turbo-surf-v8`. Stock V8 and
the experimental native155 overlay require separate integration validation before
claiming that both engine configurations remain supported.
