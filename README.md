# turbo-surf

> Native-speed, **browserless** web crawler + **MCP server** for AI agents — built
> on [turbo-dom](https://github.com/miaskiewicz/turbo-dom). Fetch + parse + extract
> + run page JS with no headless browser; 100×+ faster on server-rendered pages.

turbo-surf is a single native (Rust) engine — no Chromium, no headless browser
(it can still render a reasonably-representative **synthetic screenshot** on
demand — see below):

- **A crawler** — point it at a domain and stream page records: indexed interactive
  elements, a link/form graph, an accessibility tree, markdown and plain-text views,
  rendered-HTML capture, CSS/XPath node queries, and schema-driven structured
  extraction.
- **An agent tool** — a 80-tool **MCP** server agents drive directly over stdio
  (`crawl`, `batch`, navigate, click/fill/submit, query, extract, accessibility
  tree, markdown, `render`/`eval_js`/`inject_js`, `screenshot`, cookies/headers,
  `snapshot`).

For pages that need JavaScript it runs their scripts — either by mining
server-embedded hydration state or by executing page JS inside a **true V8 isolate**
(no browser) and re-rendering the DOM. Its page API is Playwright-shaped: the
benchmark suite drives the engine with unmodified Playwright scripts.

## What makes it different

Most tools in this space pick one lane: a crawler **or** a browser-automation
library, and they get their DOM from a real browser (Playwright/Puppeteer/
Selenium) or an in-process fake DOM with no security isolation (jsdom,
happy-dom). turbo-surf is unusual on four axes at once:

1. **AI-agent-ready out of the box.** It ships a full **MCP server** (80 tools:
   navigate, click/fill/submit, query, extract, accessibility tree, markdown,
   `crawl` a whole site, `batch` a URL list, `render`/`set_mode` to run page JS,
   `eval_js`/`inject_js` against the live render heap with a DOM-history trail,
   `screenshot` (PNG/SVG of the page or any DOM-history snapshot) + `set_viewport`,
   cookies/headers, `snapshot`, …) so an agent drives real pages over stdio with
   **no browser and no glue code** — `npx turbo-surf-mcp`. Most crawlers are
   libraries you wrap yourself; this one is an agent tool on day one.
2. **Crawler + agent surface on one native engine.** The same engine bulk-crawls a
   domain and serves the MCP tools — no browser anywhere in the stack. Its page API
   is Playwright-shaped (the benchmark harness runs unmodified Playwright routines
   on it).
3. **Its own DOM, not a browser's.** turbo-dom is a native + WASM HTML parser
   with a lazy copy-on-write DOM — native-speed parse, no browser pixels/layout/IPC
   on the hot path (a separate on-demand raster tier synthesizes screenshots).
4. **A V8 isolate to run page JS + re-render.** Page (or your own) JavaScript runs
   inside a real V8 isolate (a `deno_core` runtime — host heap unreachable from the
   guest, with a runaway-execution budget) against the native rtdom DOM, then
   re-renders. Most JS-capable crawlers instead drive a full headless browser, or
   run page scripts in-process with a fake DOM that offers **no real security
   isolation** (Node's `vm` is [explicitly not a security
   boundary](https://nodejs.org/api/vm.html); cf. happy-dom
   [CVE-2025-61927](https://github.com/capricorn86/happy-dom/wiki/JavaScript-Evaluation-Warning)).
   Running hostile page JS in a true isolate against a lightweight DOM is rare.

See [CHANGELOG.md](./CHANGELOG.md) for what shipped and
[rust/README.md](./rust/README.md) for the engine internals.

## Built on turbo-dom

Since 0.6.0 turbo-surf keeps no copies of shared engine code: it links the
[turbo-dom](https://github.com/miaskiewicz/turbo-dom) crates that turbo-test and
turbo-html2pdf use too —

- **`turbo-dom`** — HTML parser, copy-on-write DOM, CSSOM and the one selector engine;
- **`turbo-dom-layout`** — style cascade and layout behind live geometry
  (`getBoundingClientRect`, scroll, ranges) and the screenshot tier;
- **`turbo-dom-raster`** — the synthetic-screenshot painter (PNG/SVG, canvas 2D, WebGL
  recorder), formerly `turbo-surf-raster`;
- **`turbo-dom-browser-env`** — the browser-environment binding JS that the render tier
  installs into its V8 isolate (formerly copied in by a vendoring script);
- **`turbo-dom-v8`** — the rusty_v8 fork under the render tier's V8 (through
  `turbo-surf-deno-v8`).

The surf workspace (`rust/`) has eight crates: `turbo-surf-core` (network, cookies,
crawl, identity), `-view` (extraction views), `-page` (the Playwright-shaped page API),
`-render` (the `deno_core` V8 render tier), `-mcp` (the MCP server binary),
`-transform`, and the `-napi` / `-py` bindings.

## Status

Status: **v0.6.0 — the turbo-dom consolidation release**
([npm](https://www.npmjs.com/package/turbo-surf)). The engine ships hardened
networking (cookies / `document.cookie` bridge / robots + crawl-delay / charset /
size + redirect caps, HTTP/2 + a pooled client, 304 conditional cache), crawl
orchestration (global + per-host concurrency, token-bucket politeness, backoff/retry,
canonical dedupe, depth/page caps), structured extraction, CSS+XPath query, a
no-Chromium JS render tier (a true V8 isolate over the native DOM) with re-enterable
live-heap `eval_js`/`inject_js` + a DOM-history trail, **synthetic screenshots**
(PNG/SVG from a native layout+paint of any HTML snapshot — still no browser), and an
80-tool MCP server (native binary). Chrome parity is measured, not complete — see
[Chrome parity (0.6.0)](#chrome-parity-060). Benchmarks are pending a re-run
([below](#benchmarks--pending-re-run)).

## Install

The npm package is a thin launcher that spawns the native binary:

```sh
npm install -g turbo-surf   # provides the `turbo-surf-mcp` command
# …or run without installing:
npx -y turbo-surf
```

Node ≥ 20 to launch; the engine is a prebuilt per-platform native binary (no Node
runtime hosts it).

### What ships where

turbo-surf publishes to **three registries** (see [PUBLISHING.md](./PUBLISHING.md)):
a **`v*`** git tag ships the npm launcher + the Rust crates; a separate **`pyv*`**
tag ships the PyPI wheels.

| Artifact | Registry | What it is | For |
|---|---|---|---|
| [`turbo-surf`](https://www.npmjs.com/package/turbo-surf) | **npm** | A thin launcher (`cli.js`/`index.js`) **+ prebuilt `turbo-surf-mcp` binaries** for each platform in `bin/` (darwin arm64, linux x64/arm64-gnu, win x64; Intel macOS builds from source). `npx` resolves the right one and spawns it. | Running the **MCP server** / CLI. No Rust toolchain, no Chromium, no Node hosting Rust. |
| [`turbo-surf`](https://pypi.org/project/turbo-surf/) | **PyPI** | A PyO3 binding, shipped as prebuilt abi3 wheels (CPython 3.8+) for Linux (x86_64/aarch64), macOS (arm64), Windows (x64). | Calling the engine **from Python** — `markdown`/`text`/`links`/`extract`/`render`/…. No Rust toolchain, no Chromium. |
| `turbo-surf-core`, `-view`, `-page`, `-render`, `-mcp` | **crates.io** | The Rust crates, in dependency order (the screenshot tier is turbo-dom's `turbo-dom-raster`). | **Embedding** the engine in a Rust program. |

The `turbo-surf-napi` cdylib and `turbo-surf-transform` crate are **not**
published — napi is for the dev harness + the Playwright shim only. The Playwright
shim (`rust/playwright-shim/`) is a dev/in-repo tool, not an npm artifact.

## MCP server (agents)

```sh
npx turbo-surf-mcp          # stdio MCP server (80 tools), e.g.:
# navigation:  goto, go_back, go_forward, reload, set_user_agent
# content:     interactive_elements, accessibility_tree, aria_snapshot, markdown,
#              text, html, links, requests, snapshot, query, get_by,
#              hydration_state, extract
# screenshot:  screenshot (PNG/SVG of the page or a dom_history snapshot),
#              set_viewport
# interaction: click, fill, submit, click_selector, fill_selector, select_option,
#              check, uncheck, fill_many, find_text, extract_links,
#              set_bypass_consent, set_consent_socs
# accessors:   get_attribute, text_content, inner_html, input_value, count,
#              is_visible, is_checked, is_enabled, is_editable, is_focused,
#              is_empty, aria_role, accessible_name, accessible_description
# bulk:        crawl, batch
# search:      web_search, web_search_set_engine, web_search_setup_browser,
#              web_search_strategies, web_search_load_strategy, web_search_reset_strategy
# fetch:       fetch_markdown, fetch_markdown_batch, fetch_json, fetch_raw
# render/JS:   render, set_mode, eval_js, inject_js, latest_dom, dom_history,
#              evaluate, detect, detect_js, run_playwright, probe, human_interact
# stealth:     stealth_status, set_fingerprint, analyze_akamai, probe_mint
# session:     get_cookies, set_cookie, set_extra_headers, robots_check
```

The server speaks newline-delimited JSON-RPC 2.0 (MCP `2024-11-05`, `ping`
included). A tool that runs and fails answers a normal result with `isError: true`
and the message as its text; protocol failures use the JSON-RPC codes (`-32700`
unparseable line, with `id: null`; `-32600` not a request; `-32601` unknown
method; `-32602` missing `name`, non-object `arguments` or unknown tool).
`screenshot` refuses surfaces over 16384x16384 px, and `screenshot`/`set_viewport`
refuse a dimension over Chrome's 10,000,000 px bound.

`render`/`set_mode` switch the Page into the JS render tier (a true V8 isolate over
the native DOM); then `eval_js` and `inject_js` run against the **live render heap**
(page globals, handlers) and each mutation appends to a DOM-history trail readable
via `latest_dom`/`dom_history`.

`run_playwright` executes a **Playwright-style script** directly — `page` /
`locator` / `getBy*` / `expect` (with `.not`) / `test()` blocks — over the engine,
no browser. Args: `{ script, url?, testIdAttribute? }` (a `url` navigates first,
honoring `set_mode` so an SPA hydrates). It returns `{ ok, ran, logs }` or
`{ ok:false, error, logs }` — a failed assertion is a result, not a tool error:

```jsonc
// tools/call run_playwright
{ "url": "https://example.com", "testIdAttribute": "data-test-id",
  "script": "await expect(page.locator('h1')).toHaveText('Example Domain');\nawait page.fill('#q','rust');\nawait expect(page.locator('#q')).toHaveValue('rust');" }
```

### Set it up in Claude Code (step by step)

Never set up an MCP server before? This is the whole thing — three commands.

**1. Check the prerequisites.** You need [Node ≥ 20](https://nodejs.org)
(`node -v`) and the [Claude Code CLI](https://docs.claude.com/en/docs/claude-code)
(`claude --version`). That's it — **no Rust, no Chromium, no build step**. `npx`
downloads a small launcher plus the prebuilt native binary for your OS the first
time you run it.

**2. Register the server.** Run this once (the `--` separates Claude's flags from
the command Claude will spawn):

```sh
claude mcp add turbo-surf -- npx -y turbo-surf
```

That writes the server into your Claude Code config. `npx -y turbo-surf`
resolves + spawns the native binary over stdio — one process, no Node hosting it,
no browser.

**3. Verify it's connected.** List your servers (look for `turbo-surf` →
`✓ connected`):

```sh
claude mcp list
```

Now start (or restart) Claude Code and ask it to, e.g., *"use turbo-surf to fetch
the markdown of example.com"* — the 80 tools above are available. Remove it later
with `claude mcp remove turbo-surf`.

**Scope (where the server is registered).** By default it's registered for your
user. To share it with a repo instead, add `--scope project` — that writes a
`.mcp.json` you can commit:

```jsonc
// .mcp.json — committed; teammates get the server automatically
{
  "mcpServers": {
    "turbo-surf": { "command": "npx", "args": ["-y", "turbo-surf"] }
  }
}
```

**Running from a checkout** (contributors) — point at a local build instead of npm:

```sh
cargo build --release -p turbo-surf-mcp --manifest-path rust/Cargo.toml
claude mcp add turbo-surf -- "$PWD/rust/target/release/turbo-surf-mcp"
```

**Troubleshooting.** `command not found: claude` → install the Claude Code CLI.
`npx` hangs the first run → it's downloading the binary; let it finish (or pre-warm
with `npx -y turbo-surf` in a terminal, then Ctrl-C). Shows `✗ failed` in
`claude mcp list` → run the command directly to see the error; on an unsupported
platform there's no prebuilt binary (build from a checkout as above).

**Other MCP clients** (Claude Desktop, Cursor, …) — point their MCP config's
`command` at `npx` with args `["-y", "turbo-surf"]`, or at the binary path.

## Looking like a real browser (fingerprint + anti-bot)

The default profile reports **Chrome 156** through its UA and client hints
(`sec-ch-ua`, `sec-fetch-*`, …), with a configured per-version header order
and a corresponding `navigator` (`platform`, `vendor`,
`webdriver: false`, plugins, `window.chrome`, native-`toString`, coherent
`userAgentData` brands + `fullVersionList`) inside the JS render tier. This is an
emulated browser surface; successful identity probes do not establish complete
Chrome parity or acceptance by a site's anti-bot system.

Version-specific fingerprint data (reported major, full build for high-entropy
UA-CH, the navigation **header order** and the measured per-request-class orders,
`Accept`/`Priority` per destination — all of which Chrome changes across releases)
lives in a data registry (`turbo-surf-core/src/browser_releases.json`, compiled into
the `browser` module): one coherent, measured config per supported Chrome version,
defaulting to Chrome 156.0.8078.47. Wire headers and the render-tier navigator read
from it. A new release is measured and written by one command —
[`harness/detection/reversion-chrome.mjs`](harness/detection/CHROME-REVERSION.md)
runs the Chrome identity battery (TLS/H2/headers/client hints/JS identity) and
diffs Chrome against turbo-surf; a reported version change does not upgrade V8
(the JS-engine gap stays recorded).

### Quick start (anti-bot)

```bash
# 1. (optional) real TLS/JA3 + HTTP-2 fingerprint — needs cmake + nasm + libclang
cargo build --release -p turbo-surf-mcp --features impersonate

# 2. (optional) enable a challenge solver — copy the template and fill ONE
cp .env.example .env
#    then edit .env:  TURBO_SURF_SOLVER=akamai      # in-house, no key
#                or:  TURBO_SURF_SOLVER=cloudflare  # in-house, no key
#                or:  TURBO_SURF_SOLVER=hyper        + HYPER_API_KEY=...
#                or:  TURBO_SURF_SOLVER=browser      + TURBO_SURF_BROWSER_CMD=...

# 3. run — the MCP server / crawler auto-detects walls and solves them
npx turbo-surf-mcp
```

That's it. With no `.env` you get the default Chrome identity and transport profile;
set one `TURBO_SURF_SOLVER` to also clear challenge walls.

### Configuration reference

All optional. Set in `.env` (auto-loaded) or the process env.

| Variable | Values | What it does |
|---|---|---|
| `TURBO_SURF_SOLVER` | `cloudflare` · `awswaf` · `akamai` · `recaptcha` · `hyper` · `scrapfly` · `browser` | Pick the challenge solver. Unset = no solving (fingerprint only). `cloudflare`/`awswaf`/`akamai` are in-house (no key). |
| `HYPER_API_KEY` | key | Hyper Solutions token API (Akamai/DataDome/Kasada). Used when solver=`hyper`. |
| `SCRAPFLY_API_KEY` | key | Scrapfly ASP API (Cloudflare + fallback). Used when solver=`scrapfly`. |
| `TURBO_SURF_BROWSER_CMD` | shell command | The hardened-headless sidecar to run for solver=`browser` (e.g. `node harness/browser-solver/solve.mjs`). When solver=`akamai`, also enables the browser fallback (see below). |
| `TURBO_SURF_PROXY` | `http://user:pass@host:port` | Egress proxy. Optional egress proxy. Where a token is IP-bound, minting and replay must use the same egress. |
| `TURBO_SURF_SENSOR_DIR` | path (default `akamai-sensors`) | Where `analyze_akamai`'s `retry` saves a working sensor (keyed by script hash + version). |

**Solver maturity** — `cloudflare` (managed/Iuam) and `awswaf` (common + targeted
`challenge.js`) both run the challenge's **own JS in the V8 tier** (no browser) and
are the most viable in-house solves. `akamai` is **experimental** (the live sensor
crypto isn't reversed per-version yet). **All in-house solvers try themselves first,
then fall back to the browser sidecar** when `TURBO_SURF_BROWSER_CMD` is set — so a
failed self-solve still clears the wall. `hyper`/`scrapfly`/`browser` are the robust
paths. CAPTCHA tiers (AWS `captcha`, CF Turnstile-interactive) aren't self-solvable
and route straight to the sidecar.

Build feature (not env): **`impersonate`** is enabled by default for the MCP binary and swaps rustls → BoringSSL
(`wreq`) for a real Chrome TLS/JA3/JA4 + HTTP-2 fingerprint. Needs `cmake`, `nasm` and `libclang`.
The Chrome 154 transport sends real `trust_anchors` IDs by default, backed by the
same Chromium root store used for certificate verification. `trust-anchors` is a
compatibility feature alias. The reviewed named registry distributions preserve
the pinned transport APIs and require Rust 1.98. Actual package and registry
consumer checks are documented in [`PUBLISHING.md`](./PUBLISHING.md).

Build feature (not env): **`--features gpu-metal`** (macOS/Metal, off by default) runs the
render tier's **WebGL on the real Apple GPU** — a live WebGL→wgpu→Metal bridge records the
page's `gl.*` calls and executes them on the GPU, so `readPixels` returns genuine framebuffer
pixels. Measured: a standard WebGL fingerprint draw is now **byte-identical to real Chrome**
(same per-pixel RGBA and the same pixel hash — wgpu→Metal lands on the same result as Chrome's
ANGLE→Metal on the same GPU). Canvas 2D `toDataURL` also rasterizes on the GPU (falls back to
tiny-skia on any GPU error, so enabling it never regresses). Zero cost to the default build +
the PyPI wheels (no `wgpu` unless enabled). Beats device-invariance/SwiftShader detection —
the shared WebGL scene matched an exact hash. This does not establish arbitrary scene parity.

A second build feature **`--features coretext`** (macOS only, off by default) rasterizes canvas
**text** through CoreText/CoreGraphics — the same system glyph stack Chrome's Skia uses on macOS —
so `getImageData` over rendered text reads back like a real Mac Chrome (measured: `getImageData` on
"Hello" 20px Arial matches Chrome's bounding box + average alpha exactly). Double OS-guarded
(`cfg(macos)` + macOS-only Cargo deps); only text is swapped (shapes stay on tiny-skia), and it
falls back to the vector glyph trace for a scaled/rotated text matrix. (Canvas-2D `toDataURL` still
differs from Chrome's *hash*: that also needs Chrome's exact PNG-container/encoder, no shared final
layer — but `getImageData` reads back the real rendered pixels, byte-identical for solids/shapes and
now CoreText-matched for text.)

MCP tools for stealth: **`set_fingerprint`** (override navigator fields),
**`stealth_status`** (inspect active profile/solver/overrides), **`probe`** (see
what a page's anti-bot JS reads), **`probe_mint`** (browserless recon: run a page's
integrity JS in-isolate → the env surface it demands + any cookies earned),
**`human_interact`** (play scripted input — curved mouse
motion, real mousedown→dwell→mouseup→click, human typing, hover + focus/blur — as a
generic `move`/`click`/`focus`/`type` step sequence or a saved `routine`), and
**`analyze_akamai`** (experimental: rebuild + test Akamai sensors). Detailed below.

**Human interaction (`human_interact`).** This MCP tool plays a `steps` array or
saved `routine` through page JavaScript. The `google-serp` routine moves to the
search box, clicks, focuses, types `{query}` and presses Enter. Those script-dispatched
events are untrusted. The differential harness uses a separate private native
host-input capability to create trusted events, bound by a cancellable execution
budget; guest scripts cannot access that capability. Neither path establishes
server acceptance. Fresh native-cookie Google controls complete genuine consent
and form navigation, but reliable native SERP acceptance remains unresolved. See
the [latest sanitized capture](./harness/detection/GOOGLE-LAST-RUN.json),
[reusable differential harness](./harness/detection/README.md),
[exact Chrome155 setup and flags](./harness/detection/CHROME-PARITY-RUNBOOK.md) and
[dated observations](./harness/detection/FINDINGS.md).

**Render-tier realism.** The isolate is shaped to match real Chrome structurally, verified by
diffing a shared collector against real headed Chrome (the reusable probes under
[`scripts/browser-sidecar/probes/`](./scripts/browser-sidecar/), run in Chrome via `run-probe.mjs`
and in-isolate via the `fp_snapshot` example). Highlights:

- **`navigator` is a real `Navigator` instance** — zero own properties, all ~84 members on
  `Navigator.prototype` (native-marked getters/methods), `instanceof Navigator`, `webdriver`
  a prototype getter (not an own prop).
- **Window surface breadth** includes many Chrome interface constructors, `on*`
  slots and API objects. Exact property counts differ by document/version, and
  presence does not establish implementation parity.
- **Namespaced helpers.** Most render helpers live on `globalThis[Symbol.for('ts')]`,
  reducing named globals. Symbol properties remain discoverable and some helpers
  are transiently exposed during evaluation/interaction; this is not invisible storage.
- **Native-fn shape**: masked shims report `[native code]` **and** have no own `prototype` (like
  real native functions); the `toString` anti-hook trap is itself shapeless.
- **Canvas 2D**: `getImageData` returns the real rendered pixels (byte-identical to Chrome for
  selected solids/shapes), `measureText` uses the system-font measurer, context
  class-tags as `CanvasRenderingContext2D` + `getContextAttributes`.
- **WebGL**: full enum-constant set on the context prototype; under `--features gpu-metal`,
  `readPixels` matched Chrome for the shared gradient scene when Metal was accessible.
  Unsupported draws/device failures can fall back to synthetic readback.
- Real **page-load lifecycle** (`readyState` loading→interactive→complete with `DOMContentLoaded`
  → window `load` → `pageshow`, in order), a Chrome-shaped **coherent high-resolution clock**
  (fractional `performance.now`/`timeOrigin`, batched rAF) with spread `performance.timing`. Observable
  time is **real**: `performance.now()`/`Date.now()` derive from real monotonic hrtime + virtual timer
  advance, so `setTimeout(fn, 100)` observes ~100ms elapsed (was 0ms — a chronometric-trap tell) and
  `timeOrigin + performance.now() == Date.now()` holds (verified by the shared clock probe, not a Google clearance result);
  timer *delays* default to virtual scheduling for fast crawling. The detection
  example selects real-time Window/frame timers before hydration; Rust consumers
  can opt in with `PageSessionOptions`. Plus **distinct**
  `Intersection`/`Resize`/`MutationObserver` constructors, real `MessagePort`/`MessageChannel`/
  `MessageEvent` instances, spec-shaped `navigator.permissions`, and general **nested-iframe realms**
  (loaded iframe/`srcdoc` scripts have their own V8 contexts and ECMAScript intrinsics,
  share the isolate and session cookies, and have a depth cap. DOM API wrappers and
  cross-origin enforcement still have limits).

Search strategies also take optional **`headers`** + **`internal_hosts`** maps (e.g. `Referer` +
`sec-fetch-site` for a real in-site-navigation shape). Opt into a coherent timezone with
**`TURBO_SURF_TZ`**. The synthetic google/youtube consent (`SOCS`) cookie is param-driven —
**`set_consent_socs`** overrides the value, and `""` seeds none so the *real*
`consent.google.com/save` Accept-all handshake runs instead of the shortcut (measured: both paths
still yielded the JS-gated shell in those historical runs). The probes cover selected
interfaces; they do not establish complete Chrome parity. Current fixes parse stylesheet
rules, expose parsed hidden-control values, allocate independent frame intrinsics and
execute classic scripts on the host pump with document source locations. Session-scoped
Accept-CH and resource metadata now distinguish navigation, scripts and fetches. GPU
renderer identity comes from the real adapter. See the [dated comparison](./harness/detection/FINDINGS.md)
for acceptance results and remaining limits.
A shell must be executed before classifying the final outcome; `/sorry` alone does not
prove IP reputation is the cause.

### Chrome parity (0.6.0)

Remeasured on 2026-10-29 (turbo-surf 166d2d6, turbo-dom 0189e4d) against frozen headless
Chrome 156.0.8078.47 / V8 15.6.75.19, localhost only, with the stock V8 15.0 engine and all
four features (`impersonate`, `trust-anchors`, `gpu-metal`, `coretext`):

| measure | before | 0.6.0 |
|---|---|---|
| API inventory, same 75 interfaces: missing own members | 142 (Chrome 155, 2026-10-21); 52 (0.5.5) | **2** (`HTMLCanvasElement` `captureStream`, `transferControlToOffscreen`) |
| API inventory difference paths | 374; 264 | **4** (370 resolved, none introduced) |
| Render parity, surf corpora (383 outputs) | — | 52 changed, all declared (turbo-dom#142, #176); 0 undeclared |

These are descriptor/ownership measurements, not a claim of full Chrome parity. The
comparison crosses an oracle change (Chrome 155 → 156) and an engine change.
Google SERP acceptance for 0.6.0 is a separate run: see GOOGLE-SERP-0.6.0 (pending,
[turbo-surf#114](https://github.com/miaskiewicz/turbo-surf/issues/114)).

Known gaps, each tracked in its own issue:
[unhandledrejection](https://github.com/miaskiewicz/turbo-surf/issues/103),
[workers](https://github.com/miaskiewicz/turbo-surf/issues/104),
[container queries in live layout](https://github.com/miaskiewicz/turbo-surf/issues/105),
[canvas captureStream / offscreen / PiP, WebGL](https://github.com/miaskiewicz/turbo-surf/issues/106),
[IndexedDB](https://github.com/miaskiewicz/turbo-surf/issues/107),
[placeholder APIs (caches, cookieStore, Navigation, locks, WebSocket)](https://github.com/miaskiewicz/turbo-surf/issues/108),
[fetch/stream residuals](https://github.com/miaskiewicz/turbo-surf/issues/109),
[CSP directives / reporting](https://github.com/miaskiewicz/turbo-surf/issues/110),
[child-frame form navigation](https://github.com/miaskiewicz/turbo-surf/issues/111),
[V8 15.0 vs 15.6](https://github.com/miaskiewicz/turbo-surf/issues/112),
[transport identity](https://github.com/miaskiewicz/turbo-surf/issues/113),
[Google SERP](https://github.com/miaskiewicz/turbo-surf/issues/114),
[GC / memory reclamation](https://github.com/miaskiewicz/turbo-surf/issues/115),
[Observable / `document.domain`](https://github.com/miaskiewicz/turbo-surf/issues/116),
audio ([#27](https://github.com/miaskiewicz/turbo-surf/issues/27)); in turbo-dom:
[XPath](https://github.com/miaskiewicz/turbo-dom/issues/185),
[XML parsing](https://github.com/miaskiewicz/turbo-dom/issues/186),
[CSS animations](https://github.com/miaskiewicz/turbo-dom/issues/187),
[focus / pickers / dialog](https://github.com/miaskiewicz/turbo-dom/issues/188),
[FileList / selector error text](https://github.com/miaskiewicz/turbo-dom/issues/189),
[RTL / vertical writing / quirks](https://github.com/miaskiewicz/turbo-dom/issues/190),
[shaping / font metrics](https://github.com/miaskiewicz/turbo-dom/issues/191),
[inset box-shadow](https://github.com/miaskiewicz/turbo-dom/issues/192), plus the
block/table layout residuals already filed there. Measurement history:
[findings](./harness/detection/FINDINGS.md),
[runbook](./harness/detection/CHROME-PARITY-RUNBOOK.md),
[remaining work](./harness/detection/PARITY-GAPS.md).


---

**TLS/HTTP-2 fingerprint (`impersonate`).** rustls can't forge Chrome's
JA3/JA4 + Akamai HTTP-2 fingerprint. The default MCP build uses BoringSSL
(`wreq`) with the [upstream Chrome 154 profile](https://github.com/0x676e67/wreq-util/blob/5715529002738f46eea86a7271f918f9374d3582/src/emulate/profile/chrome/tls.rs)
and the measured Chrome 155 navigation identity. Registry packages
`turbo-surf-wreq` 0.16.1 and `turbo-surf-wreq-util` 0.2.0 retain the Rust aliases
`wreq` and `wreq-util`; their BoringSSL chain uses `turbo-surf-btls`,
`turbo-surf-btls-sys` and `turbo-surf-tokio-btls` 0.5.6. The reviewed source pins
remain wreq `4ed3cf0c` in `greg-flux/wreq` (upstream base `c6fd8b1`),
wreq-util `5715529`, btls `f5a8a48` and BoringSSL `f1f2556`. Full revisions and the
reproducible licensed distributions are recorded in
[release-deps](rust/release-deps/README.md). These APIs require Rust 1.98.0,
pinned in `rust/rust-toolchain.toml`.

```bash
cd rust
cargo build --release -p turbo-surf-mcp --features impersonate
cargo test -p turbo-surf-core --test client_hello
```

The offline ClientHello regression checks the cipher sequence, signature-algorithm
GREASE, ML-DSA44/65/87, and exact public trust-anchor IDs against a local Chrome
154.0.8037.98 capture. The former empty trust-anchor patch is superseded by upstream
support. Surf libraries published from this source use the explicit named registry dependency
graph, so consumers do not need the former workspace Git patches. Preparing or
publishing those dependencies is separate from a passing
[actual packaged-consumer check](rust/release-deps/surf-consumer/README.md);
source review alone does not establish distribution compatibility.

The development candidate also negotiates genuine compression dictionaries from
partitioned native session state and decodes selected Brotli/Zstandard responses.
Policy-aware document/link discovery, independent encoder fixtures and Chrome
controls are described in the [dictionary parity notes](harness/detection/COMPRESSION-DICTIONARIES.md).
Dictionary-coded bodies now have bounded incremental decoding and cancellation
controls. Fresh live traces also show native dictionary acquisition/negotiation;
that does not demonstrate live dictionary decoding, complete browser parity or
Google acceptance. See the [live header boundaries](harness/detection/COMPRESSION-DICTIONARIES.md#final-live-google-observation--2026-10-16).

Native WebCrypto uses a reproducibly hardened Deno provider and genuine Rust
X-Wing keys for Chrome 155's `MLKEM768-X25519`. Key permissions and private native
cloning have adversarial regression coverage; the covered localhost algorithm
queries and KEM roundtrips match Chrome 155. The render crate's `deno_crypto`
alias resolves `turbo-surf-deno-crypto` 0.273.1 (upstream 0.273.0 plus the reviewed
patch), preserving the reviewed hardening and native clone API through its
matching Deno/V8 registry graph. See the
[provider provenance](rust/release-deps/deno/DENO_CRYPTO.md) and the
[itemized browser parity backlog](harness/detection/PARITY-GAPS.md) for remaining
behavior and validation limits.

Deno Web, WebIDL and Crypto JavaScript is compiled into the render crate in both
stock and native SDK builds; no runtime opt-in or build-machine Cargo source tree
is required. The [crate-local source manifest and reproducer](rust/crates/turbo-surf-render/DENO-EXTENSIONS.md)
documents the 28 exact licensed modules, taken from the Deno distributions'
`embedded_sources` tables (nothing vendored); the render tests check every module's
SHA-256 and length against reviewed pins. Installed-artifact JavaScript tests remain
a separate portability gate.

The default render tier's `deno_core` alias resolves
`turbo-surf-deno-core` 0.412.1. Its `v8` alias resolves the facade
`turbo-surf-deno-v8` 0.4.1, backed by turbo-dom's teardown- and
interceptor-hardened `turbo-dom-v8` 150.4.0 fork, the only V8 in the graph
(native engine 15.0.245.2, corresponding to
Chrome 150). Numeric versions and official native archive identities retain their
upstream meanings; package renaming does not upgrade the engine. The harness
records actual engine/template versions separately from the Chrome 155 identity. The TLS
upgrade does not upgrade the JavaScript engine. The reproducible
[Chrome 155 source profile](harness/detection/engine-155/README.md) builds and
validates actual V8 **15.5.35.23-rusty** and the corresponding SDK/context seams.
The historical [Chrome 154 profile](harness/detection/engine-154/README.md) remains
reproducible. [TRANSPORT-155.json](harness/detection/TRANSPORT-155.json) records zero
normalized differences for the measured local ClientHello, HTTP/2 settings/window
and navigation header-order controls against Chrome 155. The upstream transport
recipe is still 154; these controls do not cover all HTTP/2 frames or HTTP/3.
The differential harness records which engine was built; this experimental profile
does not change the published default or establish complete browser parity.

Enabled by default in `turbo-surf-core` and the MCP binary. Use
`--no-default-features` for the rustls backend. The differential harness enables the
`trust-anchors` compatibility alias, `gpu-metal` and `coretext`.

**Fingerprint seed pool.** `turbo-surf-core::fingerprint` holds ~4000 internally
coherent real-Chrome identities (UA ↔ client hints ↔ `navigator` all agree),
selected deterministically by a client key — the same host always gets the same
profile, while the fleet spreads across the pool. The MCP session and the crawl
navigator select profiles per host, but `FetchOptions.profile` only controls default
headers on the rustls backend. The wreq backend currently pins `default_profile()`;
rotating the render profile there can produce a mismatch. The differential harness
pins the native default in both layers. For rustls, use:

```rust
use turbo_surf_core::{fingerprint, net::{fetch_html, FetchOptions}};
let profile = fingerprint::select("example.com");      // stable per key
let opts = FetchOptions { profile: Some(&profile), ..Default::default() };
let res = fetch_html("https://example.com/", opts).await?;
```

**JS-challenge / PoW walls (Akamai, DataDome, Kasada, Cloudflare).** A fingerprint
gets you past *consistency* checks, not the active canvas/WebGL/PoW gates. Plug a
solver into the `ChallengeSolver` trait — the MCP session / crawl navigator
auto-detect a wall, solve it, and replay the cleared cookies on the fast path.
Inert until configured. Copy `.env.example` → `.env` and pick one:

- **Rented** (`TURBO_SURF_SOLVER=hyper|scrapfly` + `HYPER_API_KEY`/`SCRAPFLY_API_KEY`).
  **Hyper** (`akm.hypersolutions.co/v2/sensor`, `x-api-key`) generates an Akamai
  `sensor_data` payload; turbo-surf POSTs it to the target and harvests `_abck`
  (Akamai lane wired; other vendors fall back). **Scrapfly** (`/scrape?asp=true&
  render_js=true`) renders + returns cleared cookies (`result.cookies`). Both
  matched to their real API docs.
- **In-house, no key** (`TURBO_SURF_SOLVER=cloudflare`, `awswaf`, or `akamai`) —
  solve it yourself, no third party. **Cloudflare** runs the challenge's *own* JS in
  the V8 render tier to compute the answer (no reversing the math), then POSTs it and
  harvests `cf_clearance`. **AWS WAF Bot Control** (the bot layer behind CloudFront /
  ALB) does the same — runs `challenge.js` in V8 to mint the `aws-waf-token`; its
  common tier clears on the minted token + fingerprint, the `captcha` tier routes to
  the sidecar. **Akamai** is experimental (see below).
- **Self-owned browser sidecar** (`TURBO_SURF_SOLVER=browser` +
  `TURBO_SURF_BROWSER_CMD="node harness/browser-solver/solve.mjs"`) — drives a real
  hardened headless Chromium just for the handshake, harvests the cookie, then
  turbo-surf does the volume. Opt-in only (spawns a process); the browser stays a
  dev-side sidecar, never in the engine. See
  [`harness/browser-solver/`](./harness/browser-solver/README.md).

**Akamai experimental flow (`analyze_akamai`).** Akamai's live `sensor_data` is
per-version, obfuscated, and key-rotated, so the in-house solver isn't a guaranteed
live bypass — it's a **recon → rebuild → test** loop you drive from MCP:

```jsonc
// 1. navigate to an Akamai-walled page first (goto), then:
// MCP: analyze_akamai          → finds the live Akamai script, hashes it, probes
//                                what it reads, builds candidate sensors per version
{ }
// MCP: analyze_akamai          → with retry: POST each candidate, test live
{ "retry": true }               //   acceptance, and SAVE a working one locally
```

`retry` returns `accepted` (the version that cleared the wall, if any) and saves it
to `TURBO_SURF_SENSOR_DIR`. None accepted = that script's encoding still needs
reversing (the candidates are hash-seeded structural rebuilds). With
`TURBO_SURF_BROWSER_CMD` set, a normal Akamai `goto` auto-falls-back to the browser
sidecar when the in-house solve fails — so you stay unblocked while iterating.

Set `TURBO_SURF_PROXY` so the token's IP/JA3 matches your egress (and build with
`--features impersonate` so the replay JA3 matches the Chrome that minted it).

**Controllable render fingerprint.** Every render-tier `navigator` field has a
default from the registry's default release and is overridable at runtime via the MCP `set_fingerprint`
tool (or `turbo_surf_render::set_fingerprint(json)`):

```jsonc
// MCP: set_fingerprint
{ "overrides": {
    "userAgent": "...", "platform": "Win32", "vendor": "Google Inc.",
    "languages": ["en-GB", "en"], "hardwareConcurrency": 16, "deviceMemory": 8,
    "chromeMajor": 150, "screen": { "width": 2560, "height": 1440 },
    "devicePixelRatio": 2,
    "connection": { "effectiveType": "4g", "rtt": 50, "downlink": 10 },
    "userAgentData": { "platform": "Windows", "brands": [ /* … */ ] }
} }   // {} resets to the default release's macOS defaults
```

`stealth_status` reports the active profile, the wired solver, the pool size, and
any render fingerprint overrides.

**Debug mode (`probe`).** To see *what* a page's anti-bot JS reads — and what's
still missing — run the `probe` MCP tool (or `turbo-surf-render::probe_globals`):
it runs the page's scripts with `navigator`/`screen`/`window.chrome`/canvas
instrumented and reports every property touched plus the reads that returned
`undefined` (your shim to-do list). The runnable example
`cargo run -p turbo-surf-render --example probe-script -- script.js` does this on
any real captured challenge script.

## Playwright drop-in (run your e2e specs with no browser)

turbo-surf's page API is Playwright-shaped, and `rust/playwright-shim/` is a
**drop-in `@playwright/test` replacement** backed by the native engine — **no
Chromium**. A `register.mjs` module-resolution hook rewrites every
`import … from "@playwright/test"` (and `playwright` / `playwright-core`) to the
shim, so existing specs run **unchanged** on `node:test`:

```sh
node --import ./rust/playwright-shim/register.mjs --test 'e2e/**/*.spec.mjs'
```

It implements **Page**, **Locator**, **expect** (all five assertion classes),
**BrowserContext**, **APIRequestContext**, and **test + fixtures** over the engine:
navigation, every `getBy*` locator, locator composition/filtering, DOM actions
(`click`/`fill`/`check`/`selectOption`/…), `evaluate`/`render` in the V8 tier,
cookies + real `setExtraHTTPHeaders`, and the full matcher set. Most of the surface
is pure JS and never crosses into Rust; only genuine DOM/render semantics do (and
an `expect(locator)` chain is batched into one crossing).

`page.screenshot()` renders a **synthetic** image (PNG or SVG) from a native
layout+paint of the current HTML — no browser, no Chromium. It's *reasonably
representative*, not pixel-faithful (`position`/`z-index` are honored via CSS
stacking order; `<img>` + `background-image` are fetched and painted —
PNG/JPEG/GIF/WebP/BMP/ICO/AVIF/JPEG XL + SVG, unknown formats fall back to a placeholder). `full_page`
grows to the content height. Runs only when asked, over any HTML snapshot; the
viewport is configurable. See the `screenshot` MCP tool + `screenshot`/`screenshotSvg`
napi functions.

What a no-browser engine **physically can't do fails honestly** (it throws or
no-ops — never a silent pass):

| Bucket | Examples | Behavior |
|---|---|---|
| Pixels / layout | `pdf`, `boundingBox`, element-level `locator.screenshot`, `toHaveScreenshot`, `toMatchSnapshot` | **throws** |
| Input hardware | `hover`, `dragTo`, `mouse`/`keyboard`/`touchscreen` | **throws** |
| Network interception | `route`, `routeFromHAR`, `unroute` | **throws** |
| JS↔host bindings | `exposeFunction`, `exposeBinding` | **throws** |
| Truly-async/time | `waitFor*`, live `console`/`request` events, timer-driven mutation | resolve/no-op on the static DOM |
| Layout-only state | `viewportSize`, `emulateMedia`, frames | stored / collapse to self |

Full per-method coverage map: [`rust/playwright-shim/LIMITATIONS.md`](./rust/playwright-shim/LIMITATIONS.md).
Best fit: **server-rendered and hydration-on-navigation** apps. A client-only SPA
that paints entirely from JS after load (and never re-fetches) needs a real browser.

## Python

The engine is also a **PyO3 binding** on [PyPI](https://pypi.org/project/turbo-surf/) —
prebuilt abi3 wheels (CPython 3.8+) for Linux (x86_64/aarch64), macOS (arm64), and
Windows (x64), **no Rust toolchain, no Chromium**. It's fetch-free: you pass a
page's HTML in and get a view out (Markdown, visible text, links, a typed
extraction, an a11y tree); for JS-gated pages it runs the page's **own scripts** in
the same V8 isolate over a native DOM.

```sh
pip install turbo-surf
```

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
`turbo_surf.TurboSurfError`; the non-JS views never raise. Full function table:
[`rust/crates/turbo-surf-py/README.md`](./rust/crates/turbo-surf-py/README.md).

## Benchmarks — pending re-run

0.6.0 has not been re-benchmarked: the DOM, layout, raster and browser-environment code
moved into the shared turbo-dom crates and the render tier moved engines in this release,
so the earlier numbers no longer describe the shipped code. A re-run of all four turbo-*
repos together is planned after the release. The last measured (pre-consolidation) runs
— the Playwright-routine comparison against Chromium/Firefox/WebKit and the
crawler-vs-crawler races — are kept in
[`docs/BENCHMARKS-history.md`](./docs/BENCHMARKS-history.md). Harnesses:
[`harness/competitive/`](./harness/competitive/README.md) (`npm run harness`) and
[`harness/crawlers/`](./harness/crawlers/README.md) (`npm run crawl-bench`,
`crawl-bench:js`).

## Development

The engine is Rust (`rust/` workspace); the only JS is the launcher (`cli.js`/
`index.js`) + the dev harness.

```sh
cd rust
cargo test --workspace                      # the offline suite
cargo clippy --workspace --all-targets
cargo fmt
cargo build --release -p turbo-surf-mcp    # the MCP binary the launcher spawns
cargo bench -p turbo-surf-view             # Rust microbench (parse / views)
```

From the repo root: `npm run check` lints/formats the launcher JS, runs the Rust
gate, and runs the **Playwright shim** suites (`npm run test:playwright` =
`build:addon` → `test:shim` offline unit tests → `test:e2e` drop-in specs through
`register.mjs`, no Chromium). `npm run harness` / `crawl-bench` / `hotpath` run the
benchmarks (install `playwright` + `crawlee` ad-hoc for the competitor engines).

### Gates

PR CI (`.github/workflows/ci.yml`) runs on ubuntu:

- **lint + test** on Node 20 and 22 (launcher lint, the Rust workspace, the Playwright shim).
- **Coverage, blocking**: a sharded `cargo tarpaulin` (`coverage-shard` matrix: render,
  mcp, and core+view+page+transform), merged by the `coverage` job, which fails below the
  workspace `fail-under` (93.0, `rust/tarpaulin.toml`) or any per-crate floor in
  `rust/coverage-floors.json` — render 95.0, core 99.4, mcp 99.6, view 99.9, page and
  transform 100; the napi/py bindings are excluded.
- **Complexity, report-only**: `complexity / rust` and `complexity / js` (cc <= 15 per
  function) run with `continue-on-error` until the post-0.6.0 refactor clears the backlog.

macOS, Windows and Linux binaries and wheels are built only by the release workflows
(`release.yml`, `release-py.yml`). Details: [`docs/QUALITY-GATES.md`](./docs/QUALITY-GATES.md).

## License

MIT
