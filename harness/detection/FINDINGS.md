# Google detection audit — 2026-10-09

Native Google SERP clearance is **unresolved**. The harness now distinguishes
receiving a JS shell, executing that shell, and obtaining accepted results after
its retry. Chrome oracle cookies were never reused in native tests.

## Checkout and execution context

Updated clean local main from `57ce76b` to `origin/main` `e14b35d` (v0.5.3), then
worked on `codex/google-detection-trace`. The audit read current code, the
v0.4.1–v0.5.3 Google/anti-bot changelog entries, hydration notes, sidecar docs,
search strategies, recon helpers, native runtime and existing probes/botto harness.

Native test build: wreq/BoringSSL impersonation, vendored `trust-anchors`,
`gpu-metal` and `coretext`, with MCP `Session::new()` installing all raster/text/GPU
hooks. Native default HTTP and JS profile is Chrome 154/macOS; native V8 is
149.4.0 through deno_core 0.404. Chrome oracle reports 154.0.8037.98. Both use
`America/Los_Angeles`, with fresh profiles/jars, query `rust lang`, homepage then
box engagement and Enter. Chrome uses patchright headed Chrome with no custom
surface spoofing. These are fresh-session controls, not identical environments.

## Initial audit outcome (before the parity repairs)

The completed matrix used a frozen all-feature executable. Local raw artifacts
were saved under `/tmp/turbo-detection-oct09-complete`; the comparison and manifest
record executable/harness hashes, feature flags, process outcomes and observations.
Those files contain raw session data and are deliberately not committed. Reproduce
with [the saved harness](./README.md).

| Run | Outcome |
|---|---|
| Fresh Chrome baseline | 7 organic result markers on `/search`; no ENID |
| Fresh native baseline | Search shell executes, writes SG_SS, navigates with `sei`; retry redirects to `/sorry` and returns HTTP 429 |
| Chrome interface tracing + debugger | `/sorry`, 0 results; instrumentation/time/rate are possible confounds |
| Native interface tracing | Challenge executes; retry rejected, 0 results |
| Separate uninstrumented shared probes | CSSOM/realm differences below; Metal gradient pixels match Chrome |

Chrome baseline passed in repeated independent matrices, including native-first
order. Trace-instrumented Chrome failed in multiple samples. We have not isolated
wrappers versus Debugger versus sequencing/rate as the cause of those failures;
do not use traced acceptance/rejection as a clean control verdict.

Chrome's successful navigation was homepage → initial `/search` JS shell → second
`/search` with `sei` → full results DOM. Its fresh cookies included
`__Secure-STRP`, `SEARCH_SAMESITE`, `AEC`, `NID`; ENID was absent. The native
homepage also carried real hidden `sca_esv`, `sxsrf`, `ei`, `iflsig` parameters.
Older claims that only a trusted ENID grants results, or that native homepage
always lacks those parameters, do not describe these observations.

The native `/sorry` reCAPTCHA page then throws `TypeError: Cannot read properties
of null (reading 'value')` during hydration. That error occurs **after** the server
rejects the retry; it is not evidence of why the retry was rejected. The harness
retains its status, response HTML, runtime stack and preceding network records.

## Initial audit: measured interfaces and code gaps

1. **CSSOM error sent to Google.** Native homepage sends a JS-error report for `lc`
   to `/httpservice/retry/jserror`. Captured Google source finds the stylesheet by
   `ownerNode`, reads `cssRules`, reports `lc` when null/empty, otherwise checks the
   last rule is a `CSSStyleRule` with selector `sentinel` (`mc` if not).
   The standalone collector creates `body { color: red; } sentinel {}`: Chrome
   reports 2 rules and the right last-rule class/selector; native reports 0 rules.
   `browser_env.js`'s CSSOM sheet starts with an empty rule array and does not parse
   textContent into rules. This is a demonstrated client error, not proof it is
   the SERP rejection criterion. The binding is vendored: fix upstream turbo-test
   and re-vendor rather than editing it here.
2. **Cookie visibility defect fixed here.** `op_cookie_get` used the network header
   helper, exposing HttpOnly cookies through `document.cookie`; `op_cookie_set`
   accepted script overwrites/creation of HttpOnly records. The new separate
   script-cookie path prevents both. Offline render and localhost telemetry tests
   verify network requests retain the server secret while JS sees only visible
   cookies. The live retry still fails after this correction.
3. **Iframe realm differences.** A new about:blank iframe in Chrome has distinct
   `Array` and `Function`; native returns the parent's constructors. Native child
   environments are synthetic windows sharing an isolate/prototypes, not separate
   Chrome execution contexts. Existing notes claiming full realm parity were too
   strong. Search shell tracing observes iframe creation; the native tracer does
   not comprehensively instrument those synthetic child windows.
4. **Script execution stacks.** Google error telemetry contains native wrapper
   frames such as `eval at __G.execScriptEl (<env>:2042:16)`. Classic page scripts
   run by indirect eval in that pump. The shell accesses `Function.prototype.toString`
   repeatedly along with performance, document.cookie, iframe creation and other
   DOM interfaces. Those calls are visible in native tracing; they are not a
   decoded detector verdict. Returned primitive samples/stacks are saved per API.
