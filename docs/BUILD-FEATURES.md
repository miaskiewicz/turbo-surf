# Build features and artifact profiles

This is the build contract for the eight-crate workspace. Cargo features are
compile-time, additive switches; installing the npm launcher or setting a runtime
environment variable cannot change features in an existing binary or addon.
See [GPU bridge](GPU-BRIDGE.md) for GPU execution details and
[Publishing](../PUBLISHING.md) for release authorization and artifact verification.

## Defaults by crate

The declarations below come from the [crate manifests](../rust/crates). A
dependency's features can also be enabled by another package in the same Cargo
invocation. The workspace release profile uses `opt-level = 3` and thin LTO;
`--release` changes optimization, not the enabled feature set.

| Crate | Declared default features | Other declared features |
| --- | --- | --- |
| `turbo-surf-core` | `impersonate` | `trust-anchors` |
| `turbo-surf-page` | `impersonate` | none |
| `turbo-surf-view` | no feature table | none |
| `turbo-surf-transform` | no feature table | none |
| `turbo-surf-render` | none | `native-sdk-frame-read` |
| `turbo-surf-mcp` | `impersonate`, `avif`, `jxl` | `trust-anchors`, `gpu-metal`, `coretext`, `native-sdk-frame-read` |
| `turbo-surf-napi` | `impersonate`, `avif`, `jxl` | `gpu-metal`, `coretext`, `native-sdk-frame-read` |
| `turbo-surf-py` | `avif`, `jxl` | `gpu-metal`, `coretext`, `native-sdk-frame-read` |

`trust-anchors` is not a separate NAPI, page, Python or render feature. There is
no Surf Cargo feature named `rustls`, `gpu-bridge`, `webgpu`, `webgl`, or
`chrome155`. Feature names are package-specific; prefix them with the package
name when enabling them across multiple selected packages.

## What the switches enable

| Feature | Forwarding and boundary |
| --- | --- |
| `impersonate` | Core selects the optional BoringSSL-backed `wreq` stack. Page and MCP forward to core; NAPI forwards to core and page. The transport uses the pinned Chrome 154 emulation recipe; this does not change the V8 engine. |
| `trust-anchors` | Core compatibility alias for `impersonate`; MCP forwards to core's alias. The impersonation profile already includes Chromium trust-anchor IDs, so the alias adds no separate transport mode. |
| `avif` | MCP/NAPI/Python forward to `turbo-dom-raster/avif`, enabling AVIF pixel decoding for synthetic screenshots. The raster dependency itself does not enable this by default; the three Surf crates do. Disabling pixel decoding does not disable image header/natural-size probing. |
| `jxl` | MCP/NAPI/Python forward to `turbo-dom-raster/jxl`, enabling JPEG XL screenshot pixel decoding. It is separate from natural-size probing and is on by default in those Surf crates. |
| `gpu-metal` | MCP/NAPI/Python forward to `turbo-dom-raster/gpu-metal`. It compiles the optional wgpu canvas replay and recorded-WebGL backend, which requests a Metal adapter. It is off by default and needs a usable Metal host for actual GPU execution. Compiling it does not prove a GPU was used. |
| `coretext` | MCP/NAPI/Python forward to `turbo-dom-raster/coretext`. On macOS, this enables the raster's CoreText/CoreGraphics canvas text path; system framework dependencies are target-gated. It is not a V8 upgrade, general DOM layout switch or proof of WebGL execution. On other platforms the macOS implementation is absent. |
| `native-sdk-frame-read` | Render forwards to `turbo-dom-browser-env/native-sdk-frame-read`; MCP/NAPI/Python forward to render. It changes interceptor should-throw lookup for the revision-pinned native V8 SDK profile. It neither installs that SDK nor selects its version. |

