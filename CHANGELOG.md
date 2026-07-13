# Changelog

All notable changes to turbo-surf are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [0.3.5]

Consumes turbo-html2pdf-core 0.2.13 — a batch of real-site rendering fixes
(google.com / nike.com home pages): CSS Grid `justify-items` + `grid-column:span`
placement, `box-sizing:inherit` reaching the page, border-box height, `.ttc` face
index, `align-self`/min-width, lazy `<img>` data-urls, and text max-content
rounding. See turbo-html2pdf's CHANGELOG for the full list.

### Fixed
- **Lazy images render.** The delazy pass (which promotes `data-*-url`/`srcset` to
  `src`) now also injects `opacity:1;visibility:visible` so an assume-loaded image
  isn't dropped as `opacity:0` — nike's hero `<img>`s start `opacity:0` and had
  rendered as a blank band.
- **Glyphs trace from the right `.ttc` sub-font.** The raster parses the font face at
  its collection index (from `FontFace::index()`), so text in a system Arial/Helvetica
  collection no longer renders shifted.

## [0.3.4]

Consumes turbo-html2pdf-core 0.2.7 (mask-image icons, `white-space`, data-URI
values, pseudo-element/self-ref-var/flex/table fixes, flex perf) so complex pages
render like Chromium.

### Added
- **CSS `mask-image` icon painting.** The raster stencils a box's tint colour
  through a mask SVG's alpha (`tint_pixmap`), so monochrome UI glyphs (Wikipedia's
  language/menu/edit icons, Codex icon fonts) render as their shape instead of a
  solid square. A missing mask paints nothing.
- **`border-radius` rounded rects/borders** (rounded radio/checkbox circles, cards).

### Fixed
- Wikipedia renders faithfully end-to-end: title bar no longer overlaps the tabs,
  taxobox montage contained + classification aligned, full-width header, page-action
  tabs on one line, blue links, round radios.

## [0.3.3]

Hydration reach + screenshot fidelity. Consumes turbo-html2pdf 0.2.6 (real-page
layout/cascade: `var()`, sibling/structural selectors, inline-block-in-line,
grid-template shorthand, sr-only hide, static-position absolutes, float text-wrap,
`@media(...)` no-space). (0.3.2 was staged but never released; 0.3.3 supersedes it
and carries everything below.)

### Added
- **Broader JS hydration** in the render tier so real page scripts reach a
  browser-faithful DOM: `document.implementation.createHTMLDocument` (jQuery init),
  `Node.prototype.replaceChild`, `PerformanceObserver`, and DOM interface
  constructors (`HTMLElement`/`Node`/`Element`/… with duck-typed `instanceof`) so
  React/emotion `instanceof` checks resolve instead of aborting the bundle.
  Combined, MediaWiki's startup now runs (sets `client-js` + Vector layout
  classes), so Wikipedia hydrates with its taxobox + images.
- **Lazy / responsive image recovery** (`delazy_images` + `image_urls`): pulls the
  URL from `data-src`/`srcset`/`data-srcset`/`data-original`/`data-*-url` when
  `src` is absent, and fills a missing `src` before layout so the box + pixels
  render. Nike-style `data-landscape-url` images now paint.
- **Opt-in system-font screenshots** (`system_fonts` on the raster/napi assets
  entries): resolve a page's fonts against the machine's installed fonts to match
  a browser on the same host.

### Fixed
- **Resilient hydration**: a mid-hydration JS error now returns the partially
  hydrated DOM instead of discarding everything — partial hydration beats none.
- **HTML-entity-decode** extracted stylesheet/image URLs (`&amp;` in `load.php`
  query strings), so Wikipedia's real skin CSS loads instead of a stub.

## [0.3.2]
Screenshot fidelity: CSS positioning + z-index stacking, image rendering, full-page.

### Added
- **`position` + `z-index` in synthetic screenshots** — screenshots now honor
  `position: relative | absolute | fixed | sticky` (out-of-flow boxes are removed
  from flow and placed against their containing block; `relative` boxes shift
  while keeping their space) and paint children back-to-front in CSS stacking
  order (CSS 2.2 §9.9) instead of raw DOM order, so overlapping menus/modals layer
  correctly. Powered by the layout work in turbo-html2pdf 0.2.5, consumed via the
  new `Fragment::paint_order` in `turbo-surf-raster`'s PNG + SVG paint walks.
