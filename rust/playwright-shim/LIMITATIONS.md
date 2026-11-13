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

This describes the current v0.8.3 release candidate source, not a published release or
a claim that its final release gates have passed. The default renderer is Rust/V8;
Chrome/Playwright are development oracles. An explicitly selected browser solver is
a separate optional path. See [build features](../../docs/BUILD-FEATURES.md).

Native auxiliary targets use actual native `domcontentloaded` and `load` milestones.
For these targets, `networkidle` navigation and `waitForLoadState('networkidle')`
currently reject explicitly: target-scoped pending-navigation and resource
quiescence evidence is required. Ordinary legacy-root hydration retains its
existing `networkidle` behavior.

## Response-size budget

The default response-body limit remains 8 MiB. For development applications with
larger JavaScript bundles, pass `maxResponseBytes: 16 * 1024 * 1024` to
`browser.newContext()` (or `newPage()`) and to standalone `request.newContext()`.
The byte cap applies to navigation responses, page resource fetches and module
graphs; it stays finite and also accepts a smaller positive limit. Native resource
size failures retain their `BodyTooLarge`/`maxBytes` diagnostic on stderr.

## Why some things can't work

The engine has a real DOM (rtdom), V8, native layout/paint and an optional Metal
WebGL bridge, but **no browser compositor or input hardware**. Some browser
surfaces remain unavailable or approximate:

- **Pixels / layout / rendering** — PDF, video,
  `toHaveScreenshot`/`toMatchSnapshot`, browser compositor capture. There is no
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
  so app handlers receive events. No compositor means no drag image; scrolling
  and hit-testing use native layout and its admission limits.
  Without a live session every input call is a clean no-op. `keyboard` dispatches
  key events when a live session exists and is a no-op otherwise; it provides
  a subset of text-editing defaults, not full browser editing behavior. Enter
  and printable keypress events expose legacy character codes used by React
  `onKeyPress`; canceling keydown suppresses keypress, and canceling keypress
  suppresses the editing default while preserving keyup. Control/Meta command
  chords omit keypress and preserve the select-all editing default.
- **Network interception** — page-scoped fetch/XHR routes run host callbacks,
  including fulfill, continue/fallback overrides, abort and `route.fetch()`.
  Registration updates current and future sessions. General document/resource
  interception and context routes remain unsupported; HAR is playback-only.
  See the method rows below for the narrower replay semantics.

Live `PageSession` calls retain the app's V8 isolate and drain timers, microtasks and
network work when driven. Shim sessions use wall-clock timer deadlines, so driving
an action does not fast-forward future debounce or editor-expiry callbacks. The
native addon retains virtual timers when callers omit its opt-in. A `networkidle` navigation opens a session immediately;
actions, evaluation, readiness waits and web-first assertions can hydrate on demand.
`waitUntil:"commit"` returns the raw destination response without opening a live
session, even with passive observers; it does not disable later JavaScript. Raw
content inspection stays static until an operation requests execution. Static
snapshot paths have no background app event loop. Live assertions can re-drain the
session, but timing and waiting are not the same as a continuously running browser.

