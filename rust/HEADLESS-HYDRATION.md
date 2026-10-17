# Headless hydration — requirements & status (Rust port)

"Headless hydration" in turbo-surf means two distinct things. Both are tracked
here so the requirements are explicit.

## 1. No-JS hydration-state mining (`hydration.mjs`)

Recover SPA data **without executing any JS** by mining the server-shipped state
that frameworks inline:

- `<script id="__NEXT_DATA__">` (Next.js, pure JSON)
- `<script type="application/ld+json">` (JSON-LD, object or array per block)
- `<script type="application/json" id="...">` (Remix / SvelteKit / typed islands)
- `window.__INITIAL_STATE__ / __APOLLO_STATE__ / __PRELOADED_STATE__ / __NUXT__ /
  __remixContext` — `window.X = <json>` assignments parsed **without eval**

**Requirements for the Rust port:** a `hydration` module in `turbo-surf-view`
over `rtdom::Tree` — `query_selector(_all)` for the script tags, `text_content`
for the JSON, plus a tiny tolerant scanner for the `window.X = {...};` globals
(balanced-brace slice → `serde_json`). No turbo-dom additions needed.

**Status:** ✅ ported — `hydration` in `turbo-surf-view` over `rtdom::Tree`
(exposed via napi `hydrationState`). Offline-tested.

## 2. JS-execution hydration (the render tier)

Run the page's **own** scripts so a client-rendered SPA hydrates, then read the
resulting DOM. This is `turbo-surf-render` (deno_core isolate over the turbo-dom
`Tree`).

Requirements and where each stands:

| Requirement | Status |
|---|---|
| Real V8 isolate, host heap unreachable from guest (+ runaway budget) | ✅ deno_core + watchdog |
| Native DOM bound to `rtdom::Tree` (`document`/Element — jQuery/React run) | ✅ `browser_env` (vendored from turbo-test); partial Web IDL coverage |
| `window`/`navigator`/`location`/`localStorage`/`console` + Event classes | ✅ binding + env bootstrap |
| Timers (`setTimeout`/`rAF`/`queueMicrotask`) | ✅ virtual (drained by delay) |
| Promise / `async`-`await` / microtask resolution (event loop) | ✅ `render_html_async` |
| `fetch` + `XMLHttpRequest` over the real net stack (relative-URL aware) | ✅ `op_fetch` → `turbo_surf_core::net` |
| `document.cookie` ↔ `CookieJar` bridge | ✅ `op_cookie_get/set`; HttpOnly cookies hidden/protected from JS |
| `MutationObserver`/`IntersectionObserver`/`ResizeObserver`, history API | Native MutationObserver delivery; Intersection/Resize coverage remains bounded; history updates `location.href` |
| TS/JSX bundle support | ✅ `turbo-surf-transform` (swc) |
| Entry point | ✅ `render_page(html, base_url, script)` → hydrated HTML |

Validated end to end: a mock SPA hydrates into `#root`, an XHR/`fetch`-driven page
fills from a localhost server, and **real jQuery** (`quotes.toscrape.com/js`) renders
its 10 quotes — see `crates/turbo-surf-render/tests/render.rs`.

**Execution model and remaining gaps:**

- Native WebCrypto uses the hardened licensed Deno provider and genuine X-Wing
  composition. Non-extractable key material never crosses a page cloning hook;
  the private V8 serializer carries native key handles out of band. The original
  inherited-setter leak and graph/permission/realm behavior have regressions.
  See [`vendor/DENO_CRYPTO.md`](./vendor/DENO_CRYPTO.md) for the reproducible patch.
- SHA256/384/512 resource integrity is checked against decoded response bytes
  before fetch/script/module/frame/style consumption. Module entry integrity does
  not become the hash for imported dependencies. Native CORP and active HTTPS
  mixed-content checks also run before consumption/contact, including redirects.
  Passive image upgrades, signature/Integrity-Policy enforcement and complete
  embedder/worker/navigation policies remain separate gaps.

- Module scripts now run through the deno_core module loader and ESM graph pump
  (`NetModuleLoader` in `runtime.rs`); classic scripts are queued by the script-element
  pump and compiled/executed by the Rust host pump with their document/source URL.
  Global lexical bindings persist; classic execution no longer uses indirect eval.
- Observer delivery remains partial. Element box geometry now uses the public native
  live-tree layout seam, private actual CSSOM rules and transform provenance. Supported
  client/offset/scroll metrics and point-hit regions have authored Chrome controls;
  shaped text/Range/caret and composed shadow/slot controls are integrated. Source-owned
  decorated/empty/mixed-font inline boxes now have independent controls, including
  corrected macOS generic-serif font selection. The final75-test genuine-native155 cohort passes, including the exact20-scene
  iframe comparison, frame initialization/GC and production custom elements.
  Default/zero iframe and canonical computed-font corrections are integrated.
  Source31bf6fe passes1,453 genuine-native155 workspace tests plus strict checks;
  subsequent Linux CI exposes an exact bundled-font inline advance difference.
  Public layoutdcbfd20 adds complete native leading/internal-space source ownership,
  wrapped endpoints and caret affinity, with767 full producer tests and41 focused
  controls. Current stock/native155 inline targets pass5/5 with1457 source hashes
  unchanged, including all nineteen scenes; complete workspace/addon gates are
  pending. Predecessor085
  stock/native155 inline targets pass5/5 at1457 unchanged source hashes. Predecessor layout5a602097 repairs native
  source advances and block/Range projection;
  the real Surf bridge passes4 stock inline tests and78 actual-native155 component
  tests with1455 source hashes unchanged. Renewed complete default/native workspace
  and rebuilt addon gates remain pending. Unsupported
  inline alignment/writing/positioning, full stacking contexts and broader scrolling
  remain explicit bounds. See [the scope and controls](../harness/detection/LIVE-GEOMETRY.md).

**Current interface corrections (2026-10-10):**

- Repeated synchronous evaluations with identical supplied HTML retain the same document
  and globals. Changed HTML receives a fresh isolate, including its lexical environment
  and built-ins; repeated top-level let/const names and prototype changes cannot leak
  across documents. The `render_page_pooled` compatibility API also uses a fresh runtime
  per call while genuine fresh-main-realm reuse remains unimplemented. This deliberately
  gives up the former cross-page boot optimization; it is not a navigation cache.
- Iframes allocate distinct V8 contexts, native DOM trees, native callback caches,
  event/focus/style state and independently bootstrapped browser API constructors.
  Same-origin callbacks restore the caller's document after reentrant DOM access;
  fetched heads and bodies remain separate from the parent tree. Explicit native
  adoption preserves node identities and changes tree ownership; unsupported node kinds
  reject instead of treating another tree's handle as a local node.
  Node/Element/HTMLElement/Document and character-data interfaces have distinct prototype
  chains. Projected native accessors use the receiver's real tree handle and reject
  incompatible receivers; document-specific instance properties and compatibility APIs
  retain partial Web IDL ownership and descriptor coverage.
- Ordinary HTTP(S) frame globals use V8 origin security tokens. Cross-origin
  `contentDocument` is null. Native response records bind fetched HTML to its final
  redirect URL; guest JSON cannot assign an origin to arbitrary srcdoc HTML. The owning
  context is recorded natively, and loader script execution must match executable inline
  bodies or external script URLs parsed from that authenticated HTML. External code uses
  host-recorded fetched bytes; replacing guest RegExp/JSON parsing cannot authorize an
  injected script. Cookies use the actual native Document's creation context and URL,
  keeping HttpOnly values hidden. srcdoc/blank contexts inherit the owner's origin and
  fetch base.
- Window messages enforce `targetOrigin` and use V8 structured cloning into the
  receiver realm, including cycles and Maps. Native transfer lists move ArrayBuffer
  backing stores and MessagePort ownership after successful serialization and queue
  admission. Endpoint garbage-collection liveness and broader retirement controls
  remain under validation; see [the ownership contract](../harness/detection/MESSAGE-TRANSFERS.md). The published V8 150 backend uses a limited caller-realm Window view:
  protected reads throw `SecurityError`, and replacement creates an isolated context with
  a different `contentWindow` identity. Retained old globals/documents remain isolated
  and callable. The current experimental native V8 155 build (and historical154 profile) uses genuine V8 global detach/reuse and
  native access-check callbacks, preserving `contentWindow` and MessageEvent.source
  identity across same-origin and cross-origin replacements. Same-origin retained
  Function constructors and precompiled functions resolve the new Document; a retained
  Document keeps its original tree and metadata. Cross-origin replacement rejects the
  old Function constructor with TypeError and rejects precompiled document reads with
  SecurityError. Protected Window reads and prototype writes throw branded DOMException
  errors; cross-origin prototype reads return null. Accessible cross-origin method
  functions and descriptor getters are cached in the caller realm. Native per-document
  Location objects enforce restricted cross-origin reads/reflection, branded SecurityError,
  `href`/`replace`/Window.location navigation, and caller-based relative URL resolution.
  Same-origin components, own property order, prototype branding and genuine DOMStringList
  ancestorOrigins are native. Top-level Location writes retain the existing driver-specific
  navigation behavior; general history and same-document fragment navigation remain partial.
  The reusable Chrome operation collector and
  deterministic localhost tests cover these contracts; see
  [the native engine build](../harness/detection/engine-154/README.md).
- Created iframe hosts are native HTMLIFrameElement nodes with prototype accessors and
  private lifecycle records. Detached hosts have null contentWindow/contentDocument;
  inserting a node/subtree/fragment starts its context, including native shadow-tree
  insertion. Initial blank globals survive blank load. Supported DOM mutations and
  markup setters detect removal and retire frame work; reinsertion creates a new
  browsing context, and source attribute changes navigate asynchronously. Load events
  are branded, trusted native Events. Retired timers, descendant timers, classic-script
  queues and queued message delivery stop. Native Document ownership guards prevent
  old documents from initiating replacement-realm script/frame work. Retained guest
  documents, constructors and pending native jobs preserve their actual ownership;
  unreachable contexts can collect before page teardown. Real V8 GC schedules bounded
  native-storage collection and pruning of dead weak-context registrations at host
  boundaries, without traversing Trees inside the GC callback. Registry vector capacity
  retains its high-water mark. Retired definitions retain registry/instance identity
  while automatic upgrades and queued reactions stop; retired documents expose empty
  geometry. Broader reclamation and cancellation of every in-flight async operation
  remain incomplete.
- Connected child indices follow browsing-context connection order. Moving an iframe
  discards its old context and appends the replacement after surviving contexts, matching
  the measured Chrome behavior even when DOM order differs. Shadow-tree frames own real
  contexts but do not contribute to window.length/indices. Source removal commits an
  inherited about:blank document; retained replaced Documents have null defaultView.
- Native sandbox flags inherit through descendants and snapshot the actual iframe
  attribute at document initialization. `allow-scripts` gates parsed and injected HTML
  scripts; `allow-same-origin` controls opaque origins. Same-origin parent-invoked eval
  remains possible when HTML script execution is disabled, matching Chrome. Opaque
  documents reject cookie/Storage access with SecurityError and use the native opaque
  request context. Data HTML/plain frames decode percent/base64 bytes natively, keep
  authenticated document capabilities, and receive opaque origins; unsupported data
  content stays opaque. Sandbox top-navigation, popup, form, download and permissions
  policies, XML/image data documents and full cancellation are not implemented.
  CSP script/resource restrictions are described below; complete CSP conformance remains partial.
- The exact native154 SDK gives each child context its own native Deno module map,
  loader, cache and evaluation lifecycle. Retiring a frame cancels its graphs and
  releases private script completion gates. Opaque module fetches use Origin:null,
  omit same-origin credentials and require an allowed CORS response; sandboxed
  script-disabled frames do not start graphs. The published SDK fallback rejects
  child modules instead of sharing the parent map. General navigation and full
  sandbox/CSP policy coverage remain incomplete. Transfer options and full Web IDL descriptor coverage are
  incomplete. Additional Storage, History and layout behavior remains partial; these
  tests establish specific contracts rather than full browser or security conformance.
- Native Node/Element/HTMLElement/CharacterData accessors now live on distinct
  interface prototypes. Getter/setter functions are native, nonconstructible, named
  and receiver-checked; their values still route into the owning native tree. Parser,
  clone and create APIs project the same cached node identities. Shared native methods
  preserve expando identity. Node constants and DOM event methods live on their owning
  interfaces; EventTarget subclasses retain separate listener state. A native callable
  bridge forwards actual receivers and class `new.target` into the guest implementation
  without replacing Function.prototype.toString. Interface key/descriptor coverage is
  still incomplete for unsupported APIs. Native elements retain HTML/SVG/MathML, null and
  custom namespace URIs, prefixes and qualified names through mutation, cloning and import.
  Namespace-aware attributes compare URI plus local name; native Attr nodes retain identity,
  live values and single-owner attachment semantics. NamedNodeMap is live and branded, and
  namespace lookup follows declarations, native prefixes and ancestors. This does not add
  XML document parsing or serialization.
- Document forms/images/links/anchors/embeds/scripts collections and supported
  getElements queries are live native indexed HTMLCollections; getElementsByName and
  childNodes are live NodeLists, while querySelectorAll returns static NodeLists.
  ParentNode.children retains stable collection identity. Native property handlers
  implement indexed/named reflection and supported read-only entries; a private shared
  V8 Private record gives borrowed getters their actual cross-realm records without accepting
  fabricated dispatchers or retaining creation contexts on the host. Tree ownership/version
  stamps refresh query results after
  mutations and adoption. DocumentType metadata and compatMode use actual native parser state;
  designMode, title, direction and legacy colors store or reflect real supported state.
  Document event-handler properties retain native listener ordering and cancellation.