- **Image rendering** — `<img>` and `background-image` boxes now paint real
  pixels: **PNG, JPEG, GIF, WebP, and SVG** (SVG rasterized via resvg). The raster
  normalizes every decoded image to RGBA and hands turbo-html2pdf a re-encoded PNG
  purely for sizing, so format support lives entirely in the raster. The
  `screenshot` MCP tool and the Playwright shim fetch the page's image bytes over
  the session client (impersonation + cookies apply), `{ images: false }` opts
  out; the raster scales them into the layout box (PNG output) or embeds a base64
  PNG `data:` URI (SVG output). New `image_urls` + `screenshot*_with_assets`
  across raster/napi/Python for callers that fetch the bytes themselves; new
  `net::fetch_bytes` (+ napi `fetchBytes`) returns raw bytes without charset
  decode. Unknown formats fall back to a placeholder.
- **Full-page screenshots** — `full_page` (MCP `full_page`, shim `fullPage`,
  napi/Python `full_page`) grows the image height to the full laid-out content
  height (viewport width still drives layout), clamped to 24k px. The `screenshot`
  reply reports the true image `width`/`height`.

### Changed
- `turbo-surf-raster` now depends on `turbo-html2pdf-core` **0.2.5** (positioning,
  `paint_order`, `layout_html_with_images`), the `image` crate (PNG/JPEG/GIF/WebP
  decode), and `resvg` (SVG rasterization).

## [0.3.1]
Screenshot fidelity: external stylesheets + root-background propagation.

### Added
- **External `<link>` stylesheet fetching** — the `screenshot` MCP tool and the
  Playwright shim now fetch the page's external stylesheets (via the session /
  engine client, so impersonation + cookies apply) and cascade them, so a page
  that keeps its CSS in `<link>` sheets renders styled instead of unstyled.
  `{ external_css: false }` opts out. New `stylesheet_hrefs` +
  `screenshot_with_css` / `screenshot_svg_with_css` across napi + Python for
  callers that fetch the CSS themselves.

### Fixed
- **Root/body background propagation** — a page whose `<body>`/root carries a
  background colour now fills the whole image with it (matching how browsers
  paint the viewport canvas), instead of a white backdrop under short content.

### Notes
- Still no `position: absolute/fixed` or `z-index` / stacking-context model:
  out-of-flow and layered elements paint in document order, so overlapping
  menus / modals on complex pages can stack incorrectly.

## [0.3.0]
Synthetic screenshots — turn any HTML snapshot into an image with no browser.

