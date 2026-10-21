# Portable official Deno extension JavaScript

The render crate embeds all 28 JavaScript modules declared by these exact
published registry dependencies:

| Rust alias | Package | Version | Archive SHA-256 |
| --- | --- | --- | --- |
| `deno_web` | `turbo-surf-deno-web` | 0.290.0 | `f8e0b0b8555dffbb5886b1c0f5eaa9f457667e3e80c3cb08894acb216d992b31` |
| `deno_webidl` | `turbo-surf-deno-webidl` | 0.259.0 | `a287b376e7f1f9fd2632cf6646f927c1dbe12bc8eb37a01c733204f63d729c95` |
| `deno_crypto` | `turbo-surf-deno-crypto` | 0.273.0 | `a6b4b4b54f568b45b7ad020556cc038402d70b80bfda1c161e9f3bbc451a8cd6` |

These packages retain the original Deno versions and namespaces. Crypto's Rust
provider still comes from its hardened published package; no provider or op is
replaced here. All embedded JS bytes are unchanged from the published archives.
`src/vendor/deno_extensions/pins.json` records each source list, full specifier,
byte length, source/declaration/license checksum and official archive URL. The
three complete upstream MIT licenses are retained in that same crate-local tree.
The root third-party notices also carry the Deno attribution and full MIT text.
WebIDL's unchanged source also retains its `webidl-conversions` BSD-2-Clause
header. The complete inherited BSD text and pinned upstream source are included
in this crate's root `THIRD_PARTY_NOTICES.md` and the synchronized Python notice.

Deno core 0.412 records extension JS as `LoadedFromFsDuringSnapshot` paths rooted
at the dependency's compile-time `CARGO_MANIFEST_DIR`. Its own core bootstrap is
embedded, but the Web/WebIDL/Crypto extension modules are not. Loading those
paths in an installed wheel outside the original build container can fail even
when the same artifact worked on the build host. This is independent of glibc,
C++ exception flags, CPython abi3 and the Python license policy.

`deno_extension_sources::embed` validates every official extension's complete
ordered `js_files`, `esm_files`, `lazy_loaded_js_files` and
`lazy_loaded_esm_files` inventory, then replaces their source storage with the
public `ExtensionFileSource::new` / `ascii_str_include!` static API. Missing,
extra, duplicate, reordered or moved specifiers fail before runtime creation.
It never reads or falls back to the recorded build-machine paths. WebIDL and Web
then pass through the existing three hash-checked private dense-array adapters;
embedding does not bypass or change their security boundary. Upstream source
versions/checksums are checked against the render manifest and lockfile by the
reproducer. There is no new snapshot pipeline or dependency release.

All generated sources, licenses and pins are inside this crate, so normal Cargo
packages and Python source distributions can include them without outside-file
references. The generated Rust table uses only crate-relative compile-time paths.
Raw upstream JS is excluded narrowly from repository format/lint rewriting.

From the repository root, verify cached published archives without downloads:

```sh
python3 rust/crates/turbo-surf-render/scripts/vendor-deno-extension-sources.py --check
```

Or supply one directory containing the three exact `.crate` archives:

```sh
python3 rust/crates/turbo-surf-render/scripts/vendor-deno-extension-sources.py \
  --archive-dir /absolute/path/to/registry-archives --check
```

`--write` reproduces the generated subtree and Rust table for a reviewed change.
Do not hand-edit those outputs or Cargo registry/cache files. A dependency update
requires reviewing its complete declared source inventory and updating the pins;
manifest/version/archive/source drift otherwise fails verification.

Relocation regressions and actual direct/sdist-rebuilt installed-wheel tests are
separate required validation. Build-host execution alone cannot prove the absence
of a dependency on source files that still happen to exist on that machine.