The render crate also always requests `turbo-dom-browser-env/host-internals` and
`turbo-dom-layout/serde,svg-rasterize`; its binding test dependency additionally
requests `test-support`. These are dependency features, not new Surf CLI flags.
Layout's default bundled fonts/hyphenation features still apply. Surf does not
expose the layout crate's PDF or `svg` feature families as Surf feature switches.

## Transport and feature unification

Core can be built alone without its default impersonation backend:

```sh
cargo build --locked --release --manifest-path rust/Cargo.toml \
  -p turbo-surf-core --no-default-features
```

That core profile uses its reqwest/rustls path. Rustls dependencies remain in the
graph even when impersonation is enabled for other transport paths; the mere
presence of a rustls crate does not identify the selected main client.

**`--no-default-features` on MCP, NAPI or page is not a rustls-only build.** Their
manifests depend on core and/or page with dependency defaults enabled. Disabling
the selected package's defaults does not disable dependency defaults. Likewise,
building the whole workspace can enable core impersonation while building Python.
A standalone Python build reaches core through view/render, which explicitly
disable core defaults, and therefore uses the reqwest/rustls resource client.

For MCP/NAPI/Python, `--no-default-features` removes that package's default AVIF
and JPEG XL switches unless another selected package enables them. To change the
MCP/NAPI transport dependency graph itself requires a reviewed manifest/profile
change; there is no existing top-level off switch that guarantees rustls-only.

## Stock artifacts versus opt-in builds

| Artifact / command | Default compiled profile | What it does not establish |
| --- | --- | --- |
| npm platform packages, `release.yml` | MCP binary plus NAPI addon; impersonation, AVIF, JPEG XL; stock V8 150 family | No `gpu-metal`, `coretext`, native 154/155 SDK or real GPU receipt |
| `npm run build:addon` | Release NAPI build with its defaults | No optional feature union or test execution |
| Standalone PyPI wheel, `release-py.yml` | Python's AVIF/JPEG XL defaults, stock V8, reqwest/rustls resource transport; `pyo3/extension-module` from pyproject | No BoringSSL default, optional Metal/CoreText profile or guest WebGL bridge |
| Rust crates on crates.io | Source distributions with their feature declarations | No particular final consumer's feature union, SDK or hardware execution |

The current npm workflow runs this command once per target, without extra feature
arguments:

```sh
cargo build --locked --release -p turbo-surf-mcp -p turbo-surf-napi \
  --target TARGET --manifest-path rust/Cargo.toml
```

Its four targets are `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`. Intel macOS and Linux
musl are not current prebuilt targets. The main npm package carries the launcher,
shim and addon loader; matching optional platform packages carry the actual MCP
binary and `.node` file. `turbo-surf-napi` is not independently published as a Rust
crate or npm addon package.

Python wheels target macOS arm64, Linux x86_64/aarch64 with `manylinux_2_28`
(glibc >= 2.28), and Windows x64. PyO3's `abi3-py38` supports CPython >= 3.8;
that Python ABI guarantee does not lower the Linux C-library requirement. The
maturin configuration adds `pyo3/extension-module` for wheel construction; ordinary
workspace tests do not enable it by default. Python's `test` extra installs
pytest, not a Rust engine feature.

### GPU backend versus installed bridge

The raster feature and the embedder connection are different requirements.
MCP's `Session::new` and NAPI's `ensure_render_init` install the render crate's
WebGL execution/metadata callbacks only under their `gpu-metal` feature. The
render crate alone has callback APIs but no `gpu-metal` feature or automatic GPU
installation. Python exposes raster screenshot functions, but its current
`evaluate`/`render` initialization does not install the canvas/WebGL callbacks.
Forwarding Python's raster feature therefore does not establish a working guest
WebGL bridge.

The raster canvas PNG path can fall back to software if Metal replay fails;
WebGL readback can fail to supply a framebuffer. Feature selection, callback
installation, adapter acquisition and successful execution must be verified
separately. There is no WebGPU promise implied by `gpu-metal`; see the
[GPU bridge guide](GPU-BRIDGE.md) for the supported API and diagnostics.

