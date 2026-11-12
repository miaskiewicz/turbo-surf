# GPU bridge and render hooks

This describes the v0.8.3 source at `7fd911b`, with turbo-dom pinned to
`ce3d0d0b0f9a2c7a08e8a9fcf899085682186fd6`. Build features, installed callbacks
and successful hardware execution are separate checks.

## Features and entry points

| Surface | Current implementation |
| --- | --- |
| `gpu-metal` | Optional `turbo-dom-raster` wgpu/Metal backend. MCP, NAPI and Python forward the feature; it is off by default. |
| `coretext` | Optional macOS CoreText/CoreGraphics canvas-text rasterization. Off by default; inert on non-macOS targets. Eligible software canvas text uses it; unsupported transforms/web-font paths retain vector rendering. |
| `native-sdk-frame-read` | Compatibility feature forwarded by render/MCP/NAPI/Python to browser-env for the experimental engine-154/155 SDK. It does not enable GPU rendering. |
| MCP | `Session::new()` installs process-global measurement, PNG and raw-RGBA hooks, plus WebGL executor/metadata when `gpu-metal` is compiled. `initialize_render_hooks()` installs the same callbacks without session/solver/`.env` initialization. |
| NAPI | Render/live entry points call `ensure_render_init()`: measurement and PNG hooks, plus optional WebGL executor/metadata. This initializer does not install MCP's separate raw-RGBA callback. |
| Python | The feature is forwarded to raster, but the Python entry points do not install the guest render/WebGL callbacks. Compiling that feature alone does not activate guest WebGL replay. |
| Direct `turbo-surf-render` embedder | The host installs the public `set_measure_fn`, `set_raster_fn`, `set_raster_rgba_fn`, `set_webgl_fn` and `set_webgl_metadata_fn` callbacks it needs. The render crate does not select a hardware backend itself. |

The callbacks live in process-global locks; MCP/NAPI install their defaults once.
They must be available before the page's graphics bootstrap creates its wrappers.
Adapter/device creation occurs when metadata or replay is requested. Raster caches
both successful initialization and initialization failure for the process lifetime.
A feature boolean or the existence of `readPixels` proves neither adapter creation
nor a successful draw.

## Enable and verify

On a macOS host with an accessible Metal adapter and the normal Rust/native build
toolchain, a stock-SDK build can opt in explicitly:

```sh
cargo build --manifest-path rust/Cargo.toml --release \
  -p turbo-surf-mcp -p turbo-surf-napi \
  --features turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext,turbo-surf-napi/gpu-metal,turbo-surf-napi/coretext
```

Use an isolated/coordinated Cargo target and freeze the resulting executable/addon
before comparisons. There is no runtime `--gpu` switch. The npm platform release
workflow builds MCP/NAPI with their default features, without these two opt-ins;
GPU availability on the host does not change those compiled artifacts. See the
[publishing instructions](../PUBLISHING.md).

The [Chrome 155 runbook](../harness/detection/CHROME-PARITY-RUNBOOK.md) adds an
independent, verified source-built V8 SDK. Its `detection_trace --build-info`
reports compiled `gpuMetal`/`coretext` booleans and the actual V8 version. GPU
support also works with the stock SDK; selecting the experimental SDK alone does
not install or enable GPU hooks.

Verify real content-dependent WebGL pixels and adapter metadata with the shared
GPU controls, plus canvas text/pixel probes for the selected text path. Record
artifact hash, features, OS/GPU and actual engine/browser versions. The adapter is
requested through the Metal backend with no forced fallback adapter. Software or
synthetic fallback is not evidence of successful GPU execution.

`TURBO_SURF_WEBGL_DEBUG` being present enables backend error/translation logging;
it does not install callbacks or enable a Cargo feature. Unset it for an ordinary
run rather than setting it to `0`.

## Replay and current limits

Surf records a bounded WebGL call stream, including shader source, vertex/index
buffer bytes, uniforms and draws. Raster translates supported GLSL through naga
to WGSL, executes offscreen through wgpu/Metal and returns top-left RGBA8. Surf
converts the requested rectangle to WebGL's bottom-left `readPixels` coordinates.
This is an implemented GPU bridge, not a full browser GPU process or WebGPU
implementation. Recorder compile/link queries are success-shaped compatibility
methods, not complete shader validation.

Unsupported shaders, sampler/texture uniforms and other unsupported operations or
GPU failures can return no hardware result; the wrapper then falls back to the
binding's original synthetic readback. Canvas PNG replay tries Metal when enabled
and falls back to tiny-skia; raw RGBA canvas replay remains the software path.
CoreText does not turn every PNG, screenshot or WebGL draw into a CoreText render.
A passing measured scene does not establish general WebGL/WebGL2 or Chrome pixel
parity.

The native DOM layout/paint screenshot tier and source-specific geometry
admission are separate from this bridge. CSS/Web Animations run in canonical
browser-env, but native layout does not generally sample animated geometry.
Proven independent local-box/isolated-extent reads are admitted under the native
overlay-scrollbar/zero-offset policy. Affected animated targets/ancestors,
animated descendants of an extent query, layout-changing/unknown effects and
strict hit-test/scroll paths can still reject explicitly. GPU opt-in does not
remove those checks. A general animation-disable option is tracked in
[#500](https://github.com/miaskiewicz/turbo-surf/issues/500); reduced-motion media
emulation is not that option.

## Source references

- Surf [hook installation](../rust/crates/turbo-surf-mcp/src/lib.rs),
  [NAPI initialization](../rust/crates/turbo-surf-napi/src/lib.rs),
  [Python entry points](../rust/crates/turbo-surf-py/src/lib.rs), and
  [recording/readback callbacks](../rust/crates/turbo-surf-render/src/runtime.rs).
- Pinned raster [features](https://github.com/miaskiewicz/turbo-dom/blob/ce3d0d0b0f9a2c7a08e8a9fcf899085682186fd6/crates/turbo-dom-raster/Cargo.toml),
  [canvas backend selection](https://github.com/miaskiewicz/turbo-dom/blob/ce3d0d0b0f9a2c7a08e8a9fcf899085682186fd6/crates/turbo-dom-raster/src/lib.rs), and
  [WebGL replay/adapter lifecycle](https://github.com/miaskiewicz/turbo-dom/blob/ce3d0d0b0f9a2c7a08e8a9fcf899085682186fd6/crates/turbo-dom-raster/src/webgl.rs).
- [Host-hook controls](../rust/crates/turbo-surf-render/tests/host_render_hooks.rs)
  verify callback wiring; hardware controls must independently verify the real adapter.