- adoptNode preserves existing native wrapper identities while moving tree ownership,
  attached Attr nodes, shadow trees, listeners, control values and nonce state. Created
  fragments, text, comments, processing instructions, attributes, doctypes and template
  content are supported. Native custom-element adoption callbacks have independent
  Chrome controls; broader form-associated/reaction lifecycle remains bounded. Import/clone copy supported
  live control values and nonce state through captured per-document helpers. HTMLElement
  nonce keeps its private value independently of the reflected attribute; tabIndex uses
  measured defaults, HTML integer parsing and Web IDL conversion. These contracts do not
  implement an editing engine or general focus/layout behavior.
- ProcessingInstruction is a native CharacterData node with an immutable target, XML-name
  validation, HTML serialization and clone/import/adoption support. Character-data methods
  use V8 UTF-16 offsets and throw IndexSizeError for invalid ranges. HTML documents reject
  createCDATASection with NotSupportedError; XML document factories are bounded and
  CDATA construction/XML parsing remain incomplete. HTML XML
  metadata follows the measured null defaults, mutable xmlStandalone and version-1.0-only
  setter. Native original-unit APIs preserve unpaired UTF-16 through stored DOMString
  mutations, parser provenance and script sources; serialized Rust strings still
  require explicit conversion. Broader parsing/DOMString edges remain tracked.
- Event state lives in private native records. Script dispatch remains untrusted;
  private record maps capture their native WeakMap operations before guest execution.
  Trusted lifecycle, message delivery and script/module completion callbacks live in
  native context slots. Privileged callbacks use strict calling semantics and captured
  Event/ErrorEvent constructors, preventing guest replacements or Function.caller from
  obtaining a trust-minting capability. The internal Symbol.for('ts') support namespace
  remains observable; its remaining frame records do not grant cross-origin document access.
  Resource, lifecycle and frame events use a host-only dispatch entry point. Event fields
  are read-only accessors. Element property handlers use private state and the native
  listener registry, retaining assignment order and cancellation behavior. Window load
  and pageshow retain their measured Chrome legacy target/dispatch state. Inline event-handler
  attributes, propagation, listener options and event subclass coverage remain
  incomplete.
- DocumentFragment uses a real native fragment node; append/insert moves its children
  into the destination and empties the fragment. Native ShadowRoot retains distinct
  branding, host/mode, open/closed lookup, scoped queries and composed root lookup.
  Slot assignment, event retargeting, shadow options beyond mode and composed layout/CSS
  remain incomplete; this is not a full Shadow DOM implementation.
- Child classic scripts execute in their own context through the host queue with
  document source locations. The native154 SDK also evaluates child module graphs in
  separate module maps; the published fallback explicitly rejects child imports before
  touching the main map. Main-document modules retain their own loader. Each frame
  has its own timer/microtask/resource work; styles finish before initial classic
  scripts, DOMContentLoaded precedes resource-complete window load. Analytics/auth
  churn hosts can still use an inert frame stub.
- CSS Syntax tokenization/error recovery uses `cssparser`, with Lightning CSS validating
  selectors and supported property values while retaining authored valid declaration
  text. Unsupported property grammars retain token lists. Style text parses into
  style/group rules, including `CSSStyleRule` selectors. CSSOM
  insertion/deletion changes the sheet without changing the DOM text; replacing style
  text replaces the sheet. CSSRuleList and StyleSheetList are branded live indexed
  collections with native, receiver-checked list methods and accessors. Style/import rule
  fields use private state behind native prototype accessors. CSSStyleSheet inherits
  native branded StyleSheet callbacks with private rules and owner/import metadata;
  MediaList exposes stable indexed queries and actual mutations. Constructed sheets
  reject inserted imports and filter them from replacement; single-rule insertions
  validate their complete token stream. Inline/rule declarations
  share branded native CSSStyleDeclaration methods with stable identity and private values;
  inline edits update the native style attribute; authorized external attribute changes refresh
  existing declarations. CSS records share the isolate's private ephemeron registry so same-origin
  cross-realm borrowed getters retain their brands. Supported computed values are live and read-only. CSS property
  enumeration, shorthand serialization, unsupported property grammars and the full
  computed cascade remain incomplete. Internal stylesheet loading
  and computed-style reads bypass page-installed CSS accessors in the detection trace. Hydration loads
  stylesheet links and import graphs with resource request
  context. Opaque cross-origin sheets protect their rules and mutations with SecurityError;
  CORS-authorized stylesheet access uses the actual native response policy. Loader functions and
  internal response metadata stay private. This facade still has partial at-rule, nesting and Web IDL coverage, and CSSOM
  changes feed the live geometry cascade through private stylesheet exports.
  Unsupported grammar, dynamic pseudo states, downloaded fonts and composed shadow
  scoping still limit this native cascade; it is not complete browser layout.
- Scalar ARIA, editing enums, hidden state and basic HTML properties reflect actual native
  attributes. Spellcheck, translation and writing suggestions resolve ancestor values;
  capitalization/correction use associated forms. Reflection of `inert`/`autofocus` does
  not implement browser input suppression or automatic focus. Nonce internal state,
  editing services, popovers and full attribute/interactivity coverage remain incomplete.
- Parsed hidden controls expose populated `.value` properties, including after insertion
  and cloning. Text input/textarea live values have dirty state separate from their
  defaults; textarea defaults use text, and select defaults/unmatched values have explicit
  selection state. Cloning carries live control state. Full input-type sanitization,
  validation and reset behavior remain incomplete. HTML serialization contains content
  defaults and does not preserve every live control value across a new runtime.
- Live hydration advances the document through loading/interactive/complete, runs deferred
  scripts before DOMContentLoaded, and waits for image/resource loads before window load.
  PNG/JPEG/GIF/WebP images are fetched as bytes and decoded before success/dimensions are
  exposed. SVG/AVIF, responsive image selection, CSS image/font initiation, incremental
  parsing and complete browser async-script scheduling remain outside this resource loader.
  Image, linked/imported stylesheet and classic-script fetches record their actual duration
  in the performance resource timeline; unavailable wire sizes and transport phases remain zero.
  Concurrent resource requests merge their cookie writes and deletions into the shared
  jar without discarding unrelated response or document.cookie updates.
- `ClientHints` holds negotiated `Accept-CH` state per origin and a scoped HTTP client.
  Render sessions share that state with fetch/XHR and module requests; callers must pass
  the same state through navigation and a replacement runtime to retain it. Request
  contexts select navigation/resource headers, referrer/Origin and credentials behavior.
  Simple and preflight CORS authorization checks run before guest-visible responses;
  every followed redirect applies native origin/cookie/CSP/CORS policy and negotiates
  a fresh preflight when the rewritten cross-origin method/headers require it. Authorization
  is preserved on same-origin hops and permanently removed after an origin transition.
  Public fetch honors follow/manual/error redirects, including opaque redirect filtering
  and redirect-limit rejection. Connection failures reject with TypeError; HTTP error
  status codes still resolve to Responses. Permissions-policy hint delegation, Critical-CH
  retries and a complete browser preflight cache remain unsupported. Request cache/integrity/signal/referrer policies have bounded real network
  effects; binary bodies, response streams/cancellation and HTTP2 uploads are
  integrated. Keepalive and full browser Fetch/cache behavior remain incomplete.
  Genuine multipart FormData BodyInit still throws explicitly. The default
  referrer policy is strict-origin-when-cross-origin. Full resource-policy and
  HTTP authentication behavior remain partial.

The reusable [detection harness](../harness/detection/README.md) records baseline and
instrumented Chrome/native runs separately. The corrected CSSOM sentinel, iframe
intrinsics and shared GPU probe now match in the measured tests, but the fresh native
Google runs remain unsuccessful. Earlier baselines reached `/sorry` with HTTP 429;
the latest run, after the ForcedColors media fix, fails on CSS rule type0 before
form submission. These checks do not establish complete Chrome parity or identify
Google's decisive rejection signal.

## 3. Driving an authenticated SPA (live sessions)

Beyond one-shot render, `PageSession` (render tier) keeps a hydrated app **alive
across calls** — the V8 isolate, React fibers, closures, and delegated listeners
persist, so dispatched events re-enter the running app and the re-render is
observable (the one-shot `render_*` paths serialize + reset after each call,
killing the app). Thread-per-session (the isolate is `!Send`); `eval` drains to a
stable-DOM signal and returns best-effort on the budget. napi:
`liveOpen`/`liveEval`/`liveSerialize`/`liveCookies`/`liveClose`; the Playwright
shim opens a live session on a `networkidle` `goto` and dispatches real
click/fill events (fill bypasses React's `_valueTracker` via the native value
setter; click fires mousedown→focus→mouseup→click + the form submit default).

**Validated end to end (no browser):** a real PropelAuth login —
`fill email/password → click submit → onSubmit → POST login (200) → session
cookie → client redirect chain (login → /post-login → /auth/me →
/entity/{id}/admin/home)` — renders the authed dashboard fully. In-app redirects
(a path change in the live session) re-load the new route as a fresh page
carrying cookies, so the redirect chain completes hop-by-hop. Test:
`live_session_dispatches_events_into_running_app`.

**Open limitation — cold deep-route loads render empty.** A *cold*
`goto('/entity/{id}/admin/people/active', {networkidle})` (cookie carried, no
prior in-app nav) commits an **empty app-root `div`** while `/admin/home` (same
shell + providers) renders fully. It's why many authed-page e2e specs still fail
"locator matched no elements". A few specs (`boundingBox`) need a real browser by
design.

Diagnosis so far (via a React-DevTools-hook probe over the live isolate):
- **React is NOT parked/suspended** — it fires `onCommitFiberRoot` 12× for the
  empty route (19× for the rendering one). It commits; the people segment just
  reconciles to **empty host DOM**.
- **Not a missing module/chunk** — all client-reference modules register and all
  chunks load `200`. **Not data-suspense** — the app has no `useSuspenseQuery`
  and no data fetch fires. **Not an error** — the only console error is a benign
  `Cannot read properties of undefined (reading 'prototype')` that appears
  *identically* on the route that DOES render (red herring).
- Both routes render `Providers` + `Theme` identically; divergence is deeper, in
  the segment subtree, and **silent**. Other deep authed routes (e.g.
  company-settings) render fine — so it's specific to certain segments.
- **Not suspense** either: a Suspense-boundary fiber probe (tag 13) shows the one
  boundary is NOT in fallback state at the final commit (`suspended=0`). So React
  commits a real tree; the people segment subtree just resolves to nothing.
- **Not a redirect**: the only `history.replaceState` is Next's canonical-URL
  sync to the *same* path (`admin/people/active`), not a navigation away.
**Localized via prod `console.log` markers down the render path (overturns the
"returns null" guess):** the ENTIRE people component tree executes — markers fire
for `AdminRouteLayout → EntityAdminGuard → PayrollAdminLayout → PeopleLayout →
PeopleProvider → PeopleLayoutInner → ActiveEmployeesPage → EmployeesListPage →
EmployeeTable`. So nothing returns null. Render-pass counts are **finite** (most
=1; `PeopleProvider`=3, `EmployeesListPage`=4, `EmployeeTable`=4 — normal
state-settle, NOT an infinite loop). Yet `BODYLEN=0`: React runs every render
function but **commits no non-empty DOM** (not even the shell that DID render its
functions). `/admin/home` cleanly stops at `PayrollAdminLayout` and commits.

The deepest app component that renders is `EmployeeTable` → which renders
`<DataTableProvider><DataTable/></DataTableProvider>` from
`@flux-payroll/flux-payroll-ui` (a custom grid; readable source under
`dist/components/data/DataTable/`). `DataTableCore` does
`useMediaQuery(theme.breakpoints.down('md'))` to pick table-vs-card layout. The
empty commit originates **at/below this design-system grid** — every higher
component (incl the shell) runs its render but the atomic commit is empty, which
means the failure is during React's render/commit of the grid subtree (a thrown
value or a render-phase update that prevents commit), not a null return upstream.
The grid's own code has no `extends`/`.prototype`; the `prototype` TypeError is
unrelated (fires identically on `/home`).

**Blocked on tooling for the last mile:** the prod build is minified and our V8
isolate captures **no stack frames** (even with `Error.stackTraceLimit` raised).
A `next dev` build would name the failing internal — and the render tier can now
hydrate dev builds (the `import.meta`/ESM + best-effort-on-budget work merged on
`main`) — but `next dev` login was flaky to drive headlessly. Last mile: drive a
dev session to a readable React error at the `DataTable` grid, or bisect the grid
internals (likely a `useMediaQuery`/layout path or a host API the grid needs).

**Root cause confirmed (dev-build probe):** running the route through a `next dev`
build (in a temp checkout with its own `.next`, so the prod `:3000` server is
untouched — the engine can now open dev sessions thanks to the `import.meta` +
best-effort-on-budget work on `main`) renders **"Application error: a client-side
exception has occurred"** — Next's **root error boundary** fallback. So
`/people/active` **throws a client-side exception during render**; with no
intermediate `error.tsx` boundary in the entity/admin/people segments, it unwinds
to the root boundary, which replaces the whole tree (empty on the minified prod
build, the generic "Application error" page on dev). That's why the shell blanks
too even though its render function ran.

So the chain is: every component renders → the `DataTable` grid subtree throws →
root error boundary → blank. NOT null-return / suspense / missing-module /
data-fetch / redirect (all ruled out). The throw is in `DataTable`'s render-phase
hooks (it pulls `useMediaQuery`, `useTableStickyPositions` (DOM measurement),
fullscreen APIs, analytics) — one needs a host API our env lacks or mis-stubs.

