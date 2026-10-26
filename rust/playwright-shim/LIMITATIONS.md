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
- **Input devices** — `mouse`, `touchscreen`, `hover`, `dragTo` lack hardware-backed
  behavior. DOM actions dispatch events in a live session or use a static navigation/
  submit intent. `keyboard` dispatches key events when a live session exists; it is
  a no-op without one and does not provide browser text-editing defaults.
- **In-process network interception** — `route`/`unroute`/`routeFromHAR`. The
  engine fetches over Rust+wreq (the default impersonate build) or reqwest; there
  is no shim request-interception bus.
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
form) with load/error events like Chrome and selects `srcset` candidates like Chrome;
`<picture>` sources and CSS image/font resource loading are incomplete. CSSOM changes made with
`insertRule` do not alter
serialized HTML and therefore are not automatically painted by snapshot screenshots.

A `BrowserContext` owns request state shared by page navigations, live hydration,
module/fetch/XHR requests and screenshot assets. This preserves per-origin `Accept-CH`
negotiation and HTTP-client state when a live runtime is replaced. Navigation/resource
headers, referrer/Origin and credentials use explicit request contexts. This is not
complete CORS enforcement or browser redirect/permissions-policy behavior.
`APIRequestContext` calls, including the page/context request facade, currently do
not retain this browser-context hint state or share its cookie jar. Concurrent live
resource requests merge cookie writes/deletions rather than replacing the whole jar.
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
| `addInitScript` | ⚠️ | Stored for injection before page scripts when a live session opens; page-level calls also render it over the current HTML. Not complete child-frame init-script coverage. |
| `setExtraHTTPHeaders` | ✅ | **Real** — extra headers are sent on page navigation and linked-CSS fetches; not a global override of every live fetch/XHR request. |
| `setContent`, `storageState`, `addCookies`, `context`, `request` | ✅ | |
| `waitForLoadState`, `waitForTimeout`, `waitForURL`, `waitForSelector`, `waitForFunction`, `waitForResponse`, `waitForRequest`, `waitForNavigation`, `waitForEvent` | ⚠️ | `waitForSelector` checks the current snapshot; `waitForURL` and `waitForFunction` can drain/poll a live session. Other waiters have limited compatibility behavior; no browser event loop. |
| `on`, `once`, `off`, `addListener`, `removeListener` | ⚠️ | Registry; lifecycle events fire, and captured live fetch/XHR request/response events are emitted when drained. No complete browser network/console/dialog event bus. |
| `mainFrame`, `frame`, `frames`, `frameLocator` | ⚠️ | Return the page or a top-level locator. Engine iframe contexts exist, but these APIs do not expose them. |
| `viewportSize`, `setViewportSize` | ✅ | Stored, and the viewport now drives `page.screenshot()` layout width/height. |
| `setDefaultTimeout`, `setDefaultNavigationTimeout`, `emulateMedia`, `bringToFront` | ⚠️ | Stored / no-op (no live layout to affect). |
| `isClosed`, `close`, `video` (→ null), `workers` (→ []) | ✅ | |
| `screenshot` | ⚠️ | Synthetic native render (PNG, or SVG via `{type:"svg"}`) — representative, not browser-faithful. `path` and `fullPage` work; `clip` is ignored. |
| `pdf` | ❌ | Throws — no PDF implementation in the shim. |
| `hover`, `dragAndDrop`, `mouse.*`, `touchscreen.*` | ❌ | Throws — no input hardware. |
| `keyboard.*` | ⚠️ | Key-event dispatch in a live session; no-op without one, incomplete text-editing defaults. |
| `route`, `routeFromHAR`, `unroute` | ❌ | Throws — no in-process interception. |
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
| `focus`, `blur`, `dispatchEvent`, `scrollIntoViewIfNeeded`, `highlight`, `waitFor` | ⚠️ | No-op / resolve (no focus model, no layout, static DOM). |
| `screenshot`, `boundingBox` | ❌ | Throws — pixels. |
| `hover`, `dragTo`, `selectText` | ❌ | Throws — input hardware. |

## expect (5 assertion classes)

| Class | Status | Notes |
|---|---|---|
| **LocatorAssertions** | ✅ | `toBeVisible/Hidden/Attached/Checked/Enabled/Disabled/Editable/Empty/InViewport`, `toHaveText/ContainText/Count/Value/Values/Attribute/Class/ContainClass/Id/Role/AccessibleName/AccessibleDescription/JSProperty/CSS`, `toMatchAriaSnapshot`. RegExp + `.not` supported. The common chain (`toBeVisible` + `toHaveText` + …) is batched into **one** napi crossing via `node_snapshot`. |
| **PageAssertions** | ✅ | `toHaveURL`, `toHaveTitle` (string / RegExp / `.not`). |
| **GenericAssertions** | ✅ | jest-shaped: `toBe/toEqual/toStrictEqual/toContain/toContainEqual/toMatch/toMatchObject/toBeNull/Undefined/Defined/Truthy/Falsy/NaN/toBeGreaterThan(OrEqual)/toBeLessThan(OrEqual)/toBeCloseTo/toBeInstanceOf/toHaveLength/toHaveProperty/toThrow(Error)`, the `toHaveBeenCalled*` mock matchers, and `.resolves`/`.rejects`. |
| **APIResponseAssertions** | ✅ | `toBeOK` (via the response's `ok()`). |
| `toBeFocused` | ⚠️ | No focus model on a static DOM — treated as not-focused. |
| `toHaveScreenshot`, `toMatchSnapshot` (**SnapshotAssertions**) | ❌ | Throws — pixels. |

## BrowserContext

| Method | Status | Notes |
|---|---|---|
| `newPage`, `pages`, `cookies`, `addCookies`, `clearCookies`, `storageState`, `addInitScript`, `setExtraHTTPHeaders`, `browser`, `request`, `close` | ✅ | |
| `grantPermissions`, `clearPermissions`, `setGeolocation`, `setOffline`, `setDefaultTimeout`, `on/off/once` | ⚠️ | No-op / registry (no browser to permission or geo-locate). |
| `route`, `routeFromHAR`, `unroute`, `exposeBinding`, `exposeFunction`, `newCDPSession`, `tracing` | ❌ | Throws / n/a. |

## test runner & fixtures

Runs on `node:test`. `test`, `test.describe(.skip/.only)`, `test.skip/only/fixme`,
`test.step`, `test.beforeAll/afterAll/beforeEach/afterEach`, `test.extend`
(custom fixtures), the built-in `{ page, context, browser, request, baseURL }`
fixtures, and a `testInfo` (`outputPath`, `project`, `attach`, …) are supported.
`defineConfig` is identity and `devices[...]` returns an empty descriptor — the
Playwright CLI/`playwright.config` projects/reporters/`webServer` are **not**
interpreted (you run specs with `node --test`, not `playwright test`).

## Optional future perf lever (documented, not built)

The napi seam is string-in/string-out and the addon caches the last parse per
thread, so a crossing on an unchanged page is a marshal + op, not a re-parse.
`node_snapshot(html, node)` already batches the boolean/text/role accessor reads
into **one** crossing so an `expect(locator)` chain doesn't cross 3×. A further
lever — **not built**, because e2e is network-bound — would be a
`node_snapshot`-style batch for the attribute/class/CSS matchers (which still
cross per-name, as there's no attribute-map iterator on the seam).
