# turbo-surf

> Native-speed, **browserless** web crawler + **MCP server** for AI agents — built
> on [turbo-dom](https://github.com/miaskiewicz/turbo-dom). Fetch + parse + extract
> + run page JS in an isolated native engine.

> Working on this repo? See [docs/AGENTS.md](./docs/AGENTS.md) for one-command wrappers around the common workflows (oracle launch, Chrome 156 control recording, addon build, identity/parity checks, coverage, release prep).

turbo-surf's default execution path is a native Rust engine with isolated V8 page
scripts and an on-demand **synthetic screenshot** tier. Explicit browser solvers
and browser sidecars are optional integrations; see [solver configuration](./docs/SOLVERS.md).

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
   **no browser and no glue code** — `npx --yes --package turbo-surf turbo-surf-mcp`. Most crawlers are
   libraries you wrap yourself; this one is an agent tool on day one.
2. **Crawler + agent surface on one native engine.** The same engine bulk-crawls a
   domain and serves the MCP tools through the native execution path. Its page API
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

Status: **v0.8.2 package metadata; v0.8.3 release candidate in progress**.
This release branch pins the shared DOM crates to the exact revision recorded in
[CHANGELOG.md](./CHANGELOG.md); the resolved DOM version is 0.8.3. This is not a
claim that v0.8.3 has been published or that final integrated validation is green.
Registry install commands select published artifacts; use a source build and its
local launcher/runner to exercise this unreleased candidate.
The engine includes hardened
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
npx --yes --package turbo-surf turbo-surf-mcp
```

Node ≥ 20 to launch; the engine is a prebuilt per-platform native binary (no Node
runtime hosts it).

### What ships where

turbo-surf publishes to **three registries** (see [PUBLISHING.md](./PUBLISHING.md)):
a **`v*`** git tag ships the npm launcher + the Rust crates; a separate **`pyv*`**
tag ships the PyPI wheels.

| Artifact | Registry | What it is | For |
|---|---|---|---|
| [`turbo-surf`](https://www.npmjs.com/package/turbo-surf) | **npm** | Launcher, Playwright facade and matching optional platform packages carrying the MCP executable and NAPI addon (darwin arm64, linux x64/arm64-gnu, win x64; Intel macOS builds from source). | MCP/CLI runs the executable; the Playwright facade loads the addon in-process. Prebuilt installs need no Rust toolchain or browser. |
| [`turbo-surf`](https://pypi.org/project/turbo-surf/) | **PyPI** | A PyO3 binding, shipped as prebuilt abi3 wheels (CPython 3.8+) for Linux (x86_64/aarch64), macOS (arm64), Windows (x64). | Calling the engine **from Python** — `markdown`/`text`/`links`/`extract`/`render`/…. No Rust toolchain, no Chromium. |
| `turbo-surf-core`, `-view`, `-page`, `-render`, `-mcp` | **crates.io** | The Rust crates, in dependency order (the screenshot tier is turbo-dom's `turbo-dom-raster`). | **Embedding** the engine in a Rust program. |

The `turbo-surf-napi` and `turbo-surf-transform` Rust crates are not published
to crates.io. The native addon is distributed in the npm platform packages, and
the npm package includes the Playwright facade and `turbo-surf-test` command.
See [the artifact matrix](./docs/BUILD-FEATURES.md) and
[the addon loader](./rust/crates/turbo-surf-napi/README.md) for exact paths and
source-build overrides.

## MCP server (agents)

```sh
npx --yes --package turbo-surf turbo-surf-mcp   # stdio MCP server (80 tools), e.g.:
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
claude mcp add turbo-surf -- npx --yes --package turbo-surf turbo-surf-mcp
```

That writes the server into your Claude Code config. `npx --yes --package turbo-surf turbo-surf-mcp`
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
    "turbo-surf": { "command": "npx", "args": ["--yes", "--package", "turbo-surf", "turbo-surf-mcp"] }
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
with `npx --yes --package turbo-surf turbo-surf-mcp` in a terminal, then Ctrl-C). Shows `✗ failed` in
`claude mcp list` → run the command directly to see the error; on an unsupported
platform there's no prebuilt binary (build from a checkout as above).

**Other MCP clients** (Claude Desktop, Cursor, …) — point their MCP config's
`command` at `npx` with args `["--yes", "--package", "turbo-surf", "turbo-surf-mcp"]`, or at the binary path.

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
defaulting to Chrome 156.0.8078.80. Wire headers and the render-tier navigator read
from it. A new release is measured and written by one command —
[`harness/detection/reversion-chrome.mjs`](harness/detection/CHROME-REVERSION.md)
runs the Chrome identity battery (TLS/H2/headers/client hints/JS identity) and
diffs Chrome against turbo-surf; a reported version change does not upgrade V8
(the JS-engine gap stays recorded).

### Quick start (challenge solvers)

The native engine runs without a solver. To configure one from a source checkout:

```sh
cargo build --manifest-path rust/Cargo.toml --release -p turbo-surf-mcp
cp .env.example .env
# Edit .env: choose TURBO_SURF_SOLVER and any required key/sidecar command.
node cli.js
```

The MCP setup loads `.env` without replacing process environment values. Explicit
in-house solvers need no provider key. An unset or unknown selector auto-selects a
real `HYPER_API_KEY` first, then `SCRAPFLY_API_KEY`; with neither, no solver is
installed. Placeholder keys are ignored.

### Configuration reference

| Variable | Behavior |
| --- | --- |
| `TURBO_SURF_SOLVER` | `cloudflare`, `awswaf`, `akamai`, `recaptcha`, `hyper`, `scrapfly`, or `browser`. |
| `HYPER_API_KEY` | Key for the wired Akamai sensor-generation adapter. |
| `SCRAPFLY_API_KEY` | Key for the external ASP scrape/cookie adapter. |
| `TURBO_SURF_BROWSER_CMD` | Explicit browser solver command; also optional fallback when an in-house solve returns an error. |
| `TURBO_SURF_PROXY` | Solver-adapter hint; setting it alone does not configure native replay transport. |
| `TURBO_SURF_SENSOR_DIR` | Directory for accepted `analyze_akamai` candidates; default `akamai-sensors`. |

See [SOLVERS.md](./docs/SOLVERS.md) for provider dispatch, placeholder-key handling,
sidecar protocols, fallback triggers, and limitations. The in-house paths include
experimental and structural fallbacks; configured solving and fixture success do
not guarantee acceptance by a live challenge, CAPTCHA, or cookie replay.

### Build features and GPU bridge

[BUILD-FEATURES.md](./docs/BUILD-FEATURES.md) is the feature/default matrix for all
eight crates, release artifacts, toolchains and platform requirements. Cargo
features are additive: disabling a package's defaults does not necessarily disable
defaults enabled through its dependencies.

- **`impersonate`**: BoringSSL-backed transport, enabled in stock MCP/NAPI builds.
  The lower-level core can be built with its defaults disabled for rustls; consult
  the dependency matrix before assuming a top-level flag changes the full graph.
- **`gpu-metal`**: optional macOS wgpu/Metal replay for supported canvas/WebGL
  operations. MCP and NAPI install the corresponding callbacks; Python's feature
  forwarding alone does not install a guest WebGL callback.
- **`coretext`**: optional macOS canvas-text rasterization. It is independent of
  WebGL replay and does not establish Chrome PNG or arbitrary-scene pixel parity.
- **`native-sdk-frame-read`**: a separate opt-in browser-environment capability;
  enabling a feature does not install a Chrome SDK or make every SDK operation
  available.
- **`avif` / `jxl`**: enabled in stock screenshot-capable package builds; see the
  matrix for individual crate defaults and decoder/build requirements.

The [GPU bridge guide](./docs/GPU-BRIDGE.md) distinguishes Metal replay, CoreText,
SDK/frame integration, capability checks and fallback behavior. GPU/CoreText
features are off in stock release artifacts. Historical matching fingerprint
scenes are bounded measurements, not a general parity or fallback guarantee.

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
- **Private helpers.** Render helpers live on the engine namespace, a V8 private
  that page script cannot reach by any property or symbol route (turbo-dom#222);
  driver scripts reach it through the engine eval entry points (`__tsNs`).
- **Native-fn shape**: masked shims report `[native code]` **and** have no own `prototype` (like
  real native functions); the `toString` anti-hook trap is itself shapeless.
- **Canvas 2D**: `getImageData` returns the real rendered pixels (byte-identical to Chrome for
  selected solids/shapes), `measureText` uses the faces live layout uses (system faces plus the document's loaded `@font-face` / `FontFace` faces, #130; static `screenshot`s load the page's `@font-face` faces themselves, #142), context
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
[workers](https://github.com/miaskiewicz/turbo-surf/issues/104),
[canvas captureStream / offscreen / PiP, WebGL](https://github.com/miaskiewicz/turbo-surf/issues/106),
[IndexedDB event-path residuals](https://github.com/miaskiewicz/turbo-surf/issues/148),
[placeholder APIs (caches, cookieStore, Navigation, locks)](https://github.com/miaskiewicz/turbo-surf/issues/108),
[WebTransport over HTTP/3](https://github.com/miaskiewicz/turbo-surf/issues/175),
[abort reaction / microtask order](https://github.com/miaskiewicz/turbo-surf/issues/136),
[fetch residuals: compression-dictionary, Range, consumed-body text](https://github.com/miaskiewicz/turbo-surf/issues/137),
[CSP directives / reporting](https://github.com/miaskiewicz/turbo-surf/issues/110),
[form submissions to `_blank` / unknown names: no new windows, they throw (no-browser limit)](https://github.com/miaskiewicz/turbo-surf/issues/140),
[synchronous XMLHttpRequest](https://github.com/miaskiewicz/turbo-surf/issues/138),
[blob: script/style/image/frame sources](https://github.com/miaskiewicz/turbo-surf/issues/146),
[V8 15.0 vs 15.6](https://github.com/miaskiewicz/turbo-surf/issues/112),
[transport identity](https://github.com/miaskiewicz/turbo-surf/issues/113),
[Google SERP](https://github.com/miaskiewicz/turbo-surf/issues/114),
[GC / memory reclamation](https://github.com/miaskiewicz/turbo-surf/issues/115),
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

**Challenge solvers.** `ChallengeSolver` adapters can solve a detected challenge
and return cookies for native replay. The wired choices, configuration precedence,
experimental in-house behavior and explicit browser-sidecar exception are described
in [SOLVERS.md](./docs/SOLVERS.md). Hyper's current adapter is Akamai-specific;
provider product offerings do not imply that every vendor is wired here.

**Akamai inspection (`analyze_akamai`).** The tool locates and hashes the challenge
script and builds candidate sensors. `{"retry": true}` tests candidates and saves
an accepted one under `TURBO_SURF_SENSOR_DIR`. No accepted candidate means no solve
was established. An optional browser fallback runs only on the documented solve
error path; it does not guarantee that the target accepts the replay.

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

`page.screenshot()` renders a **synthetic** image (PNG, JPEG or SVG) from a native
layout+paint of the current HTML — no browser, no Chromium. It's *reasonably
representative*, not pixel-faithful (`position`/`z-index` are honored via CSS
stacking order; `<img>` + `background-image` are fetched and painted —
PNG/JPEG/GIF/WebP/BMP/ICO/AVIF/JPEG XL + SVG, unknown formats fall back to a placeholder). `fullPage`
grows to the content height. Runs only when asked, over any HTML snapshot; the
viewport is configurable. See the `screenshot` MCP tool + `screenshot`/`screenshotSvg`
napi functions.

The facade includes live DOM geometry and input, but remains a bounded browser
replacement. Current behavior is summarized below; the per-method
[coverage map](./rust/playwright-shim/LIMITATIONS.md) is authoritative.

| Surface | Current behavior |
| --- | --- |
| Geometry and media | Native bounding boxes, scrolling, viewport/media settings and live frame operations, subject to documented resource/layout admission. |
| Input | Locator actions, keyboard, hover, mouse/touch and drag event paths operate on the live engine; physical input and compositor behavior are not provided. |
| Network | Page-scoped fetch/XHR interception, callbacks and HAR playback; general document/resource interception and context routes have separate limits. |
| JS↔host | `exposeFunction` and `exposeBinding` callbacks, including asynchronous replies. |
| Waiting and events | Driven live-session timers, waiters and console/network events; static snapshots do not run a continuous background browser loop. |
| Unavailable capture | PDF, video, element screenshots and visual snapshot assertions remain explicitly unsupported. Page screenshots use the native synthetic tier. |

Client-rendered applications can execute their own scripts in the V8 tier.
Compatibility depends on the APIs and layout features the application uses, not
on whether it is called an SPA. Synthetic screenshots are representative native
renders; they are not browser compositor captures. Animation support remains
separate from geometry on an animated target: unaffected local reads are scoped,
while unsupported dependent reads still fail explicitly. A session-level
animations on/off policy is a v0.8.4 follow-up ([#500](https://github.com/miaskiewicz/turbo-surf/issues/500)).

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

Current release performance has not been established by the historical benchmarks.
The 0.6.0 consolidation and subsequent engine changes invalidate using those numbers
as a v0.8.3 speed claim. Current same-workload validation and profiling must identify
the exact source, build profile, workload, passing case counts and timing conditions. The last measured (pre-consolidation) runs
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

- **lint + test** on Node 22 (launcher lint, the Rust workspace, and Playwright shim).
- **Coverage, blocking**: a sharded `cargo tarpaulin` (`coverage-shard` matrix: render,
  mcp, and core+view+page+transform), merged by the `coverage` job, which fails below the
  workspace `fail-under` (93.0, `rust/tarpaulin.toml`) or any per-crate floor in
  `rust/coverage-floors.json` — render 95.0, core 99.4, mcp 99.6, view 99.9, page and
  transform 100; the napi/py bindings are excluded.
- **Complexity, blocking**: `complexity / rust` and `complexity / js` (cc <= 15 per function)
  gate the workflow; one limit for the whole repo, no allow-list.

macOS, Windows and Linux binaries and wheels are built only by the release workflows
(`release.yml`, `release-py.yml`). Details: [`docs/QUALITY-GATES.md`](./docs/QUALITY-GATES.md).

## License

MIT