**Last-mile attempt (deep instrumentation) — refined, partly inconclusive:**
- **Dev throw is real but the error is unreadable headlessly.** Dev renders the
  root-boundary "Application error", but React **swallows the caught error** in our
  env (no DevTools overlay; `console.error` is re-patched by Next dev and even a
  `defineProperty`-locked hook captured nothing), and dev sessions open
  **best-effort** (dev React loops past budget → terminated isolate → `liveEval`
  returns `""` for everything, even `1+1`). So globals/console can't be read off a
  dev session.
- **On PROD the grid does NOT throw.** Wrapping `DataTableCore`'s body in
  try/catch (catches its hooks incl `useTableStickyPositions`) → `data-griderr`
  empty. Adding a real **error boundary around the grid's children** → still
  empty. So neither the grid's hooks nor its descendants throw on the prod build,
  yet `BODYLEN=0`. **The prod-empty and the dev-"Application error" look like
  different failure modes** — the earlier "grid subtree throws" was over-attributed
  to the grid.
- The `Unexpected token '.'` dev chunk turned out to be **CSS inside a `<script>`**
  (Next devtools styles). Running it + logging a SyntaxError + continuing is
  exactly what a real browser does — **browser-accurate, not a bug**. So (a) needs
  no further fix; dev-build support is complete via the merged #2.

**RESOLVED via a real-Chromium oracle (the decisive tool).** Driving the route in
actual Playwright/Chromium against the SAME servers showed the dominant cause was
**the environment, not turbo-surf**:

1. **Prod CSP blocked the backend.** `next build` emits a strict CSP whose
   `connect-src` omits `http://localhost:*` (only dev allows it), so the browser
   blocks `GET http://localhost:3001/auth/me`. Real Chromium hit this too → blank.
   (e2e normally runs the dev CSP.)
2. **Backend is 3 months out of sync (the dominant cause) → `500`s.** flux-apis
   HEAD is *Release 11 March 2026* but its DB has *June* migrations applied. The
   March code queries schema the June DB changed: the June migration **dropped
   `entity_invitations`** (yet March `auth/me` still `SELECT`s it → 500), and
   `employments` **lost `job_id`** (March `Employment` model still selects it →
   `column Employment.job_id does not exist` → `GET /employments` 500), etc.
   Recreating `entity_invitations` makes `auth/me` 200; with that + the CSP fix,
   **real Chromium renders `/people/active` fully** (5 tabs + add-employee button;
   the grid is empty only because `/employments` still 500s). The *real* fix is to
   sync flux-apis code↔DB (check out a June commit, or reset the DB to March).

3. **The measured auth flow has matching cookies and response data.** Its cookie jar carries the
   cross-domain `refresh_token` cookie (`*.propelauthtest.com`); it performs
   `GET …/api/v1/refresh_token` (×2) → `GET …/auth/me` — **all `200`**, and
   `auth/me` returns the **complete** user + organizations payload (len 2040),
   identical to Chromium.

4. **Residual turbo-surf gap — FIXED (commit `2a92f36`).** With env at parity,
   turbo-surf still committed an empty `<div>` because the page **never quiesced**
   within the render budget — two timer/host bugs let third-party scripts spin:
   - **No virtual clock in `__runTimers`.** `delay` was only a sort key, so a
     self-rescheduling `setTimeout` poll (analytics SDKs do this) fired until the
     raw count cap — spinning the whole budget so React never committed. Binding
     instrumentation (file-logged from Rust) showed `/people` doing 1150 creates /
     1319 appends and the hydrate running the FULL 30s budget with the body still
     empty. Fix: a virtual clock — `due = __now + delay`; the drain advances `__now`
     to each fired timer's due and stops delayed timers past a 15s virtual ceiling,
     so polls fire a browser-like ~tens of times and the page quiesces.
   - **`<iframe>` had no `contentWindow`.** PostHog reads a builtin's native
     prototype off a throwaway iframe's `contentWindow`; with none it bailed
     without caching and recreated an iframe on EVERY lookup — **776** iframe
     create/remove churn that also starved the budget. Fix: iframes get a
     lightweight stub whose `contentWindow` was our realm (lookup resolved + cached
     → loop stopped, 776→4) and which never entered the rtdom tree. Current unreleased
     code replaces that shared-intrinsic stub with a distinct V8 context and native
     document; see the current interface corrections above for the origin boundary
     and remaining WindowProxy/policy limits.

   Result: `/people/active` 2 tags → **485 tags in ~2.2s**; `/home` (459) and
   company-settings (557) also drop to ~1.4–2.2s (were near/over budget). Verified
   with real Chromium as oracle. Tests: `virtual_clock_bounds_self_rescheduling_timers`,
   `iframe_content_window_exposes_builtins`.