5. **Parsed controls differ.** Native hidden input `.value` can be empty while its
   `value` attribute contains the real token. The harness records a form fallback
   and uses hidden attributes rather than dropping them. That is an explicit
   accommodation; the underlying parsed-control/default-submission gap remains.
6. **Network context is incomplete.** Chrome sends accepted high-entropy hints on
   later Google requests, e.g. arch/bitness/full-version/platform-version. Native
   cold navigation defaults omit them and have no general Accept-CH state machine.
   `op_fetch` adds Origin/Referer but otherwise reuses the navigation-oriented
   backend. A JS beacon/subresource is not a top-level navigation. Native trace
   request headers are explicit overrides only, so omission from that log is not
   itself proof a header was absent on the wire. Code review establishes the
   missing hint state machine; a server/wire echo is needed for emission/order.
7. **Profile scope.** `FetchOptions.profile` affects rustls headers but is ignored
   under wreq emulation, which pins `default_profile()`. MCP/recon helpers can
   select a different per-host JS profile. Harness pins the default in both paths.
   The configured full build version can still differ from actual Chrome's, and
   viewports/languages differ. The current renderer string is hardcoded to M4 Pro;
   the Chrome probe here reported M3. A matching small pixel scene does not prove
   identity metadata is correct or coherent on every machine.

## Existing tooling: what its results establish

The botto collector/check pipeline was run with the explicitly selected current
v0.5.3 addon and all native capability features. It reported `fake=false`,
`suspicion=0`, no tells. The standalone integrity collector showed matching native
function-shape checks, but different property counts and clocks. These are scoped
probe results, not proof Google accepts the environment.

With Metal access, shared WebGL gradient readback matches Chrome:
`pixelHash=3630084024`, `centerRGBA=[128,128,153,255]`, 130944 nonzero bytes out of
131072. Without Metal permission, the compiled GPU build fell back to synthetic
pixels and a different hash. GPU flag presence alone must not be treated as a
successful GPU execution. `fp_snapshot --render` also had a preexisting collector
wrapping bug: terminal semicolons caused a SyntaxError; it is corrected here.

Current production/recon paths differ from this harness:

- Built-in `web_search` Google strategy remains `mode:"enid"`. It requires sidecar
  mint/cache, fetches natively and checks missing `rso`/`h3` markers; it does not
  execute the search shell before reminting. Cookie presence/expiry and a marker
  heuristic do not independently validate token trust.
- `browserless_google_serp` runs homepage recon/consent then raw search. Its old
  `enid_trusted` field is retained for compatibility, but is explicitly a marker
  heuristic. It now also reports `has_serp_markers` and `challenge_executed:false`.
- `page_script` now preserves source order and session cookies/client-hint state.
  It remains a concatenated execution helper, rather than the live PageSession
  challenge/navigation pump. `probe_mint` still has a separate fetch/probe jar
  path. These helpers should not be assumed to reproduce the live challenge path.
- Legacy `probe` uses Proxy instrumentation; the new descriptor tracer avoids
  receiver-breaking object proxies but still changes function identity/source.
- Sidecar mint only engages the box and checks ENID presence; it never submits
  that search. Current Chrome may search successfully yet omit ENID during mint.

## Parity repairs after the audit

The same `codex/google-detection-trace` branch now contains these general engine
repairs, implemented in parallel and checked with offline fixtures before a live rerun:

