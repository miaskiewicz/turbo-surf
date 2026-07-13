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
faithfully emulated **fails honestly** rather than silently passing.

## Why some things can't work

The engine has a real DOM (rtdom) and a real V8 (the render tier), but **no
rendering surface and no input hardware**. So three buckets are out of reach:

- **Pixels / layout / rendering** — PDF, video, bounding boxes,
  `toHaveScreenshot`/`toMatchSnapshot`, viewport-pixel scrolling. There is no
  browser raster output to capture or compare. **Exception:** `page.screenshot()`
  is supported via a *synthetic* native layout+paint (see below) — a
  representative render, not a browser-faithful capture. It fetches the page's
  external `<link>` stylesheets and cascades them (`{externalCss:false}` opts
  out), fetches + paints `<img>`/`background-image` bytes (PNG/JPEG/GIF/WebP +
  SVG; `{images:false}` opts out, unknown formats fall back to a placeholder),
  models
  `position:absolute/fixed/relative` + `z-index` for block flow (painted
  back-to-front per CSS stacking order), propagates the root/body background, and
  supports `{fullPage:true}`. It does **not** run JS-driven visuals (a `<canvas>`
  gradient background is absent) or crop to an element — `locator.screenshot()`
  still throws.
- **Synthetic input devices** — `mouse`, `keyboard`, `touchscreen`, `hover`,
  `dragTo`. There is no pointer/keyboard hardware to drive; the engine acts on the
  DOM directly (`click` resolves a link/submit intent; `fill` sets the value).
- **In-process network interception** — `route`/`unroute`/`routeFromHAR`. The
  engine fetches over Rust+reqwest; there's no in-process request bus to intercept.
  (`setExtraHTTPHeaders` and cookies ARE wired through — those are real.)

A second, subtler bucket is **truly-async, time-dependent browser behavior**. The
DOM is static between actions (no background event loop firing timers/XHR after an
action settles), so `waitFor*` resolve against current state instead of polling a
live browser. For server-rendered and hydration-on-navigation apps this is exactly
right; for an app that mutates the DOM seconds after load with no further action,
it differs from a real browser.

## Page

| Method | Status | Notes |
|---|---|---|
| `goto`, `reload`, `goBack`, `goForward` | ✅ | Real fetch (Rust+reqwest); history stack; relative URLs resolve against context `baseURL`. |
| `url`, `content`, `title`, `innerText`, `innerHTML`, `textContent` | ✅ | Over the cached DOM. |
| `getAttribute`, `inputValue`, `isVisible/Hidden/Checked/Enabled/Disabled/Editable` | ✅ | Selector shortcuts → Locator. |
| `locator`, `getByRole`, `getByText`, `getByLabel`, `getByTestId`, `getByPlaceholder`, `getByAltText`, `getByTitle` | ✅ | `getByTestId` honors the context `testIdAttribute`. |
| `click`, `dblclick`, `tap`, `fill`, `type`, `press`, `check`, `uncheck`, `setChecked`, `selectOption` | ✅ | DOM-level. `click` follows the no-JS intent (navigate `<a>` / submit `<form>`); `press("Enter")` submits. |
| `evaluate`, `evaluateHandle` | ✅ | String and `(fn, arg)` forms; runs in the V8 render tier over rtdom. |
| `render`, `addScriptTag({content})` | ✅ | Runs page JS and re-renders the DOM (hydration). |
| `addInitScript` | ⚠️ | Stored, and run once over the current DOM (no pre-navigation injection pipeline). |
| `setExtraHTTPHeaders` | ✅ | **Real** — headers are sent on subsequent fetches. |
| `setContent`, `storageState`, `addCookies`, `context`, `request` | ✅ | |
| `waitForLoadState`, `waitForTimeout`, `waitForURL`, `waitForSelector`, `waitForFunction`, `waitForResponse`, `waitForRequest`, `waitForNavigation`, `waitForEvent` | ⚠️ | Resolve against current state (static DOM) — see "truly-async" above. `waitForSelector`/`waitForURL` assert presence/match rather than poll. |
| `on`, `once`, `off`, `addListener`, `removeListener` | ⚠️ | Registry; `load`/`domcontentloaded`/`close` fire. Live `console`/`request`/`response`/`dialog` events don't (no browser event bus). |
| `mainFrame`, `frame`, `frames`, `frameLocator` | ⚠️ | Collapse to the page itself — no real cross-origin frames. |
| `viewportSize`, `setViewportSize` | ✅ | Stored, and the viewport now drives `page.screenshot()` layout width/height. |
| `setDefaultTimeout`, `setDefaultNavigationTimeout`, `emulateMedia`, `bringToFront` | ⚠️ | Stored / no-op (no live layout to affect). |
| `isClosed`, `close`, `video` (→ null), `workers` (→ []) | ✅ | |
| `screenshot` | ⚠️ | Synthetic native render (PNG, or SVG via `{type:"svg"}`) — representative, not browser-faithful. `path` is written; `fullPage`/`clip` ignored (image is the viewport). |
| `pdf` | ❌ | Throws — no rendering surface. |
| `hover`, `dragAndDrop`, `mouse.*`, `keyboard.*`, `touchscreen.*` | ❌ | Throws — no input hardware. |
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
