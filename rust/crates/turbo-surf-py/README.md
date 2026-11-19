# turbo-surf (Python)

Browserless, native-speed web crawler + extractor with a real V8 JS-render tier —
a PyO3 binding over the [turbo-surf](https://github.com/miaskiewicz/turbo-surf)
Rust engine. Fetch-free: you pass a page's HTML in, and get a view out (Markdown,
visible text, links, a typed extraction, an accessibility tree). For JS-gated
pages, the engine runs the page's **own scripts** in a true V8 isolate over a
native DOM — no headless Chromium.

```python
import turbo_surf as ts

html = open("page.html").read()

ts.markdown(html, base_url="https://example.com/")   # -> Markdown str
ts.text(html)                                        # -> visible text
ts.links(html, base_url="https://example.com/")      # -> list[str]

# Typed extraction: a JSON schema maps field names to selector specs.
schema = '{"title": {"selector": "h1"}, "prices": {"selector": ".price", "list": true}}'
ts.extract(html, schema, base_url="https://example.com/")   # -> JSON str

# JS-gated page: run its own scripts, read the hydrated DOM.
hydrated = ts.render(html, script, base_url="https://example.com/")
```

Fatal faults (malformed schema JSON, a render-tier failure) raise
`turbo_surf.TurboSurfError`; the non-JS views never raise.

## Install

```
pip install turbo-surf
```

Prebuilt abi3 wheels (CPython 3.8+) are published for Linux (x86_64/aarch64),
macOS (arm64), and Windows (x64). Linux wheels require glibc 2.28 or later
(`manylinux_2_28`). The CPython 3.8+ abi3 contract is unchanged; it does not imply
compatibility with older Linux C libraries.

## Source builds and feature profile

Use Rust 1.98.0 and maturin (the build backend requires `>=1.9.3,<2.0`; release CI
uses 1.15.0). From the repository root:

```sh
maturin build --release --manifest-path rust/crates/turbo-surf-py/Cargo.toml
# Or install into an active virtual environment:
maturin develop --release --manifest-path rust/crates/turbo-surf-py/Cargo.toml
```

The crate defaults to `avif` and `jxl` screenshot pixel decoders. Its independent
resource client uses reqwest/rustls: view/render disable core's default
impersonation feature. A workspace build that also selects MCP/NAPI can unify
BoringSSL back in, so validate a standalone wheel separately. Native V8 and
C++20 audio still need their build toolchains; this is not a pure-Python wheel.

`pyproject.toml` enables `pyo3/extension-module` for wheel construction, while the
crate selects `abi3-py38`. Keep the extension-module feature out of ordinary
workspace test/link assumptions. The Python `test` extra installs pytest; it does
not change the engine's Cargo features.

Optional `gpu-metal`, `coretext` and `native-sdk-frame-read` forward to the
raster/render crates. They are off in the stock wheel workflow. Python exposes
synthetic raster screenshot functions, but its current `evaluate`/`render`
initialization does not install guest canvas or WebGL execution callbacks.
Enabling a raster feature alone therefore does not establish a guest GPU bridge.
See [Build features](../../../docs/BUILD-FEATURES.md) for all defaults,
forwarding, platform/toolchain requirements and [GPU bridge](../../../docs/GPU-BRIDGE.md)
for the execution boundary.

## API

| function | returns | notes |
| --- | --- | --- |
| `markdown(html, base_url="")` | `str` | Markdown render |
| `text(html)` | `str` | visible text |
| `title(html)` | `str` | document `<title>` |
| `html(html)` | `str` | re-serialized HTML |
| `links(html, base_url="")` | `list[str]` | resolved hyperlink targets |
| `interactive_elements(html, base_url="")` | JSON `str` | links/buttons/inputs |
| `accessibility_tree(html)` | JSON `str` | a11y tree |
| `hydration_state(html)` | JSON `str` | hydration probe |
| `detect(html)` | JSON `str` | is the page JS-gated? |
| `query(html, selector, kind=None)` | JSON `str` | `kind` = `"css"`/`"xpath"`/auto |
| `extract(html, schema_json, base_url="")` | JSON `str` | typed extraction |
| `evaluate(html, script)` | `str` | run `script` over the DOM (sync) |
| `render(html, script, base_url="")` | `str` | hydrated HTML after page scripts run |
| `transform(src, ts=False, jsx=False)` | `str` | TS/JSX → classic JS (swc) |

MIT licensed.
