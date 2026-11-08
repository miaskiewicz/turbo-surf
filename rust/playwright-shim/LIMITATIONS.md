# Playwright shim — coverage & limitations

`rust/playwright-shim/` is a **drop-in `@playwright/test` replacement** backed by
the turbo-surf native engine (turbo-dom + the napi addon + the V8 render tier) —
**no browser, no Chromium**. A suite that imports `@playwright/test` runs on it
unchanged via the [`register.mjs`](./register.mjs) module-resolution redirect:

```sh
node --import ./rust/playwright-shim/register.mjs --test 'e2e/**/*.spec.mjs'
```

It implements the **Page**, **Locator**, **expect** (all five assertion classes),
**BrowserContext**, **APIRequestContext**, and **test/fixtures** surfaces. This file
is the authoritative map of what is **supported**, what is a **no-op**, and what
**throws** (a no-browser engine physically can't do it). Anything that can't be
faithfully emulated is described below; some compatibility surfaces are stored values
or no-ops rather than complete browser implementations.

## Why some things can't work

The engine has a real DOM (rtdom), V8, native layout/paint and an optional Metal
WebGL bridge, but **no browser compositor or input hardware**. Some browser
surfaces remain unavailable or approximate:

- **Pixels / layout / rendering** — PDF, video, bounding boxes,
  `toHaveScreenshot`/`toMatchSnapshot`, viewport-pixel scrolling. There is no
  browser raster output to capture or compare. **Exception:** `page.screenshot()`
  is supported via a *synthetic* native layout+paint (see below) — a
  representative render, not a browser-faithful capture. It fetches the page's
  external `<link>` stylesheets and cascades them (`{externalCss:false}` opts
  out), fetches + paints `<img>`/`background-image` bytes (PNG/JPEG/GIF/WebP/BMP/ICO/
  AVIF + SVG; `{images:false}` opts out, unknown formats fall back to a placeholder),
  models
  `position:absolute/fixed/relative` + `z-index` for block flow (painted
  back-to-front per CSS stacking order), propagates the root/body background, and
  supports `{fullPage:true}`. It does **not** run JS-driven visuals (a `<canvas>`
  gradient background is absent) or crop to an element — `locator.screenshot()`
  still throws.
- **Input devices** — `mouse`, `touchscreen`, `hover`, `dragTo`, `dragAndDrop`
  lack hardware-backed behavior. In a live session the shim dispatches synthetic
  pointer / mouse / touch / HTML5-drag events at the matched element (or
  `elementFromPoint(x, y)` for the coord-based `mouse` / `touchscreen` entries),
  so JS handlers fire exactly as in a real browser. No compositor means no native
  scroll, no drag image, no hit-testing beyond what native layout provides.
  Without a live session every input call is a clean no-op. `keyboard` dispatches
  key events when a live session exists and is a no-op otherwise; it does not
  provide browser text-editing defaults.
- **In-process network interception** — `page.route(fulfill)` and
  `page.routeFromHAR` are wired via an init-script `fetch`/XHR wrapper (register
  BEFORE the next live session opens). The engine still fetches over Rust+wreq
  (the default impersonate build) or reqwest; there is no shim request-
  interception bus for dynamic `continue`/`abort`/`fallback` or context-scoped
  routes, and HAR record mode is a follow-up.
  (`setExtraHTTPHeaders` and cookies ARE wired through — those are real.)

Live `PageSession` calls retain the app's V8 isolate and drain timers, microtasks and
network work when driven. A `networkidle` navigation opens a session immediately;
actions can hydrate on demand unless navigation used `waitUntil:"commit"`. Static
snapshot paths have no background app event loop. Live assertions can re-drain the
session, but timing and waiting are not the same as a continuously running browser.

The current render tier gives iframes separate V8 contexts, native DOM trees and
constructor prototypes. V8 origin tokens restrict cross-origin access, while complete
WindowProxy/navigation/policy semantics remain incomplete.
Node/Element/HTMLElement/Document and character-data interfaces have separate prototype
chains and projected native accessors validate their receivers. Document-specific
instance properties and compatibility APIs still have partial Web IDL coverage.
Native fragments move their children on insertion. ShadowRoot supports host/mode,
open/closed lookup and scoped queries; slots, retargeting and composed painting remain
incomplete. DOM event methods share EventTarget's prototype while retaining native tree
listeners and independent subclass listener state.
Native namespace-aware element/attribute APIs retain URIs, prefixes and qualified names;
Attr nodes retain live values and ownership through attachment/removal. NamedNodeMap is live
and branded. XML document parsing and serialization remain unsupported.
Event state is private; script dispatch is untrusted and engine events use host dispatch.
Element property handlers retain private state and participate in the native listener registry,
including assignment order and cancellation. Inline handler attributes, event propagation,
listener options and subclass coverage remain incomplete.
Playwright `Frame`/`FrameLocator` objects are still not implemented by this shim.
Classic page/frame scripts run through the Rust host pump with source locations rather
than indirect eval. Parsed hidden inputs expose their token values; input/textarea dirty
values, defaults, cloning and select selection state work in the live runtime. Input-type
sanitization, validation/reset and persistence of live values through an HTML snapshot
remain incomplete. Style text exposes grammar-validated CSSOM rules and live branded
CSSRuleList/StyleSheetList collections with receiver-checked native list operations.
Live hydration retains the authentic HTTP policy headers, response status and
selected navigation referrer. Context/page init scripts run through the trusted
native initializer rather than added HTML tokens. Source header referrer policy
and final redirect request metadata are forwarded before page scripts execute.
Dynamic meta referrer changes and complete browser navigation/resource reporting
lifecycle remain separate coverage work.

Live hydration loads linked/imported stylesheets, enforces inline style CSP against native
attributes, and protects opaque stylesheet rules from script access. Same-origin cross-realm
CSS getters share private interface brands. It sizes PNG/JPEG/GIF/WebP/BMP/ICO/AVIF/JPEG XL and `image/svg+xml` image resources (any `data:`
form) with load/error events like Chrome and selects `<picture>` / `srcset` / `sizes`
sources like Chrome (the binding's selection); CSS image/font resource loading is incomplete. Known gaps: a
DOMException raised by a binding module (e.g. `img.decode()`'s `EncodingError`) is not
`instanceof DOMException` and `structuredClone`s to `{}` (turbo-surf#42), and a parser-created
`is=` element can match `:defined` once a GC has run turbo-dom's storage collection
(turbo-surf#41). CSSOM changes made with
`insertRule` do not alter
serialized HTML and therefore are not automatically painted by snapshot screenshots.

A `BrowserContext` owns request state shared by page navigations, live hydration,
module/fetch/XHR requests and screenshot assets. This preserves per-origin `Accept-CH`
negotiation and HTTP-client state when a live runtime is replaced. Navigation/resource
headers, referrer/Origin and credentials use explicit request contexts. This is not
complete CORS enforcement or browser redirect/permissions-policy behavior.
`page.request()` and `BrowserContext.request()` share the parent context's cookie jar
(reads + `Set-Cookie` writeback, #230) but still don't retain the `Accept-CH` hint state,
and concurrent live resource requests merge cookie writes/deletions rather than replacing
the whole jar. A bare `request.newContext()` (unbound) keeps its empty-jar behavior.
HttpOnly cookies remain available to HTTP and storage state,
while page JS cannot read, create or overwrite them through `document.cookie`.

## Page

| Method | Status | Notes |
|---|---|---|
| `goto`, `reload`, `goBack`, `goForward` | ✅ | Real fetch (Rust+wreq/reqwest); shared context request state; history stack; relative URLs resolve against context `baseURL`. |
| `url`, `content`, `title`, `innerText`, `innerHTML`, `textContent` | ✅ | Over the cached DOM. |
| `getAttribute`, `inputValue`, `isVisible/Hidden/Checked/Enabled/Disabled/Editable` | ✅ | Selector shortcuts → Locator. |
| `locator`, `getByRole`, `getByText`, `getByLabel`, `getByTestId`, `getByPlaceholder`, `getByAltText`, `getByTitle` | ✅ | `getByTestId` honors the context `testIdAttribute`. |
| `click`, `dblclick`, `tap`, `fill`, `type`, `press`, `check`, `uncheck`, `setChecked`, `selectOption` | ✅ | DOM-level events in live sessions; static paths resolve navigation/form intents. Not hardware input. |
| `evaluate`, `evaluateHandle` | ⚠️ | String and `(fn, arg)` forms run in V8 over rtdom. The current bridge returns string values; typed result/handle serialization is incomplete. |
| `render`, `addScriptTag({content})` | ✅ | Runs page JS and re-renders the DOM (hydration). |
| `addInitScript` | ✅ | Stored on the context and installed before authored scripts in every frame of the live session — the top page plus every child frame (static `<iframe src>`, srcdoc, nested, dynamically created, blob URL). Page-level calls also render over the current HTML. |
| `setExtraHTTPHeaders` | ✅ | **Real** — extra headers are sent on page navigation and linked-CSS fetches; not a global override of every live fetch/XHR request. |
| `setContent`, `storageState`, `addCookies`, `context`, `request` | ✅ | |
| `waitForLoadState`, `waitForTimeout`, `waitForURL`, `waitForSelector`, `waitForFunction`, `waitForResponse`, `waitForRequest`, `waitForNavigation`, `waitForEvent` | ⚠️ | `waitForLoadState('load'\|'domcontentloaded'\|'networkidle')` all hydrate the SPA (one "settled" tier). `waitForResponse` / `waitForRequest` short-circuit on the native-log drain (#218/#222). `waitForNavigation` polls for a URL change. `waitForEvent` handles `'response'/'request'/'requestfinished'/'download'`. `waitForSelector` checks the current snapshot. `waitForURL` and `waitForFunction` drain/poll a live session. No browser event loop. |
| `on`, `once`, `off`, `addListener`, `removeListener` | ⚠️ | Registry; lifecycle events fire, and captured live fetch/XHR request/response events are emitted when drained. No complete browser network/console/dialog event bus. |
| `mainFrame`, `frame`, `frames`, `frameLocator` | ⚠️ | Return the page or a top-level locator. Engine iframe contexts exist, but these APIs do not expose them. |
| `viewportSize`, `setViewportSize` | ✅ | Stored, and the viewport now drives `page.screenshot()` layout width/height. |
| `setDefaultTimeout`, `setDefaultNavigationTimeout`, `emulateMedia`, `bringToFront` | ⚠️ | Stored / no-op (no live layout to affect). |
| `isClosed`, `close`, `video` (→ null), `workers` (→ []) | ✅ | |
| `screenshot` | ⚠️ | Synthetic native render (PNG, or SVG via `{type:"svg"}`) — representative, not browser-faithful. `path` and `fullPage` work; `clip` is ignored. |
| `pdf` | ❌ | Throws — no PDF implementation in the shim. |
| `hover` | ✅ | Live app only: dispatches mouseenter/over into the isolate so JS hover handlers (menu-open) fire. Static snapshot: no-op. Not hardware input. |
| `dragAndDrop`, `mouse.*`, `touchscreen.*` | ✅ | Live app only: dispatches pointer / mouse / touch / HTML5-drag events into the isolate at element centers (or at `(x, y)` for coord-based entries). Static snapshot: no-op. Not hardware input. |
| `keyboard.*` | ⚠️ | Key-event dispatch in a live session; no-op without one, incomplete text-editing defaults. |
| `route`, `unroute` | ⚠️ | Shim-side only: the handler runs once at register-time against a stub route; only `route.fulfill(...)` is captured (`continue` / `abort` / `fallback` are no-ops, no per-request dynamic responses). The fulfilled fixture is injected into the live isolate via an init script that monkey-patches `fetch` and `XMLHttpRequest` — call `page.route(...)` BEFORE the next live session opens. |
| `routeFromHAR` | ⚠️ | Playback-only. Each `log.entries[].request.url` is registered as a shim-side route returning that entry's response body / status / content-type via the init-script `fetch`/XHR wrapper — call BEFORE the next live session opens. `options.url` filters entries (RegExp or substring). Record mode and strict `notFound: 'abort'` are not implemented. |
| `exposeBinding`, `exposeFunction` | ❌ | Throws — no persistent JS↔host binding across renders. |
| `pause`, `addLocatorHandler`, `pickLocator` | ❌ | Throws / n/a — no inspector UI. |
| `clock`, `coverage`, `tracing`, `requestGC` | ❌ | n/a — no devtools protocol. |

## Locator

| Method | Status | Notes |
|---|---|---|
| `first`, `last`, `nth`, `filter`, `and`, `or`, `all`, `count` | ✅ | Pure JS over the resolved match set. |
| `locator` (nesting) | ⚠️ | CSS-concat when the parent is selector-backed (the common case). A `getBy*`-rooted parent resolves document-wide; scope it with a CSS parent or `filter`. |
| `getByRole/Text/Label/TestId/Placeholder/AltText/Title` | ✅ | Delegate to the page (document-scoped). |
| `textContent`, `innerText`, `innerHTML`, `allTextContents`, `allInnerTexts` | ✅ | |
| `getAttribute`, `inputValue`, `selectedValues`, `cssValue`, `ariaRole`, `accessibleName`, `accessibleDescription` | ✅ | |
| `is*` (`Visible/Hidden/Checked/Enabled/Disabled/Editable/Empty`) | ✅ | |
| `fill`, `clear`, `type`, `pressSequentially`, `check`, `uncheck`, `setChecked`, `selectOption`, `click`, `dblclick`, `tap`, `press` | ✅ | DOM-level (same intent model as Page). |
| `evaluate` | ⚠️ | Needs a CSS-selector-backed locator (element passed by `document.querySelector`). |
| `ariaSnapshot` | ✅ | |
| `dispatchEvent` | ✅ | Live app only: fires the named DOM event into the isolate (e.g. a row's `click`). Static snapshot: no-op. |
| `hover` | ✅ | Live app only: dispatches mouseenter/over into the isolate. Static snapshot: no-op. Not hardware input. |
| `focus`, `blur` | ✅ | Live app only: calls `el.focus()` / `el.blur()` in the isolate so `document.activeElement` updates (and the app's focus/blur handlers fire). Static snapshot: no-op. A page made via `setContent(...)` stays static until a goto with `waitUntil:'networkidle'` or a `waitForLoadState` / explicit `_openLiveSession` opens the isolate — `.focus()` + `toBeFocused` on a pure `setContent` page will not work. |
| `scrollIntoViewIfNeeded`, `highlight`, `waitFor` | ⚠️ | No-op / resolve (no viewport, no layout — nothing to scroll into). |
| `boundingBox` | ⚠️ | Returns `null` (there genuinely is no browser-faithful box). Geometry-conditional helpers (smooth-scroll, visibility-by-rect) no-op cleanly instead of throwing. |
| `screenshot` | ❌ | Throws — pixels. |
| `dragTo` | ✅ | Live app only: dispatches a full pointer+mouse+HTML5-drag sequence on source → target. Static snapshot: no-op. Not hardware input. |
| `selectText` | ❌ | Throws — input hardware. |

## expect (5 assertion classes)

| Class | Status | Notes |
|---|---|---|
| **LocatorAssertions** | ✅ | `toBeVisible/Hidden/Attached/Checked/Enabled/Disabled/Editable/Empty/InViewport`, `toHaveText/ContainText/Count/Value/Values/Attribute/Class/ContainClass/Id/Role/AccessibleName/AccessibleDescription/JSProperty/CSS`, `toMatchAriaSnapshot`. RegExp + `.not` supported. The common chain (`toBeVisible` + `toHaveText` + …) is batched into **one** napi crossing via `node_snapshot`. |
| **PageAssertions** | ✅ | `toHaveURL`, `toHaveTitle` (string / RegExp / `.not`). |
| **GenericAssertions** | ✅ | jest-shaped: `toBe/toEqual/toStrictEqual/toContain/toContainEqual/toMatch/toMatchObject/toBeNull/Undefined/Defined/Truthy/Falsy/NaN/toBeGreaterThan(OrEqual)/toBeLessThan(OrEqual)/toBeCloseTo/toBeInstanceOf/toHaveLength/toHaveProperty/toThrow(Error)`, the `toHaveBeenCalled*` mock matchers, and `.resolves`/`.rejects`. |
| **APIResponseAssertions** | ✅ | `toBeOK` (via the response's `ok()`). |
| `toBeFocused` | ⚠️ | Live app only: probes `document.activeElement` in the isolate. Static snapshot: treated as not-focused. |
| `toHaveScreenshot`, `toMatchSnapshot` (**SnapshotAssertions**) | ❌ | Throws — pixels. |

## BrowserContext

| Method | Status | Notes |
|---|---|---|
| `newPage`, `pages`, `cookies`, `addCookies`, `clearCookies`, `storageState`, `addInitScript`, `setExtraHTTPHeaders`, `browser`, `request`, `close` | ✅ | |
| `grantPermissions`, `clearPermissions`, `setGeolocation`, `setOffline`, `setDefaultTimeout`, `on/off/once` | ⚠️ | No-op / registry (no browser to permission or geo-locate). |
| `route`, `unroute`, `exposeBinding`, `exposeFunction`, `newCDPSession`, `tracing` | ❌ | Throws / n/a. Page-scoped `page.route(fulfill)` and `page.routeFromHAR` work — see the Page table. |

## test runner & fixtures

Runs on `node:test`. `test`, `test.describe(.skip/.only)`, `test.skip/only/fixme`,
`test.step`, `test.beforeAll/afterAll/beforeEach/afterEach`, `test.extend`
(custom fixtures), the built-in `{ page, context, browser, request, baseURL }`
fixtures, and a `testInfo` (`outputPath`, `project`, `attach`, …) are supported.
`defineConfig` is identity and `devices[...]` returns an empty descriptor.

### `playwright.config` + CLI entry

`bin/turbo-surf-test.mjs` (also installed as the `turbo-surf-test` bin by the
npm launcher) reads a `playwright.config.{ts,mts,js,mjs,cjs}` and runs each
project as its own `node --import register.mjs --test` sweep:

```sh
npx turbo-surf-test               # auto-detects playwright.config.*
npx turbo-surf-test --config pw.config.mjs --project chrome --reporter list
```

- **`projects`** — each one runs serially; `project.use` is merged over the
  top-level `use` and forwarded via `TURBO_PW_USE` + the existing
  `TURBO_SHIM_*` env (so `baseURL`/`locale`/`userAgent`/`testIdAttribute`
  land in `BrowserContext` and the `baseURL` fixture). `testInfo.project`
  reports the project name + merged `use`.
- **`reporter`** — `list`/`spec`/`tap`/`dot` map to node:test's built-ins;
  `html` captures TAP and writes a minimal `playwright-report.html` (override
  path with `[['html', { outputFile: … }]]`).
- **`webServer`** — spawn, poll `url` (or `http://127.0.0.1:${port}`) until it
  responds, tear down at the end. Arrays are supported;
  `reuseExistingServer: true` skips the spawn if a server is already answering.
- **`testDir` / `testMatch` / `testIgnore`** — resolved per project; the default
  match is `**/*.@(spec|test).@(ts|mts|js|mjs|cjs)` and the default ignore is
  `**/node_modules/**`.

`.ts` configs/specs need a Node that strips types inline (Node 23+, or Node 22
with `--experimental-strip-types`); the shim doesn't ship its own TS loader.
Browser-only reporters (`junit`/the real Playwright HTML trace viewer),
sharding, retries, and `testConfig.webServer.stdout` capture are not
implemented.

## Optional future perf lever (documented, not built)

The napi seam is string-in/string-out and the addon caches the last parse per
thread, so a crossing on an unchanged page is a marshal + op, not a re-parse.
`node_snapshot(html, node)` already batches the boolean/text/role accessor reads
into **one** crossing so an `expect(locator)` chain doesn't cross 3×. A further
lever — **not built**, because e2e is network-bound — would be a
`node_snapshot`-style batch for the attribute/class/CSS matchers (which still
cross per-name, as there's no attribute-map iterator on the seam).