## Source builds

Run these commands from the repository root. They are build recipes, not claims
that the optional profile passed its platform or parity tests:

```sh
# Stock executable and addon.
cargo build --locked --release --manifest-path rust/Cargo.toml \
  -p turbo-surf-mcp -p turbo-surf-napi

# Optional macOS Metal + CoreText profile, retaining defaults.
cargo build --locked --release --manifest-path rust/Cargo.toml \
  -p turbo-surf-mcp -p turbo-surf-napi \
  --features turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext,turbo-surf-napi/gpu-metal,turbo-surf-napi/coretext

# Python wheel; pyproject supplies the extension-module feature.
maturin build --release --manifest-path rust/crates/turbo-surf-py/Cargo.toml

# Python source install into an active virtual environment.
maturin develop --release --manifest-path rust/crates/turbo-surf-py/Cargo.toml
```

Rust is pinned to **1.98.0** in [rust-toolchain.toml](../rust-toolchain.toml);
release workflows select the same toolchain. Builds involving render compile
C++20 audio sources even without impersonation or GPU features. Stock V8 also
needs the matching native archive/build prerequisites. The impersonation stack
adds BoringSSL/bindgen tooling: release CI installs clang/libclang, CMake and NASM
and selects Go 1.26.4 for its native build. macOS source builds need the platform
SDK/framework tools; Windows artifacts use MSVC. AVIF's rav1d is built without
assembly, and JPEG XL uses jxl-oxide without its optional Rayon pool; this does
not make the entire render engine a pure-Rust/no-toolchain build.

`RUSTY_V8_ARCHIVE` selects a native archive for a source build; it is not a runtime
engine selector. For native 154/155 SDK experiments, use the pinned procedures in
[engine-154](../harness/detection/engine-154/README.md) and
[engine-155](../harness/detection/engine-155/README.md). Their build helpers verify
and arrange source overlays, SDK/archive/binding inputs and the corresponding
frame-read feature. Exporting `TURBO_SURF_ENGINE` alone or adding
`native-sdk-frame-read` to stock dependencies is not that procedure.

## Dependency identity and verification

On this v0.8.3 integration branch, [rust/Cargo.toml](../rust/Cargo.toml) patches
`turbo-dom`, `turbo-dom-browser-env`, `turbo-dom-layout` and `turbo-dom-raster`
to `ce3d0d0b0f9a2c7a08e8a9fcf899085682186fd6`. Manifest requirements are
`turbo-dom` 0.8 and the companion crates 0.8.2; the pin currently resolves all
four to version 0.8.3. The lockfile, not a README version
label, records the resolved sources. Stock V8 currently resolves
`turbo-dom-v8` 150.4.1 through the named Deno facade family. The source version
strings remain 0.8.2 until the separate release version bump.

Tag publishing rejects development Git patches. Follow
[Publishing](../PUBLISHING.md) to publish prerequisites, remove patches and verify
the actual registry graph before tagging. A source-tree test, installed wheel,
optional GPU build and native-SDK capture each prove different artifacts.

To inspect a build without changing its feature selection, use Cargo's feature
tree in that same build context:

```sh
cargo tree --locked --manifest-path rust/Cargo.toml -e features -p turbo-surf-napi
cargo tree --locked --manifest-path rust/Cargo.toml -e features -p turbo-surf-py
```

Keep the selected packages/features, lockfile/source identity, toolchain, target,
profile, environment/SDK inputs and artifact hash with validation results. Loading
an exact addon with `TURBO_SURF_NAPI_PATH` selects that file; it does not rebuild
it or validate its feature graph. Release tests and feature-specific controls are
listed in [Quality gates](QUALITY-GATES.md) and the
[Chrome parity runbook](../harness/detection/CHROME-PARITY-RUNBOOK.md).