### Added
- **`turbo-surf-raster` crate** — a screenshot tier that drives
  [turbo-html2pdf](https://crates.io/crates/turbo-html2pdf-core)'s native
  HTML/CSS layout + font engine and paints the resulting `Fragment` display list
  two ways: a **PNG** raster (tiny-skia; boxes, borders, glyph outlines) and an
  **SVG** vector (self-contained glyph `<path>`s). A *reasonably representative*
  render, not pixel-faithful: fragments paint in DOM order (no z-index/stacking
  model) and `<img>` draws as a placeholder. Runs only when asked.
- **`screenshot` MCP tool** — `{ format: png|svg, snapshot?, width?, height? }`;
  renders the current page or any **hydration-trail snapshot** (`dom_history`
  index). PNG returns base64, SVG the document string.
- **napi `screenshot` / `screenshotSvg`** and **`page.screenshot()`** in the
  Playwright shim (returns a real Buffer; `{type:"svg"}` for vector). Element-
  level `locator.screenshot()` still throws (no per-element geometry to crop to).
- **Configurable viewport** — a default `1280×800` layout viewport, overridable
  per call (napi/shim/MCP args) or per session (`set_viewport` MCP tool). Ships
  in every build, including the `impersonate` mega crawl build.
- Python parity: `turbo_surf.screenshot()` / `screenshot_svg()` on the PyPI
  wheel, matching the napi + MCP surfaces.

### Notes
- `<script>`/`<style>`/`<noscript>`/`<template>` source is stripped before
  layout so it never paints as visible text (page `<style>` CSS still cascades).
- No-JS render: visuals painted only by JavaScript (e.g. a `<canvas>` gradient
  background) are absent, so a page whose text is styled for such a background
  can look low-contrast. External `<link>` stylesheets are not fetched.

## [0.2.7]
In-house solver maturity: a proper Cloudflare solve (run the challenge's own JS),
Akamai experimental recon/rebuild tooling, and versioned encoding registries.

### Fixed
- **Hyper Solutions adapter matched to the real API** (`hyper-sdk-go`) — was built
  on wrong assumptions. Correct now: `POST akm.hypersolutions.co/v2/sensor` with the
  `x-api-key` header and `{abck,bmsz,version,pageUrl,userAgent,script,acceptLanguage,
  ip}` body; the response `{payload}` is the `sensor_data` string, which turbo-surf
  POSTs to the target to harvest `_abck`. Akamai lane verified end-to-end (mock);
  other vendors return `Unsupported` to fall back. Scrapfly adapter validated
  against its docs (`/scrape?asp&render_js`, `result.cookies[].name/value`) — already
  correct.

### Added
- **AWS WAF Bot Control solver** (`turbo-surf-core::aws_waf`,
  `TURBO_SURF_SOLVER=awswaf`) — the bot layer behind CloudFront / ALB. Classifies
  the tier (common / targeted `challenge.js` / captcha), runs `challenge.js` in the
  V8 tier (same `PowEngine` as Cloudflare) to mint the `aws-waf-token`, and replays
  it; the `captcha` tier routes to the browser sidecar. New `Vendor::AwsWaf` +
  detection (`x-amzn-waf-action`, `aws-waf-token`, `*.awswaf.com`).
- **Universal browser fallback for in-house solvers** — `cloudflare`, `awswaf`, and
  `akamai` now all try their self-solve first and fall back to the browser sidecar
  when `TURBO_SURF_BROWSER_CMD` is set (via `FallbackSolver`), so a failed in-house
  solve still clears the wall.
- **Proper Cloudflare solver (run the challenge's own JS)** — a `PowEngine` trait
  (core) implemented by the render tier (`turbo_surf_render::V8PowEngine`) lets
  `CloudflareSolver` execute the interstitial's challenge script in the V8 isolate
  and use the answer it computes, instead of the structural placeholder — the
  proper self-solve for CF's JS-compute challenge, no browser. Wired in the MCP
  session via `solver_from_env_pow`.
- **Akamai experimental routing + `analyze_akamai` MCP tool** — the in-house Akamai
  solver is now flagged experimental and, when `TURBO_SURF_BROWSER_CMD` is set,
  routes through a `FallbackSolver` (try in-house → fall back to the browser
  sidecar). New `analyze_akamai` tool: probe the live Akamai script on the current
  page, hash it, build candidate `sensor_data` per stored version, and with
  `{retry:true}` POST each candidate, test live acceptance, and **save a working
  sensor locally** (`TURBO_SURF_SENSOR_DIR`).
- **Versioned solver encodings** — both in-house solvers now store *multiple*
  generations of their challenge encoding behind a registry, since Akamai/CF shift
  format across versions. Akamai `SensorVersion` {V1 plaintext, V2 PRNG-shuffled,
  V3 encrypted-blob} via `generate_sensor_versioned`; Cloudflare `ChallengeVersion`
  {Iuam, Managed, Turnstile} via `detect_version` + `solve_pow_versioned`
  (Turnstile flagged non-self-solvable → routes to the browser sidecar). A harness
  test per vendor sweeps every stored version (deterministic + distinct + correctly
  tagged), so filling one version's real encoding keeps the rest green. Default
  tracks the latest generation.

## [0.2.6]
**Look like a real Chrome on the wire.** The stock client sent a bare
`turbo-surf/0.1` UA + a thin `Accept` and a generic rustls TLS/HTTP-2
fingerprint — an instant tell for WAFs.

### Added
- **Chrome default headers** (default, rustls path, no new build deps) — every
  fetch now sends a current Chrome 149 (macOS) UA plus the full navigation header
  set (`accept`, `accept-language`, `sec-ch-ua`/`-mobile`/`-platform`,
  `sec-fetch-*`, `upgrade-insecure-requests`), values matched against a live
  real-Chrome capture. `accept-encoding` stays client-managed so auto-decompress
  still works; caller/crawl headers still override.
- **`impersonate` feature** (opt-in, BoringSSL) — swaps the reqwest+rustls client
  for `wreq`/`wreq-util`, presenting a real Chrome TLS/JA3/JA4 + HTTP-2 (Akamai)
  fingerprint. Off by default (needs a C toolchain — cmake/nasm — to build);
  forwarded by `turbo-surf-{page,napi,mcp}`. A single `http_backend` alias in
  `turbo-surf-core` swaps the backend in one place. New live e2e
  (`tests/impersonate.rs`) asserts a Chrome JA4 + HTTP-2 fingerprint against a
  public echo (auto-skips offline); a localhost e2e asserts the Chrome headers
  reach the wire on the default path.
- **Real Chrome `navigator` in the JS render tier** — `ENV_BOOTSTRAP` now installs
  a coherent Chrome 149 (macOS) `navigator` (UA, `platform`, `vendor`,
  `webdriver: false`, `hardwareConcurrency`/`deviceMemory`, a Chrome PDF plugin
  set) plus a `window.chrome`, and masks the JS polyfills' `Function.prototype.
  toString` so built-ins (`fetch`, `setTimeout`, …) report `[native code]`.
  Replaces the old `turbo-surf`/`turbo-test` tell page JS used to see. No-Chromium
  env emulation: satisfies passive/consistency anti-bot probes, not active
  canvas/WebGL/audio fingerprinting or PoW challenges.
- **Fingerprint seed pool** (`turbo-surf-core::fingerprint`) — ~4000 internally
  coherent real-Chrome identities, selected deterministically by a client key
  (stable per client, spread across the fleet). Opt-in via `FetchOptions.profile`;
  the default reproduces the prior fixed Chrome-149/macOS wire behaviour. Raises
  the passive/consistency anti-bot bar; does not defeat active fingerprinting/PoW.
- **Challenge-solver integration** (`turbo-surf-core::challenge`) — detect a JS-
  challenge/PoW wall (Akamai/DataDome/Kasada/Cloudflare) and hand it to a server-
  side solver (`ScrapflySolver`/`HyperSolver`) that returns tokens/cookies to
  replay. Configured via env / `.env` (`HYPER_API_KEY`, `SCRAPFLY_API_KEY`,
  `TURBO_SURF_SOLVER`, `TURBO_SURF_PROXY`); inert until a real key is set. See
  `.env.example`. Wired into the MCP session (per-host profile + auto solve/replay
  on a detected wall) and `TurboNavigator` (the crawl seam). Also a self-owned
  `BrowserSolver` (opt-in `TURBO_SURF_SOLVER=browser` + `TURBO_SURF_BROWSER_CMD`)
  that shells to a hardened-headless sidecar over a JSON contract — Chromium stays
  out of the engine; reference sidecar in `harness/browser-solver/`. MCP
  `stealth_status` tool reports the active profile + wired solver.
- **In-house Akamai solver** (`turbo-surf-core::akamai`, `TURBO_SURF_SOLVER=akamai`,
  no key) — the first hand-written `ChallengeSolver`: `generate_sensor` builds a
  deterministic Akamai-shaped `sensor_data` payload, `AkamaiSolver` POSTs it to the
  sensor endpoint and parses the cleared `_abck`. Structure + POST/parse flow are
  tested + green; the dynamic field encoding a live edge validates still needs
  keying off a real `_abck` script (use the `probe` mode).
- **In-house Cloudflare solver** (`turbo-surf-core::cloudflare`,
  `TURBO_SURF_SOLVER=cloudflare`, no key) — parse the managed-challenge
  interstitial (`window._cf_chl_opt`), solve its (JS-compute) PoW, POST to the
  challenge-platform endpoint, harvest `cf_clearance`. Structure + flow green; real
  per-version PoW math keyed off a live challenge. Turnstile-interactive stays on
  the browser sidecar.
- **`probe-script` example** (`cargo run -p turbo-surf-render --example probe-script
  -- script.js`) — run the `probe` instrumentation over a real captured anti-bot
  script and print what it touched + the shim gaps. Run against a real Akamai
  sensor it surfaced two missing `navigator` props (`connection`,
  `userAgentData`), now added to the render-tier navigator (coherent with the UA).
- **Runtime-controllable render fingerprint** — every render-tier `navigator` field
  (UA, platform, vendor, languages, hardwareConcurrency, deviceMemory, chromeMajor,
  connection, userAgentData, screen, devicePixelRatio) now has a Chrome 149 default
  and is overridable via `turbo_surf_render::set_fingerprint(json)` / the MCP
  `set_fingerprint` tool. `stealth_status` reports the active overrides.
- **Fingerprint debug/probe mode** (`turbo-surf-render::probe_globals`, MCP `probe`
  tool) — run a page's JS with `navigator`/`screen`/`window.chrome`/canvas wrapped
  in logging proxies and report every property it touched + which reads returned
  `undefined` (the shim to-do list). Recon for what an anti-bot check probes and
  what's left to emulate.

## [0.2.5]
A **pooled-render latency** fix on the JS-crawl fast path.

### Fixed
- **Watchdog join latency in `render_page_pooled`** — the per-page execution-budget
  watchdog polled completion on a 2 ms sleep loop, so `watchdog.join()` after a healthy
  render blocked until the watchdog woke from its current sleep, adding up to 2 ms of
  latency to *every* pooled render. The watchdog now `park_timeout`s on the budget
  deadline and is `unpark`ed the instant the render completes, so a healthy render's join
  returns in µs (an elapsed-guard loop survives spurious wakeups; the deadline still
  terminates a runaway script). Measured on `quotes.toscrape.com/js` (warm pool):
  **renderPooled 2.6 ms → 1.3 ms (−50%)**, output byte-identical. The politeness/network-
  bound crawl wall is unchanged; CPU/parallel render throughput ~doubles.

## [0.2.4]
A **Linux SIGBUS** fix in the Playwright-shim test harness, plus a new **Python
(PyPI) binding**.

### Fixed
- **SIGBUS on Linux running the shim suite** (#6) — root cause was a bug in the shim's
  fake `@playwright/test` harness: `test.describe(...)` was registered as a node:test
  *test* instead of a *suite*, so the nested `test(...)` calls in a describe body fired
  on the global runner while the parent test was still running. node:test cancelled them
  ("test did not finish before its parent and was cancelled"), and the dangling async
  test — still holding a live-session V8 isolate — was torn down at process exit, which
  faulted with SIGBUS on Linux (macOS tolerated it). Latent until v0.2.3 wired a real
  `npm test`, so the multi-file shim run never executed on CI before. `makeDescribe` now
  registers a real node:test suite, so nested tests are awaited and torn down cleanly.
- **V8 platform init hardening** (defense-in-depth) — `ensure_platform()` initializes the
  V8 platform once on a dedicated, parked keeper thread (deno_core otherwise inits it
  lazily on whichever thread builds the first runtime; a transient one that then exits
  orphans the platform). Called before any worker isolate is created from `evaluate`,
  `render`, `render_pooled`, `hydrate`, and `live_open`.

### Added
- **Python binding (`turbo-surf` on PyPI)** — a PyO3 abi3 wheel (CPython 3.8+) exposing
  the stateless parse → view/extract → JS-render surface (`markdown`, `text`, `links`,
  `query`, `extract`, `evaluate`, `render`, `transform`, …), mirroring the Node N-API
  functions. New crate `rust/crates/turbo-surf-py`; `release-py.yml` builds + publishes
  wheels on a `pyv*` tag (gated on a `PYPI_TOKEN` secret). A real `test` npm script + the
  stale shim-assertion fixes from v0.2.3's CI work are included.

## [0.2.3]
A **JS-render speed** pass on the crawl path: the render tier built a fresh V8 isolate
per page (boot + the ~90 KB env bootstrap + parse dominate), so a JS-mode crawl paid
the full isolate boot on every page. A pooled fast path reuses one isolate across pages
— boot is paid once per worker thread — with a cross-page global scrub so a reused
isolate still renders like a fresh navigation. **11.5 ms → 3.2 ms per page** on
`quotes.toscrape.com/js` (3.6×), output byte-identical to the fresh render.

### Added
- **Pooled render fast path** — `render_page_pooled` (Rust) / `renderPooled` (napi)
  reuses a thread-local V8 isolate across pages, on a persistent render worker (one
  long-lived thread + one reused tokio runtime). Per-page session repoint
  (base/cookies/UA) + a global scrub (`SCRUB_GLOBALS`) restore fresh-navigation
  semantics; a budget-terminated/errored runtime is dropped instead of repooled. The
  competitive JS adapter drives it; `render` (fully isolated, fresh-per-page) is
  unchanged for correctness-sensitive callers.
- **`harness/hotpath/render-bench.mjs`** — reusable offline profiler for `native.render`
  / `renderPooled` (faithful script extraction, cached sample, A/B + parity check).

### Notes
- Cross-page isolation is intentionally relaxed for crawl speed (matching the existing
  `EVAL_RT` stance): the scrub reverts page-ADDED globals, not builtins mutated in place.
- A V8 code cache for the bootstrap + page bundle was tried and reverted — with a fresh
  isolate per page, `ConsumeCodeCache` costs more than a re-parse. Isolate reuse is the
  real lever.

## [0.2.2]
The headless **Playwright-shim parity** push: the payroll-app Playwright e2e suite
now runs through the browserless shim (over the napi addon, **no Chromium**), driving
a real authenticated Next.js App Router SPA. Side-by-siding every failure against real
Chromium (reseeded per suite) drove the engine to parity — the suite's remaining reds
all reproduce in Chromium too (app/backend/test data, not the engine). See
`HEADLESS-HYDRATION.md` for the full record.

### Added
- **ES module support in the render tier** — `<script type=module>` + `import` graphs
  fetched/linked over the host net (shared cookie jar, same-origin), wired into the
  hydration pump. Turbopack-dev entry execution (`document.currentScript` in the module
  pump, `__name` helper); classic `<script src>` chunks with ESM bodies route to the
  module pump.
- **Live-isolate interaction drive** — `getByRole/getByText/getByLabel` resolve and
  dispatch IN the running isolate (live `querySelectorAll('*')` index), so fills/clicks
  reach the real app, not just the static snapshot. Web-first assertion retry
  (re-pumps the live app between tries); `page.on('response')` / `waitForResponse`
  backed by a real network log; fetch-aware drain (`__pendingFetches`) keeps pumping
  until a mutation's success re-render lands.
- **Nth-aware scope chain for nested locators** — `parent.nth(i).getBy*()` walks the
  chain via `__tcResolveScoped` (a CSS-concat selector can't express "the i-th match's
  subtree"); `getByRole/Text/Label`/`getByTestId` scope to the parent's subtree.
- **CSS `:hover` simulation** — hover-revealed menus (incl. emotion's nested `&:hover`)
  become visible by flattening the matched rules inline.
- **Download capture + `ElementHandle` + polling `waitForFunction`**; keyboard events +
  `navigator.clipboard`; `structuredClone`; `addInitScript`; `setInputFiles`;
  per-test fixture sharing; `test.extend` custom fixtures.

### Fixed
- **App Router RSC hydration unblocked** — defined `document.location` (a browser
  invariant the dev RSC flight client reads via `findSourceMapURL`); its absence threw
  inside the flight-stream parse and the React root suspended forever. 0 → 488 fibers on
  the live payroll route.
- **RSC soft-nav follow + query preservation** — Next client navigation
  (`router.push/replace`) fetches the target's RSC flight and never advances
  `location` headlessly; the target is recorded on `__rscNav` and re-loaded hop-by-hop
  (login redirect chain completes). It now records `pathname + search + hash` (was
  `pathname` only — dropped `?employeeIds=` etc.) and strips Next's `_rsc` cache-buster.
  Guard: `rsc_soft_nav_preserves_query_and_strips_rsc_param`.
- **`reload({waitUntil:'networkidle'})` re-hydrates the live SPA** — `reload` ignored its
  options (unlike `goto`), leaving the reloaded doc as the raw un-hydrated shell (a
  settings select read `""`). Guard: surface "reload re-hydrates the live SPA".
- **`Locator.filter()` scopes child locators** — `cards.filter({hasNotText: x}).first()
  .getByTestId('y')` resolved against the UNFILTERED set; a serializable
  `hasText`/`hasNotText` spec now rides the scope chain. Guard:
  `scoped_resolve_applies_filter_before_indexing`.
- **Browser-accurate click** — pointerdown→mousedown→focus(only if mousedown not
  preventDefault'd + focusable)→pointerup→mouseup→click, pointer events first (MUI
  v7/Radix gate on them); honors `preventDefault` so an `<a href="#">` whose onClick
  toggles state doesn't also navigate.
- **Playwright `isVisible` semantics** — `is_visible` ignores `aria-hidden` (pure
  CSS/layout, like Playwright) and treats effective `opacity:0` / `display:none` /
  `visibility:hidden` ancestors as hidden (closing MUI modals resolve
  `waitFor(state:'hidden')`).
- **Drain/timer correctness** — runtime-injected `<script>`s run during interaction
  drains (`next/dynamic` lazy modals); the virtual-timer budget is RELATIVE per drain so
  a closing MUI Fade's short exit timer isn't killed.
- **Shim parity** — `waitFor` polls the requested state (visible/hidden/attached);
  `page.evaluate` awaits a returned Promise; `waitForResponse` won't match a response
  from an earlier step; `about:blank` is a no-fetch blank doc; `waitForFunction` returns
  the function's value; RegExp locator names; `boundingBox` → null. Locale: the shim no
  longer force-seeds `NEXT_LOCALE=en-US` (matches Playwright's es-MX default).

## [0.2.1]

### Fixed

- **Authenticated SPA pages now render headlessly.** Heavy authed routes (e.g. a
  payroll people/grid page) previously committed an empty body in the render tier
  while rendering fine in a real browser. Two render-tier bugs let third-party
  scripts spin to the render budget so React never committed:
  - **Virtual-clock timers.** `__runTimers` used `delay` only as a sort key, so a
    self-rescheduling `setTimeout` poll (analytics SDKs do this) fired until the
    raw count cap, starving the budget. A virtual clock now gates delayed timers
    (`due = now + delay`, advance on fire, stop past a 15s virtual ceiling) so
    polls fire a browser-like number of times and the page quiesces.
  - **`<iframe>` `contentWindow`.** Analytics SDKs read a builtin's native
    prototype off a throwaway iframe's `contentWindow`; with none present they
    recreated an iframe on every lookup (hundreds of churned iframes). Iframes now
    get a lightweight stub whose `contentWindow` is the current realm (the lookup
    caches, the loop stops) and that never enters the rtdom tree.

  New tests: `virtual_clock_bounds_self_rescheduling_timers`,
  `iframe_content_window_exposes_builtins`.

### Added

- **Authenticated SPA journeys hydrate + drive headlessly.** Next.js App Router
  pages render through the render tier and the Playwright shim can log in and drive
  them end to end (no Chromium). Against clean staging: payroll-wizard 5/5, invites
  26/26, auth-guards 32/38. Key enabler: define `document.location` (mirrors
  `window.location`) so Next's DEV RSC-flight parse doesn't abort (0 → 488 fibers).
- **ES module support** in the render tier: `<script type=module>` + `import` graphs
  fetched over the net, classic `<script src>` chunks with ESM bodies routed to the
  module pump.
- **Shim parity surface:** `page.waitForEvent('download')` + a `Download`
  (`path()`/`saveAs()`/`suggestedFilename()`) backed by a real `URL.createObjectURL`
  registry + `<a download>` capture; `Locator.elementHandle()` + a polling
  `page.waitForFunction(fn, handle)`; CSS `:hover`-revealed menus become visible
  (`__tcApplyHover` flattens nested emotion `&:hover` rules and applies the reveal
  inline); locator subtree scoping for `getByTestId`/`getByRole`/`getByText`/
  `getByLabel`, including an nth-aware scope chain for `steps.nth(i).getBy*`.

### Fixed

- **`waitForResponse` no longer matches a response from an earlier step** — it tags
  each drained response with the interaction that produced it and only accepts one
  from the current action or later (Playwright "after the call" semantics), so a
  loose URL predicate can't grab a prior step's response.
- **Visibility matches Playwright `isVisible`:** effective `opacity:0` (self/ancestor)
  reads hidden so a faded-out MUI modal resolves `waitFor(state:'hidden')`; and
  `aria-hidden` is NOT treated as hidden (a decorative aria-hidden icon carrying a
  test-id is still visible — aria-hidden stays an accessibility-query concern).
- **Modals open/close reliably:** the virtual-timer budget is RELATIVE per drain (a
  closing Fade's late exit timer fires), and `drain_to_quiescence` runs runtime-
  injected `<script>`s so `next/dynamic` lazy modals load on click.
- **Interactions are browser-accurate:** click fires the full pointer→mouse→click
  sequence (focus only when mousedown isn't preventDefault'd), and
  getByRole/getByText/getByLabel resolve in the LIVE isolate so portal'd MUI options
  dispatch their onClick.
- **Locale parity:** the shim no longer force-seeds `NEXT_LOCALE=en-US`; with no
  cookie the app resolves its default locale (es-MX), matching real Playwright.

New tests (render + shim `surface.test.mjs`): `createobjecturl_anchor_download_is_
captured`, `hover_reveals_css_hover_menu`, `tcgetby_scopes_to_root`,
`tcresolvescoped_walks_nth_chain`, `aria_hidden_stays_visible`, waitForResponse
staleness guards, getByTestId/getByRole scoping guards.

Additional shim fixes:

- **`about:blank` navigation** is a no-fetch blank document (a `goto`/`reload` of
  about:blank used to hit the net layer → `builder error for url (about:blank)`).
- **`waitForFunction` resolves the function's return value** (not a boolean) and
  works against a static snapshot — `Number(await waitForFunction(() => 1 + 1))`
  is `2` again (regression from the polling-handle rewrite).
- **`test.extend` custom fixtures inject** — a test fn with its own extDefs (a
  custom fixture or a `page` override) opens its own fixture set instead of
  reusing the base-only shared one (they resolved to `undefined` before).
- **Test isolation:** the env-var-mapping test restored `TURBO_SHIM_*` with
  `delete` instead of `= undefined` (the latter coerces to the string
  `"undefined"`, leaking `testIdAttribute`/`baseURL` into every later context and
  cascading failures across the serial suite).

## [0.2.0]

**turbo-surf is a browserless, native-speed crawler _and_ Playwright-compatible
script runner for AI agents — one engine, no Chromium.** It fetches, parses, and
acts on pages on its own native DOM ([turbo-dom](https://github.com/miaskiewicz/turbo-dom),
the `turbo-dom` Rust crate), and for JS-gated pages it runs the page's own scripts
in a **true V8 isolate** (a `deno_core` runtime — host heap unreachable from the
guest, with a runaway-execution budget) and re-renders the DOM. No headless
browser, no pixels, no layout.

What it does:

- **Crawl** — point it at a domain and stream page records: indexed interactive
  elements, a link/form graph, an accessibility tree, markdown and plain-text
  views, rendered-HTML capture, CSS/XPath queries, schema-driven structured
  extraction. Concurrency + per-host politeness (token-bucket), backoff/retry,
  canonical-dedupe, robots + crawl-delay, depth/page caps.
- **Drive pages (Playwright-compatible)** — the same `chromium.launch()` →
  `page.goto()` → locators → actions → `expect` surface, plus a `@playwright/test`
  drop-in `test` runner, so existing Playwright scripts/tests run unchanged against
  the engine instead of a browser. Network events, request routing/mocking, and
  persistent context state (cookies + `localStorage` + `storageState`).
- **Run page JS, no browser** — recover SPA data either by mining server-embedded
  hydration state (`__NEXT_DATA__`, JSON-LD, `__APOLLO_STATE__`, …) or by executing
  the page's own scripts in the V8 isolate (real jQuery / React-style bundles
  render); `fetch`/`XMLHttpRequest` are bridged to the host net layer.
- **Agent surface** — a 60-tool **MCP** server (stdio JSON-RPC) agents drive
  directly: navigate, click/fill/submit, query, extract, accessibility tree,
  markdown, `crawl`, `batch`, `render`/`eval_js`/`inject_js`, cookies/headers,
  `snapshot`. Available as both a Node server and a native Rust binary.

Engine: a Rust workspace (core / page / view / render / transform / napi / mcp) on
the `turbo-dom` crate, exposed to Node through a napi addon and a Playwright shim.
Performance is network-bound — a pooled HTTP client, a persistent V8 isolate
reused across pages, an external-script cache, and a per-page parse cache. In
benchmarks it runs the same routines as Chromium at parity while being multiples
faster, and outpaces other crawlers (Cheerio/Scrapy/Colly and browser-driving
crawlers) — see the README.