The binding surface is maintained in the pinned
[turbo-dom-browser-env documentation](https://github.com/miaskiewicz/turbo-dom/blob/ce3d0d0b0f9a2c7a08e8a9fcf899085682186fd6/crates/turbo-dom-browser-env/README.md),
with Chrome controls in that crate. It includes native DOM interfaces, namespace
and Attr operations, shadow DOM, events, forms, XML `DOMParser`/CDATA/
`XMLSerializer`, and bounded internal-DTD attribute/entity processing. Supported
interfaces are not promises of complete browser behavior. Surf-specific examples:

- [Binding controls](../crates/turbo-surf-render/tests/binding_dom_controls.rs)
  cover shadow-tree event paths, adoption reactions and realm DOMException
  identity; [DOMException](../crates/turbo-surf-render/tests/dom_exception_identity.rs)
  and [custom-element state](../crates/turbo-surf-render/tests/custom_elements_defined.rs)
  controls cover the former #42/#41 regressions.
- [XML response controls](../crates/turbo-surf-render/tests/xhr_document.rs)
  cover `responseXML`/document responses and malformed/encoded XML;
  [live geometry contracts](../crates/turbo-surf-render/tests/live_geometry_contracts.rs)
  cover native XML factories and empty geometry for inert documents.
- [Form methods](../crates/turbo-surf-render/tests/form_methods.rs) and
  [host input](../crates/turbo-surf-render/tests/host_input.rs) cover validation,
  requestSubmit, live values and cancellation. Live property state should be
  read in the owning realm; an HTML snapshot cannot represent every DOM property.
- [Frame controls](./surface.test.mjs) cover srcdoc and fetched/nested frame
  identity, detach events, scoped locators and frame mutations. Frames share the
  page session while using their own realm/document; live frame actions and
  evaluation refresh the session state. Snapshot reads reflect the last drain.
- [CSP metadata](../crates/turbo-surf-render/tests/csp_navigation_metadata.rs)
  and [referrer policy](../crates/turbo-surf-render/tests/referrer_policy.rs)
  controls cover trusted initialization and request policy forwarding.
- [Font loading](../crates/turbo-surf-render/tests/font_face.rs) covers
  `@font-face`, WOFF/WOFF2, CSP/CORS, native font registration and `document.fonts`
  timing. Live font/layout behavior is distinct from snapshot screenshot capture.

Classic page/frame scripts run through the Rust host pump with source locations.
Live hydration loads linked/imported stylesheets, enforces inline style CSP and
protects opaque stylesheet rules from script access. Image resources include
PNG/JPEG/GIF/WebP/BMP/ICO/AVIF/JPEG XL and SVG, with picture/srcset/sizes selection.
CSSOM `insertRule` changes do not alter serialized HTML and therefore are not
automatically painted by snapshot screenshots. Engine-specific frame and policy
bounds belong in the [parity gaps](../../harness/detection/PARITY-GAPS.md) and
[hydration guide](../HEADLESS-HYDRATION.md), rather than an obsolete missing-API list.

A `BrowserContext` owns request state shared by page navigations, live hydration,
module/fetch/XHR requests and screenshot assets. This preserves per-origin `Accept-CH`
negotiation and HTTP-client state when a live runtime is replaced. Navigation/resource
headers, referrer/Origin and credentials use explicit request contexts. Policy behavior is scoped to the native request services and the documented
[parity gaps](../../harness/detection/PARITY-GAPS.md).
Captured requests retain authored/context headers and buffered POST bytes. Native
navigation tracing emits each redirect-hop request; `isNavigationRequest()` distinguishes
document navigations from subresources, and `frame()` uses captured native frame identity
and document paths rather than URL matching. Delivery still follows the native log drain.
Redirect response events preserve native per-hop status and headers; intermediate bodies
are unavailable and reject body reads. Captured fetch responses publish admitted headers
independently of body completion, including while the isolate awaits the body. Response
body/text/JSON and finished promises wait for real transport completion; body failures
reject and emit requestfailed, while requestfinished follows successful completion.
Closing the session cancels owned transport and rejects pending body promises. The host
event queue is bounded and reports overflow instead of leaving body promises unresolved.
Streamed upload bodies are not retained by the request snapshot.
`page.request()` and `BrowserContext.request()` share the parent context's cookie jar
(reads + `Set-Cookie` writeback, #230) and native transport identity, while browser
`setOffline` does not affect API request contexts. Concurrent live resource requests
merge cookie writes/deletions rather than replacing
the whole jar. A bare `request.newContext()` (unbound) keeps its empty-jar behavior.
HttpOnly cookies remain available to HTTP and storage state,
while page JS cannot read, create or overwrite them through `document.cookie`.

## Native geometry limits

Geometry admission is scoped to the source and purpose of a read. Unaffected
boxes and leaf scroll extents can be read despite unrelated animations. An
animated query or ancestor, animated descendants contributing to an extent,
nonzero scrolling and uncertain scrollbar gutters retain strict checks. Actual
point hit-testing also stays strict. Fresh canonical per-element lifecycle proof
can admit underlying geometry after matching finite CSS animations complete
without fill; it does not advance the timeline or sample active transforms.
Playback changes revoke the proof even without DOM mutation. Missing or
mismatched lifecycle data, including scoped shadow stylesheet effects tracked in
[turbo-dom #509](https://github.com/miaskiewicz/turbo-dom/issues/509), stays strict.
An animation-toggle option is future work ([#500](https://github.com/miaskiewicz/turbo-surf/issues/500), v0.8.4).

## Page

| Method | Status | Notes |
|---|---|---|
| `goto`, `reload`, `goBack`, `goForward` | ✅ | Real fetch (Rust+wreq/reqwest); shared context request state; history stack; relative URLs resolve against context `baseURL`. |
| `url`, `content`, `title`, `innerText`, `innerHTML`, `textContent` | ✅ | Over the cached DOM. |
| `getAttribute`, `inputValue`, `isVisible/Hidden/Checked/Enabled/Disabled/Editable` | ✅ | Selector shortcuts → Locator. |
| `locator`, `getByRole`, `getByText`, `getByLabel`, `getByTestId`, `getByPlaceholder`, `getByAltText`, `getByTitle` | ✅ | `getByTestId` honors the context `testIdAttribute`. |
| `click`, `dblclick`, `tap`, `fill`, `type`, `press`, `check`, `uncheck`, `setChecked`, `selectOption` | ✅ | DOM-level events in live sessions; static paths resolve navigation/form intents. Not hardware input. |
| `evaluate`, `evaluateHandle` | ✅ | Typed values and nested handle arguments run in the owning V8 realm. `evaluateHandle` returns a retained JSHandle/ElementHandle with live identity, property handles, evaluation and explicit disposal. Navigation/close/retired frames invalidate their handles. |
| `render` | ✅ | Runs page JS over the snapshot and re-renders the DOM. |
| `addStyleTag`, `addScriptTag` | ⚠️ | Inserts content, URL, or path sources into the live document and awaits external loads. Returns an ElementHandle for the inserted tag. Module script injection rejects because its load-completion lifecycle is unsupported. |
| `addInitScript` | ✅ | Stored on the context and installed before authored scripts in every frame of the live session — the top page plus every child frame (static `<iframe src>`, srcdoc, nested, dynamically created, blob URL). Page-level calls also render over the current HTML. |
| `setExtraHTTPHeaders` | ✅ | **Real** — extra headers are sent on page navigation and linked-CSS fetches; not a global override of every live fetch/XHR request. |
| `setContent` | ⚠️ | Native host replacement preserves the Window, document, prototypes, bindings, and document listeners; parser scripts retain CSP admission and resource/load scheduling. Supports `load`, `domcontentloaded`, and `networkidle` waits with deadlines. Compatibility mode is retained; guest `document.open/write/close` remains unsupported. |
| `storageState`, `addCookies`, `context`, `request` | ✅ | |
| `waitForLoadState`, `waitForTimeout`, `waitForURL`, `waitForSelector`, `waitForFunction`, `waitForResponse`, `waitForRequest`, `waitForNavigation`, `waitForEvent` | ⚠️ | `waitForLoadState('load'\|'domcontentloaded'\|'networkidle')` all hydrate the SPA (one "settled" tier). `waitForResponse` / `waitForRequest` short-circuit on the native-log drain (#218/#222); their timeout precedence is explicit option, page default, context default / configured `actionTimeout`, then 30s (`0` disables the timeout). `waitForNavigation` polls for a URL change. `waitForEvent` handles `'response'/'request'/'requestfinished'/'download'`. `waitForSelector` checks the current snapshot. `waitForURL` and `waitForFunction` drain/poll a live session. No browser event loop. |
| `on`, `once`, `off`, `addListener`, `removeListener` | ⚠️ | Registry; lifecycle events fire, and captured live fetch/XHR request/response events are emitted when drained. Console events expose type, text, source location and live argument handles. Native dialogs remain unsupported. |
| `mainFrame`, `frame`, `frames`, `frameLocator` | ✅ | `mainFrame` returns the host page (the page implements the Frame surface). `frames()` retains nested native frame identity and detach state; `frame(name)` / `frame({name,url})` find a child; `frameLocator('iframe[...]')` scopes the locator chain to the child document. Live hydration loads src/srcdoc frames; snapshot reads reflect the last drain. |
| `viewportSize`, `setViewportSize` | ✅ | Stored, and the viewport now drives `page.screenshot()` layout width/height. |
| `setDefaultTimeout`, `setDefaultNavigationTimeout` | ⚠️ | The general default applies to request/response waits; navigation defaults configure navigation deadlines. Other action deadlines remain partial. |
| `emulateMedia` | ⚠️ | Forwards color scheme and reduced motion into native request/session configuration and re-drains a live session. Full media-type emulation is not implemented. |
| `bringToFront` | ⚠️ | No-op; no browser window. |
| `isClosed`, `close`, `video` (→ null), `workers` (→ []) | ✅ | |
| `screenshot` | ⚠️ | Synthetic native render (PNG/JPEG, or SVG via `{type:"svg"}`) — representative, not browser-faithful. JPEG quality and path inference work; `clip` crops PNG/JPEG output to rendered-image bounds without changing layout. SVG clipping throws. Snapshot-scoped locator masks use native clipped element geometry and CSS mask colors; cross-page/frame masks throw. PNG/SVG omitBackground preserves authored backgrounds; JPEG retains its opaque canvas. Device scale uses native capture with scale css/device and physical-pixel clip rounding; full-page capture includes positioned overflow and retains the viewport height minimum. |
| `pdf` | ❌ | Throws — no PDF implementation in the shim. |
| `hover` | ✅ | Live app only: dispatches mouseenter/over into the isolate so JS hover handlers (menu-open) fire. Static snapshot: no-op. Not hardware input. |
| `dragAndDrop`, `mouse.*`, `touchscreen.*` | Partial | Live app only: dispatches pointer / mouse / touch / HTML5-drag events into the isolate at element centers (or at `(x, y)` for coord-based entries). Touch taps require genuine constructible native `Touch` and populated `TouchEvent` lists; unsupported bindings reject before dispatch instead of losing payloads ([turbo-dom#499](https://github.com/miaskiewicz/turbo-dom/issues/499)). Supported taps honor cancellation before compatibility mouse activation. Static snapshot: no-op. Not hardware input. |
| `keyboard.*` | ⚠️ | Key events plus partial input/contenteditable editing defaults in a live session; incomplete browser editing behavior. |
| `route`, `unroute` | ⚠️ | Page fetch/XHR interception supports glob, RegExp and host URL predicates (including function-identity unroute), and runs actual Node callbacks with closure state, async host work, fulfillment (including response/raw bytes), continue/fallback overrides, abort and `route.fetch()`. Uses the originating realm's native fetch/cookie/policy services; unsupported `route.fetch` options reject explicitly. Registration updates current and future sessions. Fulfilled and rewritten fetch/XHR responses feed native response/body events; internal `route.fetch()` traffic is private, and continued requests retain native observations. General document/resource interception remains unsupported. |
| `routeFromHAR` | ⚠️ | Playback-only. Each `log.entries[].request.url` is registered as a shim-side route returning that entry's response body / status / content-type via the init-script `fetch`/XHR wrapper; registration updates current and future sessions. `options.url` filters entries (RegExp or substring). Record mode and strict `notFound: 'abort'` are not implemented. |
| `exposeBinding`, `exposeFunction` | ⚠️ | Bindings persist across navigations and settle guest promises with serialized host results or errors. Circular/non-JSON host results reject. |
| `pause`, `addLocatorHandler`, `pickLocator` | ❌ | Throws / n/a — no inspector UI. |
| `clock`, `coverage`, `tracing`, `requestGC` | ❌ | n/a — no devtools protocol. |

## Locator

| Method | Status | Notes |
|---|---|---|
| `first`, `last`, `nth`, `filter`, `and`, `or`, `all`, `count` | ✅ | Pure JS over the resolved match set. |
| `page` | ✅ | Returns the owning Page synchronously, including for derived and frame locators; does not resolve the DOM. |
| `locator` (nesting) | ✅ | CSS parents, indexed/filtered ancestors, `:scope`, and role/text/label parents retain their descendant scope for reads and live actions. |
| `getByRole/Text/Label/TestId/Placeholder/AltText/Title` | ⚠️ | Role/text/label/placeholder/alt/title children preserve parent scope, including indexed ancestors. Regex test IDs support matching and live actions. Open-shadow-root piercing remains unsupported (#476). |
| `textContent`, `innerText`, `innerHTML`, `allTextContents`, `allInnerTexts` | ✅ | |
| `getAttribute`, `inputValue`, `selectedValues`, `cssValue`, `ariaRole`, `accessibleName`, `accessibleDescription` | ✅ | |
| `is*` (`Visible/Hidden/Checked/Enabled/Disabled/Editable/Empty`) | ✅ | |
| `fill`, `clear`, `type`, `pressSequentially`, `check`, `uncheck`, `setChecked`, `selectOption`, `click`, `dblclick`, `tap`, `press` | ✅ | DOM-level (same intent model as Page). |
| `evaluate`, `evaluateAll` | ✅ | Resolve the indexed live match or full matched collection in the owning page realm; page effects persist. |
| `ariaSnapshot` | ✅ | |
| `dispatchEvent` | ✅ | Live app only: fires the named DOM event into the isolate (e.g. a row's `click`). Static snapshot: no-op. |
| `hover` | ✅ | Live app only: dispatches mouseenter/over into the isolate. Static snapshot: no-op. Not hardware input. |
| `focus`, `blur` | ✅ | Live app only: calls `el.focus()` / `el.blur()` in the isolate so `document.activeElement` updates (and the app's focus/blur handlers fire). Static snapshot: no-op. `setContent(...)` opens or retains a live session, so focus and blur operate on its current native document. |
| `scrollIntoViewIfNeeded` | ⚠️ | Calls native `scrollIntoView` with nearest/instant behavior; subject to native geometry admission. |
| `highlight` | ⚠️ | No-op; no inspector overlay. |
| `waitFor` | ⚠️ | Opens a live session and polls attached/detached/visible/hidden state with a deadline, re-draining between attempts. |
| `boundingBox` | ⚠️ | Reads live native layout geometry; hidden or boxless elements return null. Unsupported geometry rejects rather than fabricating browser values. |
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
| `setDefaultTimeout` | ⚠️ | Inherited by request/response waits; page and explicit settings take precedence. `0` disables their timeout. Other action deadlines remain partial. |
| `grantPermissions`, `clearPermissions`, `setGeolocation` | ⚠️ | No-op / registry (no browser to permission or geo-locate). |
| `on/off/once`, `waitForEvent` | ⚠️ | Actual owned `newPage`/`close` events and subscribed page network events, with predicates and deadlines. Auxiliary popup contexts remain unsupported (#326); commit-only navigation response events are not collected. |
| `setOffline` | ✅ | Native per-context conditions, also supported by `newContext({ offline: true })`. New transport sends fail after HTTP cache lookup; already-started HTTP responses continue. Owned tabs/frames receive trusted online/offline events and live `navigator.onLine`. API request contexts remain online independently; no native POST replay. Established WebSocket frames wait for reconnect on the same socket. |
| `tracing`, `recordVideo`, `use.trace`, `use.video` | ❌ | Tracing methods and enabled recording options reject with a clear no-browser engine error. Explicit trace/video `off` modes are accepted; `page.video()` returns null without recording. |
| `route`, `unroute`, `routeFromHAR`, `newCDPSession`, `tracing` | ❌ | Throws / n/a. Page-scoped `page.route(fulfill)` works — see the Page table. |
| `exposeBinding`, `exposeFunction` | ⚠️ | Bindings apply to current and future pages across navigations; guest promises receive serialized host results or errors. |

## test runner & fixtures

Runs on `node:test`. `test`, `test.describe(.skip/.only)`, `test.skip/only/fixme`,
`test.step`, `test.beforeAll/afterAll/beforeEach/afterEach`, `test.extend`
(custom fixtures), the built-in `{ page, context, browser, request, baseURL }`
fixtures, and a `testInfo` (`outputPath`, `project`, `attach`, …) are supported.
Test callbacks run in a persistent subprocess per test file. Successful tests
share module state and worker fixtures. Failed attempts restart that process;
`testInfo.retry` identifies the fresh attempt. Config/project `timeout` and
`retries`, `test.describe.configure`, and runtime `test.setTimeout` /
`testInfo.setTimeout` are honored (zero disables the test deadline). Each `beforeAll`
hook and the test start a separate budget; `beforeEach` and the body share the
test budget, including runtime updates.
A deadline terminates the subprocess, including synchronous runaway code, before
any retry starts. A terminated process cannot run remaining JavaScript cleanup;
ordinary failures await fixture and hook cleanup before restarting.
`defineConfig` is identity and `devices[...]` returns an empty descriptor.

### `playwright.config` + CLI entry

`bin/turbo-surf-test.mjs` (also installed as the `turbo-surf-test` bin by the
npm launcher) reads a `playwright.config.{ts,mts,js,mjs,cjs}` and runs each
project as its own `node --import register.mjs --test` sweep:

```sh
npx --yes --package turbo-surf turbo-surf-test
npx --yes --package turbo-surf turbo-surf-test --config pw.config.mjs --project chrome --reporter list
# In a source checkout, use the exact local runner instead:
node rust/playwright-shim/bin/turbo-surf-test.mjs --config pw.config.mjs
```

- **`projects`** — each one runs serially; `project.use` is merged over the
  top-level `use` and forwarded via `TURBO_PW_USE` + the existing
  `TURBO_SHIM_*` env (so `baseURL`/`locale`/`userAgent`/`testIdAttribute`
  land in `BrowserContext` and the `baseURL` fixture). `testInfo.project`
  reports the project name + merged `use`.
- **`workers`** — positive integers or integer percentages control file
  concurrency. The default is 50% of logical CPUs, rounded down with a minimum
  of one; a project limit cannot exceed the global cap. The runner forwards the
  resolved value to Node's `--test-concurrency`. This is a config option, not a
  supported `--workers` CLI argument.
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
sharding and `testConfig.webServer.stdout` capture are not
implemented.

## Optional future perf lever (documented, not built)

The napi seam is string-in/string-out and the addon caches the last parse per
thread, so a crossing on an unchanged page is a marshal + op, not a re-parse.
`node_snapshot(html, node)` already batches the boolean/text/role accessor reads
into **one** crossing so an `expect(locator)` chain doesn't cross 3×. A further
lever — **not built**, because e2e is network-bound — would be a
`node_snapshot`-style batch for the attribute/class/CSS matchers (which still
cross per-name, as there's no attribute-map iterator on the seam).


### CLI runtime capabilities

The `turbo-surf-test` CLI requires Node.js 20.14+ or 22+ for completed-test
worker shutdown (`--test-force-exit`). Configured `grepInvert` requires Node.js
22.1+ (`--test-skip-pattern`). Earlier runtimes receive a clear error before
web servers start; the package's launcher remains available on Node.js 20+.

### Retained handles

`evaluateHandle`, `waitForFunction`, `waitForSelector` and locator `elementHandle` return genuine realm-owned handles. Handle access can open a retained realm after `waitUntil: "commit"`; values are not copied into JSON wrappers. `jsonValue()` serializes the current value, while `getProperty`, `getProperties` and `evaluateHandle` retain new references. Dispose handles when no longer needed. Passing a handle to a different page or frame rejects. Element reads preserve the exact node even after detachment; actions reject detached nodes.

Console argument handles belong to the emitting frame. The native queue retains up to 1000 undelivered messages; eviction releases their argument references. Delivered argument handles remain live until disposed or their execution context is destroyed. Messages drained without a console listener release their references automatically.