- CSSOM parses connected style text into rules with selectors and CSSStyleRule
  identity. Replacing text creates a replacement sheet; a retained old sheet keeps
  its rules and loses its owner. Constructed sheets, mutation and detached styles
  have regression checks. The upstream turbo-test fix is committed as `c7ba567`
  ([upstream PR #24](https://github.com/miaskiewicz/turbo-test/pull/24)) and
  mechanically re-vendored, including the existing Symbol namespace.
- Parsed/cloned/inserted hidden controls expose token values, names/types and form
  ownership; textarea defaults and explicitly selected options are covered. The
  harness form fallback now reads `.value` instead of hidden attributes.
- Iframes get independent V8 contexts and ECMAScript intrinsics. Child script
  literals, Function global scope, lexical bindings, parent/frameElement/defaultView,
  inherited origin, HttpOnly-safe shared cookies and message source identities have
  regression checks. Promise rejection callbacks borrow deno_core runtime state
  safely; child-relative fetch/XHR URLs and request source metadata have localhost
  regressions. Nonfatal unhandled rejection stacks are retained in stderr.
- Classic page and frame scripts queue onto the Rust host pump. They compile/run
  at the document or external URL with inline source padding; caught stacks no
  longer contain `eval at __G.execScriptEl` or hydration wrapper frames. Uncaught
  script errors remain isolated from subsequent scripts.
- Shared ClientHints state honors secure-origin Accept-CH opt-ins across navigation
  and resource requests. Browser metadata distinguishes document/iframe/script/
  style/image/fetch, derives referrer and Origin, filters guest-owned forbidden
  headers, and respects credentials. Repeated Accept-CH and Clear-Site-Data
  response fields are combined before parsing. PSL classification handles same-site siblings
  and private suffixes. Local server echo tests check actual transmitted headers.
- MCP sessions and NAPI browser contexts carry this state into fetches, hydration,
  and navigation. MCP script bundling now preserves original source order and
  session cookies rather than discarding the jar during execution.
- Metal renderer identity comes from the actual readback adapter. Native UA-CH full
  version matches the observed Chrome build; macOS platform version comes from the
  host ProductVersion (`27.2.0` here) in both JS and HTTP. High-entropy JS results
  contain requested fields rather than every possible field.

These fix the specific measured contracts, not every Chrome behavior. CSS parsing
is a style/group-rule scanner rather than a complete CSS parser. Ordinary text
input dirty/default value semantics and select default selectedIndex remain
incomplete. Frames still use detached DOM wrappers and mostly parent-created DOM
API constructors; cross-origin access enforcement is incomplete. Critical-CH
retries and Permissions-Policy delegation are not implemented. Native V8/backend
versions, viewport/language configuration and some API shapes can still differ.
Standalone recon/solver/Google ENID helpers have separate state paths; the default
Google web_search strategy still uses its sidecar path.

## Completed repair reruns

The successful repair matrices used frozen all-feature executables and fresh independent
sessions. Artifacts remain private under `/tmp/turbo-detection-oct09-repaired`
and `/tmp/turbo-detection-oct09-repaired-hints`. The second matrix ran native first
and included the repeated-header negotiation correction discovered in the first.
The reviewed iframe async/source fixes were then checked in
`/tmp/turbo-detection-oct09-final-full`, using a separate immutable binary copy.
An intervening attempt captured a Cargo example rebuilt with default features;
the harness correctly flagged missing capabilities and that run is excluded here.

| Check | Final observed outcome |
|---|---|
| Chrome baseline | 7 result markers on `/search` |
| Native baseline | Search shell HTTP 200 executes; subsequent retry reaches `/sorry`, HTTP 429, 0 results |
| Instrumented Chrome/native | Both rejected; instrumented verdicts remain confounded |
| CSSOM probe | Both return 2 rules and `sentinel` as a CSSStyleRule; native no longer sends any `/jserror` request in these runs |
| Iframe probe | Both have separate Array/Function/prototypes; child Function and literals use the child global/intrinsics |
| Parsed-control probe | Both expose the parsed hidden token, textarea default and explicitly selected option values, with correct form ownership |
| Request hints | Native search and retry include accepted arch, bitness, full version/list, model, platform version, form factors and WoW64 |
| GPU probe | Both report actual Apple M3 adapter and pixel hash 3630084024, center `[128,128,153,255]` |
| Current-addon botto | `fake=false`, `suspicion=0`, no tells |

Hidden-control properties, HttpOnly visibility and document/resource metadata are
also covered by local regression fixtures. Classic-script source URLs/line numbers
and absent indirect-eval wrapper frames are covered by render tests. The native
final live trace retains 98 baseline and 89 instrumented valid JSONL network
events, with zero incomplete records; all four capability features report enabled. Instrumented Chrome additionally preserves
CDP Network ExtraInfo headers; baseline automation headers are less complete.

These observations do **not** establish the server's rejection reason. Remaining
measured differences include viewport (native 1280×800, Chrome 1200×887), languages
(native en-US/en, Chrome en-US), property counts and stack depth, native V8 149
versus Chrome 154, wreq's Chrome149 transport emulation, and request/lifecycle
coverage. The final successful Chrome baseline had 206 requests; native had 33
application-layer requests and substantially less browser resource activity. That
count alone cannot identify a scoring input. Some API shapes, iframe DOM behavior,
full CSS parsing and request policy behavior remain incomplete as described above.
No oracle cookie was reused and native SERP clearance remains unresolved.

## Next discriminating experiments

Keep clean baseline verdicts separate from intrusive traces. Compare lifecycle and
resource initiation on a deterministic local Chrome/native fixture before
attributing missing live requests to server scoring. Extend explicit coverage to
child-realm accesses and worker/dynamic sources, and measure transport emission
with a wire/server echo. Matching the listed contracts has not decoded Google's
server-side decision, and should not be presented as a clearance solution.

## Verification and limits

Rust workspace tests pass; all-feature MCP tests pass (64 tests), including the
GPU bridge path. New offline harness tests pass (5), including a localhost
challenge/retry/payload/cookie fixture. Clippy and rustfmt pass. JS lint/format
pass (the repo-wide lint retains existing vendored-shim warnings). The final
all-feature release-addon shim suite passes (82 passed, 1 skipped),
and end-to-end tests pass (20). The current debug-addon shim surface
suite aborts in tiny-skia's `scan/hairline_aa.rs` while rendering a screenshot;
that raster path is unchanged by this work and remains a separate debug-render
limitation; the release gate passes, but the debug surface suite does not.
