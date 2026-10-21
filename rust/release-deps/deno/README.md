# Distributable Deno/V8 dependency closure

These are preparation tools for eight explicitly named registry distributions.
They do not modify Cargo's cache, publish packages or replace the Surf workspace's
dependencies. Production consumers require seven packages; the eighth preserves
the upstream web/WebIDL benchmark and test graph without a second Deno/V8 runtime.

| Publish order | Distribution | Upstream version | Internal dependency aliases |
| --- | --- | --- | --- |
| 1 | `turbo-surf-v8` | 150.4.0 | none |
| 2 | `turbo-surf-deno-v8` | 0.4.0 | `rusty_v8` → forked V8 |
| 3 | `turbo-surf-serde-v8` | 0.321.0 | `v8` → forked facade |
| 4 | `turbo-surf-deno-core` | 0.412.0 | `v8`, `serde_v8` → forks |
| 5 | `turbo-surf-deno-bench-util` | 0.253.0 | `deno_core` → fork |
| 6 | `turbo-surf-deno-crypto` | 0.273.0 | `deno_core` → fork |
| 7 | `turbo-surf-deno-webidl` | 0.259.0 | `deno_core`, benchmark utility → forks |
| 8 | `turbo-surf-deno-web` | 0.290.0 | core, WebIDL and benchmark utility → forks |

Keep the original library names and dependency aliases. Rust handles, op macros,
serde conversions and native CryptoKey snapshots must share one runtime type
identity. All internal requirements are exact versions. Local candidate paths
are preparation conveniences; Cargo's actual packaged manifests must retain the
named registry dependencies and remove paths. No root `[patch]` may be required.

The new package names distinguish these distributions from the upstream releases.
The initial numeric versions intentionally match upstream. V8's unchanged build
script derives its official native archive and generated-binding URLs from
`CARGO_PKG_VERSION`; changing that version without separating the upstream native
archive identity would break downloads. Native engine identity remains V8 150;
these packages do not contain the experimental V8 155 SDK.

## Reproduce source candidates

`source-pins.json` binds exact published archive hashes and every reviewed local
V8/crypto source file. The local vendor sources contain the existing teardown and
crypto security/clone fixes. `v8-teardown-tests.patch` updates the old contradictory
slot-liveness assertion and adds ordinary/snapshot weak-slot retirement controls;
it changes no runtime code. Original archive packaging records are retained under
`UPSTREAM-*` names. MIT license files are preserved or copied from the reviewed
Deno license; V8's broad license exclusion is removed from its candidate manifest.

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
archives, source links and unexpected vendor/test changes are rejected. No archive
is written into Cargo's cache. V8's default candidate retains the reviewed
prebuilt SDK subset, so `V8_FROM_SOURCE` fails explicitly without the separately
verified complete upstream C++ sources.

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
workspace resolver, checks actual source and license bytes and requires every
Rust/embedded JavaScript file. A fresh consumer must use only those normalized
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
bodies and assertions, with a separate binary's serialized platform setup. The
upstream `test_api` target cannot compile from its published archive because its
setup includes the absent `third_party/icu/common/icudtl.dat`. That harness
limitation remains explicit; the independent consumer does not add guessed ICU
data or claim that the full upstream test target passed.

Prepublication directory-source success proves the actual archives work together;
it does not prove crates.io serves them. Publish only after release authorization
and required gates. Publish in dependency order, then repeat a clean consumer
against the real registry without the directory-source replacement. Stock V8 and
the experimental native155 overlay require separate integration validation before
claiming that both engine configurations remain supported.
