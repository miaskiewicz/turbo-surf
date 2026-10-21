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