**Net: cause #2 fully resolved.** The dominant blocker was a stale local backend
(March code vs June DB → `auth/me` 500); recreating the dropped `entity_invitations`
table makes auth 200. The turbo-surf side was two budget-starving spin loops
(timer virtual clock + iframe contentWindow), now fixed — authed SPA pages render
headlessly. (The local backend still needs code↔DB sync for `/employments` etc.;
that's an env fix, not turbo-surf.)

### Probing gotchas (reusable)

- **`fetchHtml`/`fetchWithCookies` (napi) return a JSON string**
  (`{"html":...,"status":...}`), not raw HTML — `JSON.parse(...).html` before
  `hydrate`. Passing the raw JSON in JSON-escapes every `"`, so the inline
  `__next_f.push` flight scripts fail to eval.
- **`next build --turbopack` + `next start` is unreliable for prod probes** here
  (`routesManifest.dataRoutes is not iterable`; a stale `.next` serves a 404ing
  runtime chunk → false "empty render"). Use a standard webpack `rm -rf .next &&
  npm run build && npx next start`, and verify the referenced runtime chunk
  returns 200 first. The dev server is the reliable target.
- Cap probes at the Node level and `pkill -f turbo-surf` after.

## Lane routing (when to hydrate)

`detect` (Lane B heuristic, ported to `turbo-surf-view`) decides whether a page is
JS-gated and worth the isolate: near-empty rendered text + heavy external scripts, or
an empty known SPA mount (`#root`/`#app`/`#__next`/`[data-reactroot]`). A caller picks
Lane A (no-JS parse) vs Lane B (`render_page`) from its verdict.

## Driving the full Playwright suite through the shim (what we built + what stalls)

Running the payroll-app Playwright e2e suite through the `playwright-shim` (over the
napi addon, no Chromium) took it from **6/114 → ~75/102 passing, 0 skipped**. The
work split into render-tier capabilities and shim parity. This section is the field
record: what we implemented, and — equally important — what we *tried that did not
work* so the next person doesn't re-walk it.

### Implemented (render tier)

- **RSC soft-nav follow.** Next App Router client navigation (`router.push/replace`)
  fetches the target's RSC flight with an `RSC` header and never changes
  `location.href` headlessly. The fetch wrapper records the target on `__rscNav`; the
  live-session driver re-loads that route as a fresh page (browser hard-nav
  equivalent), following a redirect CHAIN hop-by-hop. This is what makes login
  (`/login → /post-login → /entity/…`) complete.
- **Fetch-aware drain.** `drain_to_quiescence`/`__pendingWork` tracked timers +
  un-run scripts but NOT in-flight fetches, so a save `POST` could resolve *after* the
  drain quiesced (the DOM looks "stable" while waiting) and its success re-render — a
  modal close, a redirect — was lost. A `__pendingFetches` counter now keeps the drain
  pumping until requests settle.
- **Network log → events.** Each live-app `fetch` is recorded in private native op-state; the shim
  drains it through the host-only `liveNetworkLog` accessor to emit `page.on('response')` and back a real `waitForResponse`. Tests use
  these to capture API payloads (payroll period, employments lists).
- **Globals:** `KeyboardEvent` dispatch path, `navigator.clipboard` (in-memory
  write/read), `structuredClone` (V8 serializer for Date/RegExp/Map/Set/cycles and typed arrays, ArrayBuffer transfers and native MessagePort ownership). Apps
  probe `globalThis.structuredClone.prototype`; absent it threw.

### Implemented (shim parity)

- **getBy\* live-drive.** `getByRole/getByLabel/getByText` have no CSS selector, so
  `fill`/`click`/`check` fell back to mutating the static HTML snapshot — never
  reaching the running React app (empty form fields → failed saves). napi `get_by`
  now returns the matched element's document-order `idx`; the shim dispatches into the
  live isolate via `querySelectorAll('*')[idx]`.
- **Deepest-descendant event dispatch.** A real click lands on the leaf under the
  cursor and bubbles up; dispatching on the matched wrapper missed handlers bound to
  inner nodes (a MUI `Select`'s inner `role="combobox"` opens on its own mousedown).
  Events now fire on the deepest child; focus + form-submit logic stay on the control.
- **preventDefault honoring.** A click reports `defaultPrevented`; the static-intent
  fallback (anchor navigate / form POST) is skipped when the app handled it — an
  `<a href="#">` whose React `onClick` toggles state no longer also navigates to `#`
  and wipes the new state (the company-settings edit toggles).
- **Web-first assertion retry.** `expect(locator|page)` re-evaluates (re-pumping the
  live app between tries) until it passes or times out — a UI change that lands just
  after an action (modal close, async row, redirect) is observed.
- **`addInitScript` actually runs** (prepended as the first `<head>` script of the
  live session) — `injectTestPartner`'s `window.__FLUX_DYNAMIC_PARTNERS__` etc.
- **`waitFor` polls real state** (visible/hidden/attached), `evaluate` awaits a
  returned Promise, `setInputFiles` (File objects in the isolate), `hover`,
  `dispatchEvent` (+checkbox toggle), `test.skip(cond, reason)` overloads (killed 12
  bogus skips), per-test fixture sharing (afterEach gets the live logged-in page),
  live-session close on teardown (isolate leak), RegExp locator names, `boundingBox`
  → null.

### The remaining wall: RSC-flight hydration of deferred Suspense boundaries

~7–8 failures (vacation, time-entry, parts of company-settings) all reduce to ONE
thing: a deep client subtree never hydrates. On the payroll wizard, React hydrated
**484 of 996** elements (stable — more pumping doesn't change it); the static
"Add Manually" button has **no React fiber**, so it has no `onClick` and clicks are
dead (native or synthetic), so the `dynamic()` modal it would open never mounts.

**Bisected with repros (each run through the real render tier):**

| Hypothesis | Verdict |
| --- | --- |
| Dynamic `<script>` inject → `onload` → promise → wire handler | **works** — `dynamically_injected_script_runs_and_fires_load` (committed) |
| Real React 18 `Suspense` + `lazy()`, client render | works (ad-hoc diag, React 18 UMD) |
| Real React 18 `hydrateRoot` of pre-rendered `Suspense`+`lazy` | works (ad-hoc diag) |

So **React core hydration + code-splitting work in the engine.** The failure is
specific to **Next App Router RSC**: the wizard page content is a Suspense boundary
that suspended during SSR; client hydration of it is deferred until its **RSC flight
chunk** (`self.__next_f` → `react-server-dom-webpack` reader) is consumed, and in the
isolate that deferred boundary's flight never reaches React's reader. Click-triggered
selective hydration can't rescue it either — it needs the same flight data.

### Things we TRIED that did NOT fix it (don't repeat)

- **More pumping / bigger budget.** Fiber count is stable at 484 regardless of extra
  drains — hydration isn't merely incomplete-by-budget, that boundary is never
  scheduled.
- **Non-zero layout measurement.** Forced `getBoundingClientRect`/`offsetWidth`/
  `clientWidth` to a real box (theory: libs gate render on measurement). No effect on
  the un-hydrated subtree. **Reverted** (risk to the passing set).
- **`structuredClone`.** Added it (was genuinely missing) — did not change hydration.
- **The `TypeError: …reading 'prototype'` (×5).** Red herring. It's **caught** (never
  reaches `window.onerror` nor the page `console.error`; the page's own override sees
  nothing) — a PostHog native-prototype probe. Not in React's hydration path.
- **Chunk-load / script-skip failures.** None: 251 chunks load with **0** failures,
  **no** `script skipped (ESM…)` and **no** `script error` during the wizard load.
- **DB flush + reseed.** Net wash for this (helped grant-leak/locale data pollution,
  but a fresh DB lacked KB seed data and exposed a flux-apis platform-role seed gap →
  KB `403`). NOT a turbo-surf issue. `migration:revert:all` is also blocked by an
  irreversible app migration — don't rely on it for a clean rebuild.

### Precise diagnosis (validated against real Chromium)

With the env finally correct — **real Chromium on `next dev` PASSES the vacation
spec** (prod CSP drops `http://localhost:*` from `connect-src`, so a real browser
on a prod build can't reach flux-apis `:3001` → "Failed to load organizations" →
empty entity-select; the shim isn't subject to CSP since it fetches via the host
net layer, so it uses the **prod** build for classic chunks). So the wizard failure
is **confirmed turbo-surf-specific**: same flow, real Chromium green, shim red.

Walking the dead "Add Manually" button's ancestors in the shim's hydrated DOM:

```
BUTTON#vacation-time-add-manually-button : none
DIV … (the whole page/wizard subtree)    : none
MAIN                                      : FIBER
DIV / DIV#payroll-admin-shell            : FIBER
```

React hydrates the **layout** (`payroll-admin-shell` → `MAIN`) but NOT the App
Router **page-segment** inside `MAIN` — that boundary stays **dehydrated**, so its
whole client subtree (incl. the button) has no fiber and no handlers.

Ruled out (each tested): deps mismatch, prod-vs-dev CSP (that's the *real-browser*
blocker, not the shim), failed/missing chunks (251 load, 0 fail), skipped ESM
scripts (none), React core (Suspense+lazy+hydrate all work via React-18-UMD diags),
the flight stream not closing (**328 ReadableStreams created, 328 closed**),
`useSearchParams` (page uses `useParams`, which doesn't suspend), the `prototype`
TypeError (caught PostHog probe, not in the hydration path), and "needs more pumping"
(fiber count is stable at 484/997 across redraws + clicks + load/DOMContentLoaded
events — React is waiting on a *signal*, not scheduler ticks).

So: the page-segment dehydrated Suspense boundary's flight arrives + the stream
closes, but the **flight-resolved → reconciler "retry the dehydrated boundary"
link never fires** in the isolate, and selective-hydration-on-click doesn't rescue
it (a synthetic click doesn't trigger React's continuous-event replay for the
dehydrated boundary). That link lives inside Next's bundled
`react-server-dom-webpack` + React reconciler.

### Where to start next (RSC flight)

The tractable next step is instrumenting how `self.__next_f` (the array Next replaces
with a stream-feeding `.push`) is consumed by Next's bundled `react-server-dom-webpack`
client reader in the isolate — specifically whether late `__next_f.push([1, …])`
chunks (the deferred boundary's flight) reach the reader, and whether the flight
`ReadableStream` is ever closed. That reader is bundled + minified inside the Next
chunks, so it can't be unit-tested in isolation here; a focused diagnostic that taps
`__next_f.push` and the flight stream controller is the way in. This is a render-engine
project, not a shim gap.

### Other non-turbo-surf blockers seen (env, not engine)

- **KB write `403`** — KB uses `@RequireAnyLocalRole('FLUX_ADMIN')` resolved from a
  *local* DB role mapping; after a postgres reset the PropelAuth user persists but the
  local role row isn't recreated → 403. flux-apis seed gap.
- **es-MX locale test** — the shim seeds `NEXT_LOCALE=en-US` (needed so English
  text/role-name assertions match everywhere else); the one settings test wants the
  es-MX default. Proper fix is per-user backend-pref locale, not a global cookie.
- **Grant order-dependency** — delegated-write grants are shared server state; the
  auth-guard grant tests pass in isolation but bounce in the full run. The
  `afterEach` revoke (now driven correctly via fixture sharing) mitigates but doesn't
  fully serialize it.
- **Run serial.** `node --test` defaults to parallel → concurrent PropelAuth logins
  trip rate-limiting; the real Playwright config is `workers: 1`. Use
  `--test-concurrency=1` (set in payroll-app's `test:e2e:turbo:run`).

## Hydration regression harness (standing rule)

**Every headless-hydration issue we fix gets a permanent, committable repro here.**
Don't rely on the live payroll app (it churns + needs the full env) — capture the
mechanism in a self-contained fixture driven through the render tier.

- **Render-tier fixtures:** `rust/crates/turbo-surf-render/tests/fixtures/*.html` —
  self-contained pages (React/ReactDOM UMD **inlined**, no npm dep at test time),
  loaded by a `#[tokio::test]` in `tests/render.rs` via `include_str!`, asserted by
  opening a `PageSession` and checking the hydrated behaviour (e.g. a button's
  `onClick` firing → `window.__clicked`).
- **Generators:** `rust/crates/turbo-surf-render/tests/fixture-gen/*.mjs` regenerate
  the fixtures from real React (run with the app's `node_modules` path). Committed so
  the fixtures are reproducible, not magic blobs.
- First guard: `react18_streaming_suspense_boundary_hydrates` (fixture
  `react-streaming-hydration.html`, generator `gen-react-streaming.mjs`). Real React 18
  streaming SSR: a `<Suspense>` that suspended on the server → streams its content late
  with a `$RC` completion script that walks the `<!--$?-->…<!--/$-->` comment markers
  and calls the boundary's `_reactRetry`. **It passes** — proving the generic
  dehydrated-boundary hydration path (comment markers + `_reactRetry`) works in the
  isolate, which is exactly why the Next App Router wizard failure is narrowed to the
  **RSC-flight (`react-server-dom-webpack`)** variant, not generic React.
- The RSC-flight variant can't be isolated cheaply: `react-server-dom-webpack/server`
  requires the bundler-only `react-server` export condition, and the client's flight
  references resolve through the real app's webpack runtime. Until that's stubbed, its
  repro is the full-app shim e2e (the payroll wizard vacation specs).

### Correction (flight capture): NOT a dehydrated Suspense boundary

Captured the real wizard HTML + flight (`gen` via the shim). Key facts that
**overturn the "dehydrated boundary" guess**:

- The wizard HTML has **0** Suspense stream markers (`<!--$?-->`) and **0** `B:`
  templates. So the page content is NOT a server-streamed/dehydrated boundary.
- 11 `__next_f.push([1,…])` flight rows: client-ref tables (`I[id,[chunks],name]`)
  + the App Router segment tree (row 8: `entity→admin→payroll→scheduled→
  [payrollRunId]→__PAGE__`, each wrapped in `$L5` ClientSegmentRoot) + the full
  element tree (row 9, ~324 KB, `$L{id}` client refs).
- The **generic** React-18 streaming dehydrated-boundary path PASSES the harness
  fixture (`react18_streaming_suspense_boundary_hydrates`).

So the failure is: React hydrates the **layout** (`payroll-admin-shell`→`MAIN`),
but the `__PAGE__` segment's **client tree built from the RSC flight doesn't
reconcile onto the SSR page DOM** (the SSR rendered the button; the client tree
resolves that segment to empty) → the page subtree gets no fibers. Most likely a
`$L{id}` client ref for the page component whose **webpack chunk loads but its
module factory doesn't register** in the isolate (so `__PAGE__` resolves empty),
or an RSDW flight-row reconciliation gap. Confirmed in the isolate: 251 chunks load
`200`, but module-registration of the specific page client ref isn't verified.

**Next:** verify webpack module registration in the isolate — after load, check
`__webpack_require__`/`self.webpackChunk_*` has the page component's module id (the
`$L{id}` for `__PAGE__`); if the chunk pushed before the webpack runtime installed
its array `.push` handler, the module never registers. (A `next dev`, non-minified
build would name it directly, but the classic render tier skips ESM `import`/`export`
scripts, so dev/turbopack chunks don't run — dev diagnostics need the ESM-script
support tracked on `main`.)

### Also ruled out: webpack module registration

`self.webpackChunk_N_E.push !== Array.prototype.push` (the webpack runtime DID
install its jsonp callback) and chunk entries carry registered modules
(`[[8441],{…}]`, mods≥1). So client-component modules register fine — consistent
with the layout hydrating. Not a chunk/registration bug.

**Convergence:** with dehydrated-boundary, flight-close, webpack-registration,
generic-React, and env/CSP all ruled out, this re-confirms the §3 finding — the
`__PAGE__` segment's client render RUNS (component markers fire) but the atomic
React commit for the **flux-payroll-ui `DataTable` grid subtree** is empty (no
thrown error surfaced; `matchMedia` stub is complete so `useMediaQuery` isn't it).
It's a render/commit-phase failure in that design-system grid.

**The real unblock is tooling:** a `next dev` (non-minified) build would name the
failing internal + give stack frames, but the classic render tier skips ESM
`import`/`export` scripts, so turbopack/dev chunks don't execute. Landing ESM-script
support in the render tier (tracked on `main`) → run the dev build → named frame at
the empty commit → fix. That's the next lever, not more black-box probing of the
prod bundle.

## ESM support landed (foundation) + dev-build status

The current explicit `<script type="module">` path uses native dynamic imports
and `NetModuleLoader` to resolve and fetch real HTTP(S) graphs over the shared cookie jar,
client hints and script request context. Async graph loads run concurrently;
non-async modules wait in document order before DOMContentLoaded. Explicit module
execution keeps `document.currentScript` null. HTTP failures and non-JavaScript
MIME responses reject the graph instead of executing their bodies.

The exact native154 SDK adds a `ModuleLoader` evaluation-stage callback. It records
actual V8 evaluation, parse/link exceptions and graph-load failures; its host pump
then dispatches script completion. This releases script `load`, DOMContentLoaded
and window `load` before a pending top-level await finishes. Parse/runtime errors
produce trusted window `ErrorEvent`s with the original V8 error and source location;
the fetched external script still receives `load`. Inline modules have distinct
module-map keys while V8 source URLs and `import.meta.url` identify the document.
The published Deno SDK fallback waits for the dynamic-import promise, so its TLA
lifecycle and module exception classification remain partial, and child modules
reject on that fallback. The exact SDK registers independent native child module
maps in their actual V8 contexts and polls them with the shared isolate event loop.
Identical URLs cache within each context, while imports, `import.meta.url` and
DOM access use that child context. Final response URLs determine redirected module
source identity and relative imports. Real timer queues continue progressing while
a graph awaits document lifecycle events; a bounded host pump prevents a timer
queue from running forever. Child navigation destroys pending graph work rather
than reusing it for the replacement document. Parent document load waits for child
navigation and module evaluation gates, including the shared parent-load collector.
Module graphs inherit the originating script's `crossorigin` credential mode and
native document origin; `use-credentials` includes the shared HttpOnly-safe cookie
jar and requires credentialed CORS authorization. Every redirect validates CORS and
source policy before the next request, preserves redirect-origin taint, and checks
final JavaScript MIME. Classic scripts honor `crossorigin` and reject non-JavaScript
responses when `X-Content-Type-Options: nosniff` applies; legacy classic MIME behavior
is tested separately. `data:` and `blob:` module URL execution, import assertions/types
and every module fetch option are not implemented.

Response-header CSP is supplied through `PageSession::open_with_response_headers`
or `open_with_browsing_state`; older signatures provide empty headers. Header policies
intersect and head meta policies activate at their parsed script positions. Enforced
contracts include inline nonce/SHA256/SHA384/SHA512 authorization, eval restrictions,
and per-hop script/connect/image/frame/linked-style source checks. Module dependencies
retain the initiating script's policy and nonce. A native policy slot prevents guest
request options from changing the document's policy. Inline style elements and attributes use
private native authorization with parser-position meta policies. Blocked attributes remain in
markup while their effective declarations remain unchanged; CSSOM writes follow Chrome's separate
mutation behavior. The native inheritance cascade filters blocked sources without changing the DOM.
Authored HTML event attributes compile through V8's native function compiler with
Document/form/element scope extensions and script-src-attr authorization; assigned
function handlers retain their separate behavior. Redirected module completion reports
the initiating requested URL while its source and relative imports use the final URL.
Supported report-only policies, native violation events/HTTP reports, Trusted Types,
SRI and bounded form-action admission are integrated. Strict-dynamic and broader
navigation/form/worker/media directive behavior remain incomplete; diagnostics
retain unsupported/partial policy paths.
The HTML tree is parsed before hydration, so incremental parser races are not covered.

`PageSession::hydration_diagnostics()` reports real host-budget termination,
partial open errors and encountered unsupported CSP features independently of guest
globals. Its `progress.phase` identifies the last entered host lifecycle gate;
`progress.completed_pumps` counts completed post-load pump iterations. These describe
the initial open operation, including after a watchdog interruption. Repeated
snapshots preserve that initial diagnostic. Reaching the500-pump cap does not
establish global idle. Callers can inspect diagnostics before using a partially
hydrated DOM. Optional private Inspector CPU profiles and their acknowledged
sampling setup are described in the
[detection harness](../harness/detection/README.md). BrowserStorage can be retained by the owning browsing session
through the browsing-state APIs rather than shared globally across runtimes.

Classic scripts with genuine module-syntax parse failures retain a separate
compatibility queue for Turbopack bundles. That path uses `load_side_es_module` +
`mod_evaluate` and preserves its historical classic `currentScript` behavior. It
is not the standards path for explicit module elements.

Offline coverage lives in `tests/module_lifecycle.rs`, `tests/frame_modules.rs`
and the shared `harness/detection/modules.mjs` fixtures (`--evaluation`, `--frames`),
`script-credentials.mjs` and `csp.mjs`. The evaluation fixture gates a TLA fetch
on DOMContentLoaded, exercises actual parse/runtime errors and cached graphs, and
compares native completion to a fresh local Chrome. Historical ESM tests remain.

**Historical Next dev-build diagnosis:** the observations below predate the current
script pump. It now routes classic sources with genuine module-syntax parse failures
to the module loader; these older failures are not a claim that ESM source chunks
are still universally skipped.

- `next dev --turbopack` serves classic `<script src>` chunks whose CONTENT has
  top-level `import`/`export` → the then-current `__execScriptEl` skipped them (they're not
  `type=module`, so the module pump doesn't claim them either). Routing src chunks
  with import/export to the module pump is the next step.
- `next dev` (webpack, no turbopack) serves classic `webpackChunk.push` chunks (run
  fine), but: the Next **dev-tools inject CSS inside a `<script>`** (`.nextjs-data-
  copy-button{…}`) which evals as JS → `SyntaxError: Unexpected token '.'` (caught,
  noisy; skip CSS-bodied scripts), and the dev page still hydrates 0 fibers (≈35
  nodes) — dev SSR + chunk-execution quirks beyond the ESM foundation.

So the ESM foundation is in, but "boot the dev build headlessly for a named frame
at the empty commit" needs: (1) route src ESM chunks to the module pump, (2) skip
CSS-bodied `<script>`s, (3) get the dev app to actually execute its chunks. That's
the continuation.

## Turbopack dev: entry runs, flight delivers — RSC client-ref resolution is the wall

Status after the currentScript + `__name` fixes (commit "make turbopack dev entry
execution work"): on `next dev --turbopack`, the App Router boot now gets FAR:

What works (verified through the shim against the live payroll app):
- All ~36 turbopack chunks register (incl. the ESM vendor chunk, now keyed by its real
  src via the module-pump `document.currentScript` fix).
- The runtime entry instantiates: `window.next` = `{version, appDir, turbopack, router}`,
  `__REACT_DEVTOOLS_GLOBAL_HOOK__` present, app-bootstrap + app-index run, `hydrate()` is
  reached (`self.__next_s` is undefined → `loadScriptsInSequence` calls hydrate directly).
- `ReactDOMClient.hydrateRoot(document, …)` is reached with NO throw and NO unhandled
  rejection (drain_event_loop's swallow path stays silent).
- The RSC flight stream fully DELIVERS: instrumenting the env ReadableStream showed 45
  streams created, 79 chunks enqueued, **45 closed, 0 errored** — the flight payload and
  all chunk bodies stream + close cleanly. `nextServerDataCallback` is installed as
  `__next_f.push`; rows are buffered then flushed to the controller registered in the
  stream's (synchronous) `start`.

What DOESN'T work — the remaining wall:
- Nothing COMMITS to the DOM. After render: 0 `[data-testid]` elements, 0 React fibers,
  no `__reactContainer$`-style marker enumerable on `document` (note: expandos DO persist
  on document/html/body — see `expando_properties_persist_on_document_and_root_nodes` —
  and a plain `hydrateRoot(document, <App/>)` DOES commit + stay interactive — see
  `react_document_root_hydrates_and_commits`; so the binding is fine).
- So the React root SUSPENDS and never commits. `document.body.textContent` is dominated
  by the flight `<script>` rows (the 400KB+ is flight payload text, not rendered DOM);
  "wizard text" matches were false positives from the flight strings.
- The flight ROOT model immediately references client components (`8c:["$","$L1f",…]`
  where `1f:I["…/next/dist/client/…"]` is a client-import reference — providers/layout).
  The suspension is in **client-reference resolution**: the bundled
  `react-server-dom-turbopack` client resolving `I[moduleId, [chunks], name]` rows into
  real modules. Because the refs are at the top of the tree, the whole root suspends →
  zero commit.
- This is NOT wizard-specific: settings + off-cycle authed pages fail identically (10s+
  `getByTestId(...) not visible` timeouts). Only the unauth landing (smoke, no hydration)
  passes. So authed interactive hydration is broadly blocked on turbopack dev by this one
  thing.

Next focused step: make the flight client's `preloadModule`/`requireModule` resolve
client refs in the headless turbopack runtime. The chunks ARE preloaded + registered, so
`__turbopack_context__.r(id)` should resolve and `loadChunk(url)` should hit an
already-resolved resolver — instrument the bundled flight client's resolve path
(`createFromReadableStream` lives in the next-client chunk; refs go through the turbopack
context, not free `__turbopack_require__` globals) to find which call parks forever.

## SOLVED: App Router hydrates headless on turbopack dev (`document.location`)

The RSC-flight wall (above) was ONE missing global: **`document.location`**. The env
defined `window.location` but not `document.location` (a browser invariant —
`document.location === window.location`). Next's DEV RSC flight client replays server
console entries; `resolveConsoleEntry → buildFakeCallStack → findSourceMapURL` reads
`document.location.origin`, which threw `Cannot read properties of undefined (reading
'origin')` INSIDE `processFullStringRow`. That abort killed the entire flight stream
parse — so the React root suspended forever, silently (the throw became a rejected flight
chunk; no console error).

The hunt that pinned it (all reproducible via the diag specs): flight stream fully
delivers (48 reads, all rows) → 17 client refs all resolve (`resolveModuleChunk` ×17, 0
errors) → but only 6 components render → dumping `response._chunks` showed **2 rejected**
chunks → their `reason.stack` pointed at `findSourceMapURL` reading `.origin`.

Fix: one line — `def("location", () => globalThis.location)` (runtime.rs). Impact on the
live payroll wizard route through the shim: **0 → 488 React fibers / 1993 elements**; the
shell, nav, sidebar hydrate; login works; all `:3001` data fetches fire (`/auth/me`,
`/payroll-runs/:id/roster`, `/leave-balances`, `/employments`, …) and the wizard renders
("Step 1: Employees & Leave"). Guarded by `document_location_mirrors_window_location`.

### Remaining (NOT hydration — interaction/seed long-tail)
With hydration fixed, suites that were 0/N on the flight wall now partially pass
(off-cycle 1/2, company-settings 4/14, …). The remaining wizard failures are:
- `waitFor(state=hidden/visible) timed out` on modal open/close after a submit click
  (e.g. add-to-payroll modal not detected closing) — likely a MUI Dialog transition /
  visibility-detection refinement in the shim. Highest leverage (modals are everywhere).
- `403 Missing required permissions` (e.g. `deduction:read`, `/expenses`) — the e2e seed
  admin lacks some grants (seed/permission issue, not the engine).
These are per-interaction, not the structural hydration blocker — that is solved.

## Interaction tier: click fixed for non-portal; PORTAL onClick is the open blocker

After hydration was solved, the e2e failures moved to interactions. Two findings:

1. **Click made browser-accurate** (shim `index.mjs`): fire pointerdown→mousedown→focus→
   pointerup→mouseup→click, pointer events FIRST (MUI v7/Radix gate on them), and focus
   the target only when mousedown wasn't preventDefault'd (MUI Select/Autocomplete
   listboxes preventDefault mousedown to keep input focus; we were focusing the option
   <li tabindex=-1>, blurring the input, and clearOnBlur discarded the selection).
   Result: company-settings 4/14 → 7/7.

2. **Portal onClick does NOT dispatch** (OPEN — ignored repro `portal_element_onclick_
   dispatches`). A click on a React `createPortal`'d element under `hydrateRoot(document)`
   never fires its onClick. Diagnosis: React attaches delegated listeners per container in
   `completeWork` (HostPortal → `listenToAllSupportedEvents(containerInfo)`), marking each
   container with `_reactListening<rand>`. In this env the MUI portal container divs end
   up WITHOUT that marker (the autocomplete options' chain: li[option]→…→div→div→BODY; the
   intermediate divs have no marker; only `body` does). React's root-container (document)
   listener by design SKIPS portal targets (the `isMatchingRootContainer` walk returns
   early, expecting the portal container's own listener to handle it). With no effective
   listener on the option's portal path, the synthetic onClick is never dispatched —
   confirmed in the live app (MUI `handleOptionClick` never runs; `event.currentTarget`
   index never read) and in the minimal repro fixture. Keyboard selection (ArrowDown+
   Enter) DOES work (it goes through the input, which is dispatched normally), proving the
   gap is portal click-dispatch specifically. This blocks autocomplete/dialog-heavy suites
   (payroll wizard). Next: get React's per-portal-container listeners to attach + fire in
   the headless env (or have the root listener dispatch to portal targets).

## Interaction + parity tier: SOLVED (2026-06, clean-staging side-by-side)

With hydration solved (above), the work moved to making authed journeys drive + assert the
SAME as real Chrome. Run against **clean staging** (payroll-app + flux-apis both at
`origin/staging`, the flux-apis backend clean): **login works, payroll-wizard 5/5, invites
26/26, auth-guards 32/38, smoke 1/1**. The fixes below are each TDD-guarded (render-crate test
and/or shim `surface.test.mjs`) and committed on `fix-login-rsc-nav`.

### Engine/shim fixes that greened real suites
- **Portal/getBy live resolution** (`runtime.rs __tcGetBy`): getByRole/getByText/getByLabel
  resolve in the LIVE isolate, returning each match's live `querySelectorAll('*')` index, so
  the shim dispatches on the SAME node it matched. Fixed the portal onClick blocker (MUI
  Autocomplete option selection commits). The earlier snapshot-index approach reordered
  portal'd nodes → wrong target.
- **Click = browser-accurate** (`index.mjs`): pointerdown→mousedown→focus(only if mousedown
  not preventDefault'd + focusable)→pointerup→mouseup→click.
- **Modal close after mutation**: virtual-timer budget made RELATIVE per drain
  (`__resetTimerBudget`) — an absolute budget killed late short timers (a closing MUI Fade's
  ~195ms exit), so `waitFor(state:'hidden')` hung. Plus `is_visible` treats effective
  `opacity:0` (and `display:none`/`visibility:hidden` ancestors) as hidden, so a faded-out
  modal reads hidden. `next/dynamic` lazy modals: `drain_to_quiescence` now runs runtime-
  injected `<script>`s (kicks `__hydrate` each round).
- **waitForResponse staleness** (`index.mjs`): it returned ANY buffered response, so a loose
  predicate (`url.includes('/leave-requests/bulk-upload')`) grabbed the prior step's template
  GET instead of this step's POST. Now tags each drained response with an action sequence
  (`_actionSeq`, bumped per interaction) and only accepts one from the current action or later
  — matching Playwright's "responses after the call" semantics. Greened the vacation bulk
  upload (→ payroll-wizard 5/5).
- **Download capture + ElementHandle + polling waitForFunction**: a real `URL.createObjectURL`
  registry + capture-phase click listener record `<a download>` clicks over blob URLs into
  `__downloads`; `page.waitForEvent('download')` → a Download with `path()/saveAs()/
  suggestedFilename()`. `Locator.elementHandle()` + a polling `page.waitForFunction(fn, handle)`
  that resolves the handle to the live element. Backs CSV-template download → fill → upload.
- **CSS `:hover` reveal** (`runtime.rs __tcApplyHover` + shim hover): turbo-dom's cascade has
  no pointer state, so a menu shown only by `.trigger:hover .menu{visibility:visible}` (incl.
  emotion's nested `&:hover .menu`) stayed hidden and `waitFor(visible)` hung. We mark the
  hovered chain with `[data-tc-hover]`, FLATTEN stylesheet + `<style>` rules resolving nested
  `&`, rewrite `:hover`→`[data-tc-hover]`, and apply the matched rules' decls INLINE (survives
  serialize → both getComputedStyle and rtdom's cascade see the reveal). Real case: the app's
  UserMenu (overridden to open on hover) — logout lives in a `&:hover .user-menu` dropdown.
- **Locator scoping** (`index.mjs` + `napi get_by` + `runtime.rs __tcGetBy/__tcResolveScoped`):
  `card.getByTestId('x')` and `card.getByRole(...)` delegated to the page and matched the whole
  document. Now scope to the parent's subtree (descendant matching). And `steps.nth(i).getBy*`
  carries an nth-aware scope CHAIN of `{sel, idx}` resolved by `__tcResolveScoped` (walk picking
  idx per level, then match the leaf) — a CSS-concat selector can't express "the i-th match's
  subtree". Greened the payroll-approval-chain (2 steps configured independently) + the UserMenu
  logout (desktop vs mobile twin).
- **`is_visible` ignores aria-hidden** (`visible.rs`): Playwright's `isVisible()` is purely
  CSS/layout — a decorative MUI SVG icon carrying a test-id (icons are aria-hidden) is still
  visible. aria-hidden stays handled in `ax.rs` for role/name queries. → auth-logout green.
- **Locale parity** (`index.mjs`): the shim no longer force-seeds `NEXT_LOCALE=en-US`; with no
  cookie next-intl resolves the app default (es-MX), matching real Playwright (which sets none).

### Method: resolve engine-vs-not by side-by-side (Playwright vs turbo-surf)
Run each failing suite in BOTH real Chromium (Playwright, reusing the running dev servers) and
turbo-surf, one suite at a time. **Playwright PASS + turbo-surf FAIL = engine bug** (fix it).
**Both FAIL = not engine** (app/backend/data/test). Note: Playwright's `globalTeardown` deletes
the seed entity, so RE-SEED for turbo-surf after a Playwright run; the turbo-surf runner now has
a matching `globalTeardown` lifecycle for the same per-run isolation.

### Confirmed NOT-engine (tickets filed) — fail in real Chrome too / data-only
- **Knowledge-base 403s** (FLUX-1332): backend `@RequireAnyLocalRole(FLUX_*)` checks the
  entity-context-FILTERED roles; a flux-admin with an auto-selected entity has FLUX_* stripped
  from `roles` (the PLATFORM flags stay true) → 403. Backend should use `isFluxPlatformActor`.
- **Pay/work-schedule create** (FLUX-1335): the app's `assign()` omits the backend-required
  `effectiveFrom` → 400 → success banner never shows.
- **settings language** (FLUX-1336): the engine renders es-MX on fresh data; the failure is the
  data-seed leaking the `Person` + `language_preference` on user-delete, so a reused user keeps
  a stale en-US. Not a product/engine defect.
- **off-cycle**: `SKIP_ON_REMOTE` tests (CI skips via `E2E_USE_REMOTE_TARGETS=1`).
- **auth-guards delegated-write**: passes 7/7 on a fresh seed; matrix reds were stale grants
  from cross/intra-run pollution (data, not engine).

Net: the engine is solid for authed journeys; remaining e2e reds are app PRs / backend / test
data isolation, not the render or shim. Open engine-suspect still to side-by-side cleanly:
**payroll-config** (turbo-surf hangs ~30s on an `about://blank` fetch; Playwright flaked at
login so the feature step wasn't compared).

### Update — additional shim fixes + ticket resolution

More shim/parity fixes landed after the side-by-side (all in `surface.test.mjs`):
- **`about:blank` navigation** → a no-fetch blank document (was a `builder error for url
  (about:blank)` when a goto/reload hit the net layer; a login helper reloading a blank page
  before navigating then threw).
- **`waitForFunction`** returns the function's VALUE (not a boolean) and works on a static
  snapshot — regression from the polling-handle rewrite.
- **`test.extend` custom fixtures inject** — a test fn with its own extDefs opens its own
  fixture set instead of reusing the base-only shared `_current` (custom fixtures / a `page`
  override resolved to `undefined`).
- **Test isolation:** an env-mapping test restored `TURBO_SHIM_*` with `= undefined` (coerces
  to the STRING `"undefined"`), leaking `testIdAttribute`/`baseURL` into every later context →
  cascaded failures across the serial run. Fixed with `delete`. Surface suite now 51/52 green
  (1 intentional skip). KEY LESSON: a "fails only late in the full run, passes in isolation"
  cascade was test-isolation env leakage, NOT an engine/isolate leak — reproduce in the real
  multi-test run (the shim is robust to 60+ unclosed hydrated sessions in a raw loop).

Ticket resolution (the confirmed not-engine reds — all merged to staging):
- FLUX-1332 (KB platform-role RBAC) → payroll-app#254 (honor FLUX_* platform flags in role
  guards when an entity is selected).
- FLUX-1335 (pay/work-schedule assign missing `effectiveFrom`) → payroll-app#209.
- FLUX-1336 (data-seed leaks Person + language_preference on user delete) → flux-apis#255.
- FLUX-1337 (robust e2e PropelAuth cleanup: bulk + per-run + stale-local-run) → in progress.

payroll-config: resolved as NOT engine — its `openConfig` calls `login()` without first
navigating to the login page, so it fails at login in BOTH Playwright and turbo-surf (a spec
issue); the `about:blank` console error during it is benign (the pending-fetch counter settles
in a finally) and is now a no-fetch blank doc anyway.

## Full re-triage on clean staging (2026-06-23, Cycle 21)

Re-ran the whole suite through the shim against **clean staging** (payroll-app-tc +
flux-apis-staging both at `origin/staging`, the merged FLUX-1332/1335/1336/1337 fixes in):
**73 / 102 pass, 0 skip.** Triaged every red with the side-by-side method (each failing spec
run BOTH through real Chromium and turbo-surf, with a fresh seed+teardown per run so data can't
collide). Verdicts:

| Suite | Verdict | Cause |
| --- | --- | --- |
| auth-guards delegated-write | pollution-only | green 7/7 in isolation; full-run reds were cross-suite grant pollution |
| knowledge-base | NOT engine (locale) | app defaults es-MX, suite asserts English (`getByLabel('Title')`=Título); #255 removed the seed-leaked en-US that masked it |
| company-settings/pay-schedule | **ENGINE** | filter scope dropped (below) |
| off-cycle/termination | **ENGINE** | RSC soft-nav query dropped (below) |
| settings/account | **ENGINE** (fixed) | `page.reload()` didn't re-hydrate (below) |
| company-settings/tax-registrations | NOT engine | `waitForResponse('/tax-registrations')` never matches real `/entity-tax-registrations` → FLUX-1341 / payroll-app#212 |
| payroll-configuration | NOT engine | `openConfig`→`login()` never navigates to `/login` → FLUX-1342 / payroll-app#213 |
| payroll-wizard/vacation | NOT engine (backend) | `/leave-requests/bulk-upload` 400 "Failed to parse CSV" — `csv-parse` strict mode rejects benign shapes (stripped trailing col, BOM) → FLUX-1340 / flux-apis#260 |

### Engine fixes (commit `fix(render+shim): two engine bugs found via payroll e2e side-by-side`)

1. **RSC soft-nav dropped the query string.** `__rscNav` recorded only `u.pathname`, so
   `router.push('/off-cycle/new/termination?employeeIds=42')` lost `?employeeIds=` and the
   termination spec's `waitForURL(/…\/termination\?employeeIds=/)` never matched. Now records
   `pathname + search + hash`, stripping Next's internal `_rsc` cache-buster (a hard reload
   carrying `_rsc` returns a flight payload, not HTML). Guard:
   `rsc_soft_nav_preserves_query_and_strips_rsc_param`. **e2e 2/2.**
2. **`Locator.filter()` dropped the scope for child locators.** `.filter({hasNotText})` drops
   `_selector` (a filtered set isn't one CSS selector), so
   `cards.filter({hasNotText:x}).first().getByTestId('y')` resolved the child against the
   UNFILTERED set → the pay-schedule delete-409 guard clicked the wrong card's edit-toggle and
   the delete never revealed. Now a serializable filter spec (`hasText`/`hasNotText`) rides the
   scope chain (`_scopeSel` survives filter); `__tcResolveScoped` applies it before indexing.
   Guard: `scoped_resolve_applies_filter_before_indexing`. **e2e 2/2.**

Both validated against the real app (rebuilt napi addon, merged the three not-engine PRs
locally, reran). Render suite 56/56; shim surface no new regressions.

### Engine fix 3 — settings `page.reload()` didn't re-hydrate (commit `fix(shim): reload(networkidle) re-hydrates`)
The language-survives-reload step did `page.reload({waitUntil:'networkidle'})` then read the
language select — which read `""`. Traced (url probes through the live session): before the
reload `page.url()` was correct (`…/settings/personal-profile`), but **after** the reload the
isolate's `location.href` was `about://blank`, cookies empty, the select absent. Root cause:
the shim's `reload()` **ignored its `opts`** — unlike `goto`, a `networkidle` reload never
re-opened a live session, so the reloaded doc stayed the raw un-hydrated shell (empty select →
`""`). Fix: `reload(opts)` mirrors `goto` — `_navigate(this._url)` then, on
`waitUntil:'networkidle'`, `_openLiveSession()` (re-hydrate). The whole settings journey
(language → name → address → email) now passes **2/2**. Guard: surface test
"Page.reload({waitUntil:'networkidle'}) re-hydrates the live SPA".

### Still open (next layer — the original failures masked these)
- **tax-registrations (residual):** with the predicate fixed the save now succeeds, but the
  confirm dialog's `waitFor(state:'hidden')` times out — a MUI Dialog close not detected after
  the mutation (engine modal-hide family) OR an app gate; needs a PW recheck with the fix.
- **vacation (residual):** with the CSV parse fixed the upload now reaches date validation and
  fails `Invalid date format for Start Date. Expected YYYY-MM-DD` — a next-layer test/backend
  date issue, not the parser.

Tickets this round: FLUX-1340 (leave CSV parse → flux-apis#260), FLUX-1341 (tax-reg predicate →
payroll-app#212), FLUX-1342 (payroll-config login nav → payroll-app#213); FLUX-1338/1339 are
unrelated product features filed the same day.

## Public-site rendering triage (2026-07-08, Cycle 22) — wikipedia / google / nike

Historical rendering log. Google blank-page hypotheses below were superseded by the
`noscript` CSS isolation fix in v0.4.2 (CHANGELOG). They do not explain the current
search challenge retry failure. See [current detection findings](../harness/detection/FINDINGS.md).

Goal: render public homepages via the **raster** screenshot path (`hydrate()` → collect
css/images → `screenshotWithAssets`), not the authenticated SPA path. Harness scripts are in
the session scratchpad (`wikihydshot.cjs`, `hydshot.cjs`, `googconsent.cjs`) — fetch the DOM
in Chromium, `native.hydrate(rawHtml, finalUrl, "", UA)`, then screenshot the hydrated HTML.

`hydrate()` **works on all three** (Wikipedia 944ms, google ~1.9s, nike 7.5s — the JS ran,
DOM grew). The gaps are NOT hydration failures; they are rendering/reveal issues:

### Wikipedia — SOLVED, ships faithfully
Static OR hydrated both render like Chromium after the 0.2.7 engine batch (mask-image icons,
self-ref `var()`, pseudo-element non-leak, flex fixes, table min-content, `;`-in-url parser).

**TOC caret — root cause found + FIXED (Cycle 23, raster-side).** The caret
(`.mw-ui-icon-wikimedia-expand`) is NOT dropped from the box tree — the empty icon span lays
out as a 20×20 box. It vanished because its `mask-image` is an inline **`data:image/svg+xml`
URI**, and the fetch step skips `data:` (nothing to fetch), so the mask resolved to nothing →
`prepend_mask_image` in html2pdf bails at `probe_source` (resolver miss) → **no `Image`
fragment emitted** → the box paints transparent. The committed mask test only ever covered an
*external* `url(caret.svg)` asset, so this gap was invisible. Fix: the raster now decodes
`data:` image URIs itself (`add_data_uri_images` in `image_paint.rs`) — quote-aware `url()`
scan keyed EXACTLY as html2pdf's `url_token` (retains the CSS `\"` escaping), base64 + plain
(`%23`/`\"`-decoded) payloads — and merges them into `decoded` so both the layout resolver
(`png_map`) and the paint pass find them. General win: every inline-SVG mask/background now
paints (nike/others too), not just Wikipedia. Covered by
`datauri_mask_image_icon_paints_without_a_fetched_asset` + unit tests.

**Remaining caret polish (layout-engine, html2pdf — deferred):** the caret now paints as the
raw down-chevron `⌄` mid-row; Chromium shows `>` in a left gutter. Two html2pdf gaps: (a) the
collapsed toggle's `transform: rotate(-90deg)` isn't applied to the mask (down-chevron not
rotated to point right), and (b) the `.vector-toc-toggle` sits in the text flow instead of the
row's left gutter. Both are `transform`/positioning work in the separate repo — overlaps the
nike "overlay chrome + transforms paint weakly" item. Non-caret icons all render.

### Box-shadow / overlay card chrome — IMPLEMENTED (Cycle 23), release-pending
Overlay cards/modals read as flat rectangles because turbo painted no `box-shadow`.
Added end-to-end: **turbo-html2pdf** (`feat/css-positioning`) now parses `box-shadow`
(`[inset]? <ox> <oy> <blur>? <spread>? <color>?`, comma layers → first kept,
`currentColor` default) into a `BoxShadow` on `FragmentContent::Box`; **turbo-surf
raster** paints it — an outer shadow stamped as a rounded-rect silhouette, 3-pass
separable box-blur (≈ Gaussian, σ≈blur/2) in PNG, `feGaussianBlur` filter in SVG,
composited behind the box. Proven on a synthetic consent-card (soft drop shadow +
rounded corners + tinted button shadow, reads as a real modal). Covered by
`box_shadow_paints_a_soft_offset_shadow_behind_a_card` (raster) + `box_shadow_tests`
(html2pdf parser). **Blocked on landing in turbo-surf:** raster consumes it via a
TEMP `[patch.crates-io]` → needs an html2pdf **0.2.8** release, then bump the dep +
drop the patch. Inset shadows, multiple layers, and `transform`ed shadows are v1-out.
Deferred: gradients + CSS `transform` (the Wikipedia caret rotate + nike hero) are the
next engine features on the same fragment-attribute pattern.

### Linear-gradient backgrounds — IMPLEMENTED (Cycle 24), release-pending
`background`/`background-image: linear-gradient(...)` now paints (hero sections,
buttons, cards). **turbo-html2pdf 0.2.9** parses it into a `LinearGradient` (angle +
positioned stops) on `FragmentContent::Box` — angle (`<deg>`) or `to <side>/<corner>`
(default `to bottom`), `rgb/rgba/#hex/named` stops with optional `%` positions
(unpositioned spread evenly), nested-comma-safe, `radial-`/`conic-` skipped;
**turbo-surf raster** paints it via tiny-skia `LinearGradient` (PNG, start/end points
across the box per the CSS angle, clipped to the rounded rect) + native
`<linearGradient>` (SVG). Proven on a demo (to-right, 135° diagonal, 3-stop
red→green→blue with `%`, and a purple gradient card composited with box-shadow +
border-radius + text). Covered by `linear_gradient_background_paints_left_to_right`
(raster) + `gradient_tests` (html2pdf parser). Same release-coupling as box-shadow:
needs html2pdf **0.2.9** published, then bump the dep. **CSS `transform` still deferred.**

### React-18 hydration — GENERAL fixes (Cycle 26): defer order + env globals + `<style>.sheet`
Nike's app (like every React-18 SSR site) wasn't hydrating AT ALL — three general
engine bugs, all fixed:
1. **`defer` script order.** `__hydrate` ran `<script>`s in raw `querySelectorAll`
   DOM order, ignoring `defer`. Sites put their app's `<script defer>` EARLY in
   `<head>` but the framework vendor (`react.js`, non-defer) LATE near `</body>`
   (Nike: `main.js` @byte 88k defer, `react.js` @608k non-defer). Raw order ran
   `main` first → `React is not defined` → nothing hydrates. Fix (`runtime.rs`
   `__hydrate`): non-defer scripts first (document order), then `defer` scripts.
2. **Missing Web/DOM interface globals.** `x instanceof Response/Request/NodeList/
   HTMLCollection/CSSStyleDeclaration/Attr/CDATASection/MutationRecord/...` threw
   "Right-hand side is not an object" when the global was undefined — and one such
   throw in a shared webpack helper cascades to kill React hydration for the whole
   page. Fix: real `Response`/`Request` classes wired to `fetch` (`runtime.rs`);
   the DOM/collection/CSS interface constructors + real `Audio(src)` + full-shape
   `Worker`/`SharedWorker` in the versioned env (turbo-test browser_env → 0.3.10).
   Result: **instanceof errors 38 → 0; hydration completes** (DOM grows +376KB).
3. **`<style>.sheet` on SSR style tags** (turbo-test 0.3.9): emotion/styled-components
   speedy-mode `styleEl.sheet.insertRule` on server-rendered `<style>` tags now works,
   so client-injected CSS is captured (Nike styleChars 483K → 894K through the pure
   hydrate path — no Chromium-CSSOM shortcut needed).
   This was a historical serialized-style measurement. The current CSSOM correctly
   keeps `insertRule` changes separate from `style.textContent`; those changes are
   available through the live sheet but are not automatically included in serialized
   HTML or a screenshot made from that HTML.

**Nike still not a clean screenshot:** hydration now RUNS but hits a Next.js
client-side exception → it navigates to `/_error` and the homepage re-renders blank
(the SSR hero content survives in the page data, `"A LENDA CONTINUA VIVA"`, but the
error boundary trips). Plus the OneTrust preference-center panel renders in-flow.
Those are Next.js/consent-state specific, past the general engine gaps (now fixed).
Also: CSS `transform` (0.2.11) now paints — carousel slides translate, the Wikipedia
caret can rotate — so slides no longer pile at one spot.

### Nike — images FIXED (Cycle 25): AVIF `Accept` + image `%` sizing
Nike's homepage rendered text-only, zero imagery. Two independent root causes, both
now fixed:
1. **AVIF.** Nike's `f_auto` CDN serves **AVIF** to a Chrome UA that accepts it (the
   impersonate fingerprint's `Accept` advertises `image/avif`). turbo's `image` crate
   has no AVIF decoder → every hero/product image silently dropped. Fix: `fetch_bytes`
   (napi) now sends an image `Accept` **without** `image/avif`
   (`image/webp,image/png,image/jpeg,…`), so the CDN falls back to WebP, which turbo
   decodes. General: fixes every `f_auto`/Cloudinary site.
2. **`width:100%` collapse.** Even fetched, the images were 0×0 — `size_replaced`
   resolved `%` against a 0 basis (html2pdf 0.2.10 fix). Together the Ronaldo hero,
   Mercurial boots, and tennis imagery now render (verified).

**Remaining nike:** the cookie-modal (nike's own `cookie-modal-base`, `position:fixed`
overlay, `display:block` in Chromium too) renders **in-flow** at the top instead of as a
fixed overlay — turbo doesn't take `position:fixed` out of flow for the paint, so a wall
of policy text pushes the page down. Fixed/overlay positioning is the next gap.

### Google — box-tree DROP via cascade mis-computation (Cycle 25, NOT fixed)
Still blank. Ruled out, in order: JS-reveal (raw==hydrated blank), overflow/clip
(forcing `overflow:visible;height:auto` + bright bg on `.RNNXgb` still paints nothing),
and flex-height collapse (added flex `min-height→taffy` in 0.2.10; `.RNNXgb` has
`min-height:50px` but the search box STILL doesn't appear). Since forcing everything
visible + a background on the search box yields nothing, the box is **not in the box
tree** — an ancestor computes to `display:none`/`opacity:0`/`visibility:hidden` in
turbo's cascade (which Chromium overrides). Google ships obfuscated hide classes
(`.MDfoTd{opacity:0}`, `.VH47ed{visibility:hidden}`, `.gb_*{display:none}`); turbo is
picking a wrong winning declaration (specificity/order) for one search-chain ancestor
and dropping the subtree. Next: dump turbo's computed `display/opacity/visibility` per
search-chain class to find the mis-cascaded rule. Single-site, deep — deprioritized.

### Google — NOT a JS-reveal bug (hypothesis disproven, Cycle 23) [see Cycle 25 above]
Consent dismissal now works generally (`consentshot.cjs` clicks `#L2AGLb` /
`#onetrust-accept-btn-handler` / text-matched "Accept all" across main + child frames).
But google's homepage still renders **fully blank** in turbo — and crucially: rendering
the Chromium-**revealed** DOM with NO hydrate is byte-identical to the hydrated render
(`goog-raw.png` == `goog-hyd.png`, both 39842 B, 102 `display:none` each). So the earlier
"turbo re-hides via JS-reveal" theory is **wrong** — turbo collapses the page independent
of JS. Chromium trace of the search box `textarea.gLFyf` (visible at 344,377 446×50) shows
its ENTIRE ancestor chain to `<body>` is visible — `gLFyf/a4bIc/SDkEP/RNNXgb`(flex) →
`A8SBwf`(block,rel) → form → `o3j99 ikrT4e KEY6ib`(block) → `L3eUgb`(flex,the viewport
centering column) → body — **no `display:none`/`opacity:0`/`visibility:hidden` anywhere**.
So turbo either (a) mis-cascades a `display:none` onto this chain (`.o3j99` base is shared
with the hidden `.o3j99.qarstb` consent div — a specificity/order bug would hide both), or
(b) flex-collapses the full-viewport `L3eUgb` centering column. Next step is an html2pdf
`zz_` probe with google's CSS to see which class computes `display:none` / which flex box
sizes 0. Single-site, unbounded; lower ROI than the general engine features above.

### Google — content lays out, but google's OWN css hides it (JS-reveal wall) [SUPERSEDED — see above]
- With EXTERNAL css only → 44 text lines lay out. With external + INLINE (head) styles → **0**.
  So a head `<style>` rule hides everything.
- The raster uses `collect_style_blocks` (raw regex, grabs `<head>` styles too); a plain
  `layout_html` probe uses `collect_style_css` (html5ever **drops `<head>`**) — that's why a
  naive probe "renders" and the real raster is blank. **When diagnosing, always apply the head
  styles.**
- Traced the consent text ("continuar"/"continue"): its ancestors are
  `<div class="o3j99 qarstb"> display:none` and another `<div> display:none`. Google ships the
  modal in `display:none` containers and reveals it via JS (portal/clone/class-toggle). turbo's
  hydration runs the JS but the serialized DOM still has `display:none` → blank, consent state
  AND homepage state (both `turbo≈39KB` = empty). Obfuscated hide classes seen:
  `.MDfoTd{opacity:0}`, `.VH47ed{visibility:hidden}`, `.KZMqi{left:-9999px}`, `.gb_*{display:none}`.
- **Next agent:** find WHICH class/attr google's JS toggles to un-hide the modal (is it an
  iframe? a body-appended portal? a class flip on `.o3j99`?), and either (a) make the render tier
  reproduce that reveal, or (b) as a harness hack, force the reveal class before screenshot. This
  is per-site reverse-engineering of a JS-reveal app, not an engine cascade bug (turbo applies
  `display:none` correctly, same as Chromium pre-reveal).

### Nike — Emotion CSS-in-JS WORKS; barren = consent overlay + weak overlay chrome + images-behind-modal
- Nike's styling is **258 inline `<style>` blocks / 991 `.css-xxxxx` Emotion rules** (not
  external). CSS-in-JS is **already supported** — proved by a committed harness
  (`turbo-surf-raster/tests/screenshot.rs::css_in_js_inline_style_blocks_apply`: hashed classes +
  `>` combinator + `@media` all cascade). So barrenness is NOT a cascade gap.
- Nike serves a **cookie-consent wall** (Portuguese, geo/bot-triggered). Chromium shows it as a
  styled white card over the nav+hero; turbo shows the consent TEXT unstyled → the overlay
  **card chrome** (position:fixed backdrop, box-shadow, border-radius, transforms) paints weakly
  and the modal doesn't read as a card. Product images render behind the modal.
- Images: nike uses `<img loading=lazy data-landscape-url=… data-portrait-url=…>` (no `src`).
  `delazy_images`/`LAZY_IMG_ATTRS` **already handles** `data-landscape-url`/`data-portrait-url`
  → they DO get a `src` + are fetched (hydshot fetched 82). The "no images" was the consent
  overlay covering them. A `layout_html`-only probe shows 0 images because it has no resolver —
  not a real signal.
- **Next agent:** to get nike's real homepage, dismiss the consent wall in the Chromium capture
  (click Accept) BEFORE `hydrate()` (google's `#L2AGLb` pattern), OR pass consent cookies. Then
  the gaps become: overlay/fixed-position card chrome fidelity + gradient/box-shadow painting.

**UPDATE (Cycle 23) — nike homepage now renders content.** `consentshot.cjs` dismisses the
wall (`#onetrust-accept-btn-handler` / text "Accept all", main + child frames) before capture;
turbo then lays out the real homepage (nav + hero copy "O TEU JOGO, AS TUAS REGRAS" + product
sections "Nike Football / A LENDA CONTINUA VIVA" etc.). box-shadow (above) covers the card
chrome. Remaining nike gaps are its own deep layout: hero/product IMAGES stay sparse (lazy
`data-*-url` fetch fine, but the absolute/positioned hero grid doesn't place them where Chromium
does) + gradient backgrounds. Those are general engine features (positioning + gradient), not
nike-specific.

### Gotchas for the next agent
- **macOS has no `timeout`** — `timeout <cmd>` errors "command not found" and looks like a hang.
  Use `run_in_background` + poll, or `gtimeout`. Cost me multiple false "it hangs" diagnoses.
- **Rebuild the addon with `--features impersonate`** (anti-bot) and verify the dylib timestamp
  moved — `cargo build -p turbo-surf-napi` produces the dylib that index.js copies to
  `turbo-surf.dev.node`; a stale `.dev.node` silently serves old code.
- **Local turbo-html2pdf-core:** now released as **0.2.7** (crates.io). No `[patch.crates-io]`
  needed anymore. If iterating on the engine again, add the patch back AND bump the local
  workspace version to satisfy the raster's `^0.2.x` req, else cargo silently uses the registry.
- Diagnostic probes live as throwaway `tests/zz_*.rs` (color-inject `background-color:#f00
  !important`, walk `Fragment` boxes / `StyledNode` computed styles). Run in RELEASE
  (`cargo test --release`) — debug full-page layout is too slow.


`DOMParser.parseFromString(..., "text/html")`, `Document.parseHTMLUnsafe`, and
`Document.parseHTML` now parse into independent native Trees in the caller's V8 realm.
The resulting HTMLDocument has no Window, complete readiness, inert scripts, and an
empty cookie view. DOMParser captures the active document URL; the static Document
methods use about:blank. `importNode` copies supported native elements, attributes,
text, comments, processing instructions, doctypes and fragment subtrees across
these trees without evaluating markup or moving the source. Supported live control
values and private nonce state are copied as well. `adoptNode` instead preserves
the existing wrapper identity while changing its owning tree.

ProcessingInstruction has a real native CharacterData node, target validation and
clone/import/adoption support. HTML documents reject `createCDATASection` with
NotSupportedError. Their XML metadata has measured null defaults, mutable
`xmlStandalone` and a version-1.0-only `xmlVersion` setter; these compatibility
properties do not implement XML document parsing.

`parseHTML` filters the parsed namespace tree using the default policy captured from
Chrome154's public `Sanitizer.get()` interface, rejecting disallowed elements,
attributes, comments, and decoded JavaScript URL schemes. The implementation is
limited to that default policy: XML MIME types, custom sanitizer configurations,
template-content imports, and unsupported node kinds throw NotSupportedError.
The [HTML parsing and sanitization algorithms](https://html.spec.whatwg.org/multipage/dynamic-markup-insertion.html#html-parsing-methods)
and `harness/detection/document-parser.mjs` define the measured behavior and reusable
browser comparison; this is not a claim of complete Sanitizer API coverage.

### Native ownership and mutation delivery

The binding uses weak native lookups with private V8 Document/subtree edges.
Crypto's native objects trace their actual JavaScript edges through cppgc. The
workspace consumes a reproducible Rusty V8 150.4.0 ownership teardown fix; see
[`vendor/RUSTY_V8.md`](./vendor/RUSTY_V8.md). This does not upgrade the default
engine to Chrome 155.

MutationObserver records are native semantic batches delivered through a V8
notification microtask. A Document-private consumer receives mutations after
observer fanout; scalar script preparation flags retain native node history across
cloning/adoption and retired/inert documents. Resource execution integration and
full observer lifecycle coverage are still in progress. Raw native heap retention
diagnostics are available to lifecycle tests through the detection harness.


## Native resource preparation checkpoint (2026-10-14)

Resource consumers claim the canonical native PreparedScript state and original
owner document. Guest getters, Array iterators, JSON hooks and public expando
markers do not decide whether code executes. The bootstrap captures parser script
wrappers once; native mutation records supply later real preparation candidates.
This avoids reviving CharacterData-only, parser-inert or already-started scripts on
later PageSession calls. Retiring a frame disables its document resource consumer.

Inserted inline classics compile and run directly in the owning V8 context before
insertion returns. Native currentScript writes are restored after execution and
budget interruption. Ordered dynamic external classics start their downloads
concurrently and wait for their predecessor only before execution. Native queued
PromiseResolver handles replace JS completion maps; interruption performs only
native state restoration/rejection while temporarily clearing termination.

COEP headers use RFC8941 structured field parsing. require-corp/credentialless and
CORP cover general no-CORS fetches, scripts, stylesheets and images, while CORS and
SRI retain separate admission. Aboutblank/srcdoc documents inherit the parent's
embedder policy. Credentialless transports strip cookies after a cross-origin hop,
even after returning to the original origin. Saved normal Chrome155 controls and
localhost Rust contracts cover direct requests, redirects, resource errors and
frame inheritance. COOP/isolation, navigation embedding compatibility, reporting,
permissions policy and broader parser/resource lifecycle remain open.


CSP base-uri now freezes the accepted base URL at mutation admission rather than
re-querying guest overrides on every read. Document-owned records use captured
native DOM callbacks and native URL/source checks. Head meta CSP changes accumulate
restrictions; body metas have no effect. Original mutation ancestor snapshots
exclude disconnected construction records from premature policy activation.
Parser completion applies pending metas before deferred execution/DCL. Normal
Chrome155 controls measure parser vs compound-fragment order and retained retired
documents; a metadata-only retired consumer updates fallback bases without
reviving native scripts. The canonical HTMLMetaElement property/attribute reflection
fix is required for ordinary httpEquiv/content assignments to reach native state.
Form-action, violation/reporting behavior and wider CSP remain open.


Native isolation is now granted only by secure actual navigation with valid same-origin COOP, compatible COEP and inherited Permissions Policy. The iframe admission path blocks incompatible embedded documents and enforces cross-origin CORP, with genuine credentialless exceptions. Native backing-store snapshots carry agent-group/origin ownership; failed Window/MessagePort deserialization fires trusted asynchronous messageerror. Capability checks cannot be spoofed through public globals. Shared WebAssembly allocation remains available in an ordinary unisolated document, while cloning/messaging is rejected.

Credentialless frames use an ephemeral top-document partition containing a real CookieJar and separate local/session storage backing. Initial navigation, response cookies, document.cookie, fetch, scripts, images and module graphs select that native partition. Credentialless siblings share it; normal frames retain their ordinary state. Native partition teardown reclaims its storage areas. PermissionsPolicy/FeaturePolicy has actual private brands/header parsing and measured defaults; Navigator.permissions query and clipboard reject disabled features.42 top headers,79 iframe admission/delegation cases,8 real shared-memory delivery cases,3 query-effect cases and a network/storage partition control match fresh normal Chrome155. Full popup/reporting/dynamic/deep policy, client-hint delegation and additional controlled APIs remain pending; merged/native155 and live-network validation remain independent.


## Historical validated parity checkpoint (2026-10-16)

This section preserves its original source and test counts. The current
messaging/layout/DOM follow-up and pending delivery gates are recorded in
[PARITY-GAPS.md](../harness/detection/PARITY-GAPS.md); these earlier results do not
certify later changes.

Production source at `26bd0f4` vendors committed canonical turbo-test `1e93cae`
through the normal vendor script. Public DOM 0.5.7 candidate `fb60e6f` and layout 0.3.8
candidate `9d7f02fb` remain distinct from registry releases 0.5.6 and 0.3.7. The stock
engine is V8 15.0.245.2-rusty; the managed native 155 profile uses actual
V8 15.5.35.23-rusty with the matching native callback patches. These are separate
build configurations, not an engine upgrade achieved through UA strings.

Native AbstractRange/NodeRange/StaticRange, original UTF-16 boundaries, composed
shadow geometry, scrolling, point hits and caret controls are integrated.
ElementInternals/CustomStateSet, custom-element admission and form association,
live control collections, RadioNodeList, native labels, validation and FormData
lifecycle have independent behavior, receiver-realm and collection controls.
Genuine top-document text GET/POST form submission uses actual network/cookie
state and policy checks. Child-frame navigation, multipart parsing/encoding and
broader form lifecycle remain explicit follow-up work.

Private native ownership backs Window/MessagePort and genuine stream transfers.
Response/upload streams, cancellation, cache/referrer effects, partitioned and
credentialless DOM storage, CSP/Trusted Types/resource policies and bounded
reporting are integrated. Native compression dictionaries now have a real
partitioned store, discovery, validated negotiation and bounded incremental
Brotli/Zstandard decoding. The independent HTTPS and Chrome controls cover
actual partial delivery and cancellation. Publication timing and encoded Range
behavior remain open; live negotiation alone does not prove live decoding.

Official hardened Deno WebCrypto and native X-Wing keys implement the tested
Chrome 155 key/KEM contracts. Private abort/controller boundaries, timeout-signal
GC ownership and actual native/Window timer ordering have regression coverage.
The human driver awaits its real Promise. XHR identity, descriptors/constants
and private receiver checks match the independent bounded Chrome/HTTP controls.
These implementations do not close broader lifecycle, policy or API behavior.

At this checkpoint the complete workspace commands pass **1,376 default / 1,415 native 155
tests**, zero failures, across 129 component suites each. Default has three
unavailable native-callback ignores plus six generated doctest ignores; native 155
has the six doctest ignores. Both strict all-target Clippy and formatting gates
pass. Rebuilt addons pass the shim/E2E and offline detection gates, with exact
counts, feature sets and hashes recorded in
[VALIDATION-LAST-RUN.json](../harness/detection/VALIDATION-LAST-RUN.json).
Native validation includes the MCP wreq/trust-anchors/Metal/CoreText set and the
NAPI Metal/CoreText features. The shared GPU scene matches within its tested scope.

The checkpoint’s independent 75-interface pair measures 64 original-scope / 142 expanded
missing own members and 374 leaf difference paths. All measured interfaces are
present; this does not certify their full behavior or every browser interface.
[API-INVENTORY.json](../harness/detection/API-INVENTORY.json) preserves exact lists
and descriptors; every residual is tracked in #28. Conditional presence-only
Navigator, database, cache, network and Navigation API fallbacks do not establish
implemented browser behavior.

The October17 source-pinned Google matrix uses fresh native cookies, actual consent and
genuine forms with no Chrome cookie import or fallback. Native fetches `/search` with HTTP200
then follows retry to `/sorry` with HTTP429. The latest fresh retry accepts0/2 native SERPs while clean Chrome reaches7/8
result markers. Instrumented Chrome is challenged; the native rejection signal
remains unidentified.
The latest retry exhausts20s during3/4 native homepage opens, including one clean
baseline and both instrumented passes. Dismiss retains the original open diagnostic;
later navigation budget=false does not erase that failure. See the
[latest sanitized run](../harness/detection/GOOGLE-LAST-RUN.json) and
[findings](../harness/detection/FINDINGS.md) for the current repetitions, budgets,
script diagnostics and baseline/instrumentation distinction. The passing shared
probes do not identify Google's rejection signal.

All broader residuals are itemized in
[PARITY-GAPS.md](../harness/detection/PARITY-GAPS.md) and
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28). Advanced audio stays
in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27); the tested offline
DSP remains implemented. Full Chrome conformance and production release alignment
remain open. Current-head CI is recorded in the PR checks.

### Earlier integration notes

The notes below preserve intermediate source snapshots, focused counts and the
older544180d live capture. Statements about pending integration or a next batch
are historical; the current status and validation links above supersede them.

The earlier candidate includes native AbstractRange/NodeRange/StaticRange,
real Readable/Writable/TransformStream transfers through private ports, composed
shadow geometry, original UTF-16 Range/caret controls, Trusted Types/native155
code-generation admission, parser/HTTP reporting, response/upload streams and
profile-owned cache/storage effects. That checkpoint uses canonical turbo-test
`2e27274`; generated consumer files are copied through the normal vendor script.

Genuine ElementInternals and CustomStateSet now provide native custom-element
admission, form association/validation, live labels, association/disabled/reset
callbacks, default ARIA, closed-shadow access and native `:state()` matching.
Cloning/importing starts fresh state; adoption migrates exact UTF-16 membership.
Private guest-owned records preserve borrowed cross-realm brands and collect
retired realms after their last retained internals, state set or iterator is released.
Both live and exhausted state iterators have independent Chrome lifetime controls.

Native SubmitEvent, FormDataEvent, validation and guarded entry-list construction
are integrated with the runtime's private File/FormData records. Constructor and
submission events use distinct genuine FormData wrappers sharing the actual mutable
entries; listener edits affect submitted data. Metadata is captured before the event,
and form-action CSP admission follows it. Reentrant construction is guarded, canceled
or invalid submissions produce no wire request, and disconnected forms are checked
before validation and again after submit listeners. Top-document text GET/POST
submissions use the actual network profile and replace the response document only
after the prior isolate activation exits. Native tests cover real response
scripts/status, redirect cookies, fragments, cancellation/CSP and retained form/Range GC.
Multipart FormData BodyInit and broader target routing remain unsupported.

Form reflection now includes action/method/enctype/encoding/target/name/acceptCharset/
autocomplete/rel and a live branded relList. Action URL resolution uses the native
host-installed document URL; CSP approval of reflected base URLs remains open, and
wider malformed-URL serialization is unverified. Input/Button prototype placement, control descriptors
and actual text-selection/replacement behavior have measured Chrome controls.
ValidityState's genuine constructor prototype is read-only. The next canonical
batch implements genuine live form controls collections/RadioNodeList and form
elements/length/iterator; production real-frame/GC validation remains separate
from canonical focused results.

Host media evaluation now supplies the measured forced-colors, contrast, color-scheme,
reduced-motion/transparency, pointer and hover capabilities. The last recorded combined
native cohort passed 371 tests before the subsequent Blob/ValidityState descriptor
corrections. The subsequent actual155 renderer/API/forms/media337 cohort passes;
full final consumer gates remain pending.
Standalone canonical validation at `72da6ea` passes 237 native155 and 236 default-engine
contracts plus both all-target strict Clippy gates. These are separate validation scopes.

The public layout dependency is a Git-pinned0.3.8 candidate from producer PR23;
registry0.3.7 is published. The stock production engine remains V8150, while the
experimental155 configuration uses actual15.5.35.23 and matching patched native
callbacks. A newer JS/HTTP profile does not establish native engine alignment.

Navigation activation is private native context state. Trusted pointerdown,
mousedown and key input activate before page handlers run. Genuine form and
Location admission snapshot transient activation for the outgoing request and its
HTTP redirects; later new-document script retries do not inherit it. Public
UserActivation exposes genuine brands and context state, including same-origin
sticky carry and cross-origin reset. The source-pinned localhost Chrome activation
oracle covers these paths and real elapsed expiry. These controls do not prove
server acceptance.

The authored offline-audio adapter uses explicit `std::exp2(float)` to avoid the
platform-dependent unqualified overload. The independent Chrome 176-cell response
grid matches exactly on Linux x86, macOS ARM and Rosetta x86, with NaN bounds and
existing signed-zero equality preserved. Licensed kernels and exact assertions
remain unchanged. Linux Chrome browser audio output and deferred advanced audio
remain outside this bounded proof.

The pushed544180d affected renderer cohort passes385 tests with zero failures/
ignores and the full all-target workspace strict gate passes. The harness now waits
real elapsed time and pumps callbacks between waits; a delayed-XHR regression
verifies response-cookie delivery before the next search. Hash-guarded private
Deno event arrays preserve official abort lifetime/timeout handling. The measured
stream/listener microtask-order difference and broader descriptor-hook isolation
remain open in the parity checklist.

The earlier optimized544180d Google matrix repeats twice. Both native baselines
complete consent/save204 with its HttpOnly BUCKET cookie before search, then follow
search200 to sorry429 (0/2 native baseline SERP successes). Chrome baselines each
have9 markers and zero recorded errors. One native traced pass fails Missing promise
after homepage budget exhaustion; the other reaches sorry. Real elapsed consent
waits are verified; reliable acceptance remains unresolved. Offline follow-ups
cover watchdog/async-op admission recovery and premature timeout-signal GC.
The native155 follow-up passes155 focused tests, including timeout retirement/
forced GC, and native all-target workspace strict checks. Final integrated full
gates remain separate. Earlier dated checkpoints above preserve
their historical scope. Broader DOM/security/Fetch/resource behavior and final
combined/live validation remain tracked in
[PARITY-GAPS.md](../harness/detection/PARITY-GAPS.md) and
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28). Deferred audio
remains in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27).

The runtime follow-up default workspace gate passes1,312 tests across120 suites,
with three explicit native-CSP capability skips and six generated doctest skips.
Default all-target strict checks and formatting pass. Native155 affected tests
pass155/155 and native all-target strict checks pass. These results precede the
next canonical DOM/dictionary integrations; addon/shim/E2E, final integrated gates
and fresh live validation remain pending.


An intermediate integration vendors canonical `51b5b97`: scoped registry provenance,
submitter reflection, genuine live form collections and node-owned radio grouping,
canceled-click restoration, adoption and indexed container/owner lifecycle behavior.
Source-pinned production collection/real-frame GC tests pass2 controls, actual
form navigation passes12 controls, and geometry passes23 controls under actual155.
The final actual155 combined cohort passes46 tests, including ordinary radio GC
and32 exact dictionary resource/lifecycle cases. Stock renderer library326 and
dictionary/radio integration tests also pass. Focused passes do not replace
full combined workspace/addon/production gates.

Genuine compression-dictionary support now has independent Chrome resource/policy
controls and official C-encoder fixtures. Native store/selection/decoding, decoded
cache reuse and renderer background discovery are being integrated. Guest-supplied
Available-Dictionary is rejected at network admission while public Headers/Request
retain it; automatic native negotiation remains separate. Dictionary-coded bodies
now use bounded incremental workers/channels; real HTTPS gates verify headers
before body completion, partial output and cancellation without partial caching. See the
[implementation and explicit bounds](../harness/detection/COMPRESSION-DICTIONARIES.md).
Neither these source additions nor a passing local control establish Google SERP
acceptance; the last frozen Google result predates this integration.

The subsequent integration vendors canonical1e93cae for genuine live control
labels, receiver-realm collection identity and namespace-safe native association.
Actual155 production behavior/iframe and retired-realm GC tests pass. The host
pump also waits on a precise real Window deadline while native work is pending,
preventing virtual timers from overtaking earlier native timeouts or body I/O.
Independent Chrome controls and `tests/window_timer_clock.rs` retain the exact
relative outcomes; full combined retries remain separate. See the
[timer controls](../harness/detection/README.md#window-and-native-timer-clocks).
