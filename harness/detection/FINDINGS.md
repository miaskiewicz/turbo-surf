# Google detection audit — 2026-10-09; follow-up 2026-10-14

Native Google SERP clearance is **unresolved**. The harness now distinguishes
receiving a JS shell, executing that shell, and obtaining accepted results after
its retry. Chrome oracle cookies were never reused in native tests.

## Local integration checkpoint — 2026-10-14

Native Google acceptance remains unverified. The current canonical binding is
re-vendored from turbo-test `dfaff47` ([PR #25](https://github.com/miaskiewicz/turbo-test/pull/25)).
The canonical UTF-16/script-text checkpoint passes179 contracts in Rusty V8149.3.0.
The earlier slot checkpoint passed165 contracts; four
subsequent private composed-tree controls pass in Surf's defaultV8150.4.0 and
genuineV815.5.35.23. The integrated slot/isolation/messaging checkpoint passes264
contracts on both Surf engines, followed by42 API/policy/Window controls on each.
The subsequent full renderer library checkpoint passes222 contracts on both
engines, with ten source/script/microtask contracts on genuine155.23. Strict
affected checks pass on both; all42 offline harness tests pass.
The current native SDK matches installed Chrome155.0.8059.79's V8 source pin;
the earlier155.20/SDK152 results below are historical checkpoints. Full gates,
rebuilt addon/shim/E2E, prototype inventory and fresh Google artifacts remain pending.

Published turbo-dom0.5.5 preserves original DOMString payloads, names, namespace
identity and parsing through the native binding. Inline classics, handlers,
srcdoc/frame source and immutable inline-module snapshots retain original units.
The SDK loader uses actual two-byte V8 strings; original source/location/cache
identity and independent module maps pass its native proof. The33-contract
genuine155.23 source/iframe/sandbox/microtask cohort passes. Default SDK150 has
guarded frame/module tests, so its pass count does not certify those native cases.
Fresh Chrome confirms successful inline modules do not emit load; the regression
awaits actual execution and compares source units, import.meta.url and events.
HTMLScriptElement.text now uses native child text and preparation; associated
inert template documents, custom-element lifecycle and Trusted Types remain open.

queueMicrotask now uses actual V8 jobs rather than virtual timers. Fresh Chrome
controls preserve Promise/observer/timer ordering, function/receiver brands,
callable proxies and removed-frame callback realms. Exceptions dispatch trusted
Window errors without losing later jobs, and self-rescheduling jobs remain bounded
by the native watchdog. Four contracts pass on both default and genuine155.23.

The new authored element-interface matrix records 723 cases in real Chrome155,
including all preserved SVG observations. Native HTML/null-namespace interface
selection now uses exact local names, with genuine unknown/custom-name identity
and namespace-aware borrowed control brands. The canonical control passes;
SVGScriptElement and SVGImageElement projection/resource support are still gaps,
so the tested interface subset is not full SVG or custom-element lifecycle parity.
The mapping follows the [HTML element-interface algorithm](https://html.spec.whatwg.org/multipage/dom.html#elements-in-the-dom)
and independently recorded behavior. Earlier canonical and default native cohorts
passed135 contracts before storage/isolation/color work, then161 including meta
reflection, cached-thread teardown and token-list
PutForwards behavior. The new six actual PageSession contracts cover independent identities,
interleaved/concurrent polls, both close orders, real pending-fetch cancellation,
cached/stateless evaluation and per-isolate observer queues. Inspector teardown
with two live sessions also passes. The refreshed 48-contract session, tracing,
frame GC/lifetime, host-input and message-transfer cohort passes strict affected
Clippy. These focused results do not replace the final workspace gates.

Cached runtime disposal also survives the real thread-exit order where binding
TLS cells disappear first. Iframe sandbox lists preserve live native attributes,
receiver brands, supported tokens and SameObject identity. PutForwards uses
ordinary assignment: own value setters run and nonwritable own values leave the
attribute untouched, matching the saved Chrome155 control.

The runtime parks each persistent V8 isolate between operations and enters it for
one future poll at a time, restoring the caller before asynchronous suspension.
Native document/listener/observer/CSS/focus state belongs to its actual isolate.
Synchronous expression completion now runs a real V8 microtask checkpoint before
returning; the previous completion shortcut stranded MutationObserver delivery.
The public turbo-dom 0.5.4 bounded storage collector is integrated with real weak
wrapper roots and genuine host-job node pins. Thirteen storage and eight isolation
contracts cover retention, collection budgets, teardown and cold installation.
The default runtime now consumes actual V8 GC notifications at completed host
operation boundaries. The GC callback only sets a private per-isolate flag; the
scan runs later with fixed per-Tree limits (250,000 nodes, 2,000,000 edges, 64 MiB
retained payload) and at most 256 Documents. Budget rejection is recorded privately
and does not fail page work. No forced GC is introduced. Two genuine storage
contracts reclaim detached nodes/payloads while preserving retained Range/subtree
identity and independent sessions; the refreshed focused runtime cohort passes 50
contracts. The merged native155 session/storage and resource cohorts pass focused
controls; final full-workspace/lifecycle certification remains open.

Integrated and tested behavior includes real document factories and private node
projection, native cross-document insertion and live Range boundaries, live CSSOM
exports/adoption, Window/MessagePort transfers and endpoint cleanup. Wrapper,
helper and context lookups are weak; private V8 ownership edges preserve live
Document/subtree identity. WebCrypto uses actual cppgc tracing, and queued key
snapshots retain native data rather than sender realms. A source-pinned Rusty V8
teardown patch keeps weak handles live until native slot/finalizer cleanup finishes.

An actual heap trace found the main Window's private inert-document initializer
retaining the last child Document. Shared Deno ops run in their creation realm;
registration now uses the initializer callback's creation realm. A second trace
found main-Document component maps allocated in a child realm: ownership graph
objects now allocate in their actual Document's creation context, and draining
observer records no longer dirties unchanged ownership graphs. The paired Chrome
GC control passes after releasing retained Documents. The reusable host-only heap
snapshot diagnostic and `heap-retainers.py` identify unconditional native root paths;
raw heap payloads remain local.

MutationObserver delivers typed native records in a V8 notification microtask,
including compound mutation batches, old values, namespace filters and transient
removed subtrees. A trusted Document-private consumer runs after observer fanout.
Native script flags cover parser/force-async/already-started state, clone/import/
adoption and inert/retired documents. Trusted native postconnection consumers now
compile inline classics directly and synchronously, with authentic private
currentScript/error reporting and ordered external execution over concurrent
fetches. Native completion resolvers settle interrupted queued scripts without
guest code running during termination cleanup. Parser wrapper arrays release when
their lifecycle work finishes. COEP/CORP controls cover require-corp/credentialless,
redirect cookie admission, DOM resources and inherited srcdoc policy. The integrated
default resource/script/error/input/frame/session/storage cohort passes 35 focused
contracts and strict affected Clippy; the merged genuine155 resource cohort passes 27 contracts, including modules,
COEP, scripts, integrity, frame GC and actual session/storage integration.
See [RESOURCE-LIFECYCLE.md](./RESOURCE-LIFECYCLE.md) for bounds and genuine task-root
inventory. Parser reentry, SVG scripts, import maps and speculation rules remain open. Broader
observer lifecycle, Intersection/Resize queues, slots and custom elements remain.

The renderer now consumes published turbo-html2pdf-core 0.3.5 for genuine live DOM
rectangles, client/offset metrics, element scroll extents/projection and point hit
regions. It uses native source identities and CSSOM exports, with private DOMRect
clones and no page layout getters. The 17 V8 geometry controls pass. Text/caret/
Range geometry, complete stacking/scrolling, image/font lifecycle and SVG remain:
any active SVG namespace currently rejects the whole snapshot, including HTML
receivers in pages with SVG icons. Fractional native layout arithmetic also differs
from Chrome's LayoutUnit arithmetic; returned rectangles are not rounded to hide it.
See [LIVE-GEOMETRY.md](./LIVE-GEOMETRY.md) for precise bounds.

All eight referrer policies affect actual native request headers and redirects;
the fresh Chrome loopback control records 33 request cases. Focused integration
passes 56 document, GC, teardown, geometry, referrer and host-budget/transfer
tests. The earlier affected checkpoint also passed 45 clone, crypto, hybrid,
event and retained-frame tests. The host-budget test
caught an iframe bootstrap allocation abort; private capability publication is now
fallible and interruption recovery passes. Strict Clippy passes the affected core/
render libraries and GC/geometry/referrer/input/transfer test targets.

Still open: full final native155/default release gates,
full geometry and DOM lifecycle/parsing/lossless UTF-16, remaining CSP/COOP/COEP/
permissions policy, fetch streaming/cancellation/cache and partitioned storage/access.
The public storage API is integrated from verified published turbo-dom 0.5.4; final
focused default and native155 resource/storage validation passes; broader policy
and full final gates remain pending. Text provenance APIs are published in turbo-html2pdf0.3.6 from PR #20;
Surf consumer integration remains in progress. The
historical missing-own-member count is **94**, not a current measurement. The prior
848-test workspace gate is historical and does not certify these changes.

Current default HTTP/high-entropy JS profile fields use Chrome155.0.8059.79;
older Chrome controls retain their original captured version. The additional
source profile pins the official installed-release V8 revision
`d89479ac5446a374c55a35be19b5a23437558d99` / 15.5.35.23. Its independent
native source build, matching archive/bindings and SDK security smoke are
certified; the merged renderer passes231 focused contracts and affected Clippy. Experimental Cargo
commands resolve copied manifests/private locks with current source links; real
Cargo regressions verify production lock preservation and source/manifest updates.
The detection harness now passes 41 offline contracts.

The current experimental native155 engine is15.5.35.23-rusty, matching installed
Chrome155.0.8059.79 / V815.5.35.23. The preserved155.20 engine previously passed39 focused
contracts, with strict affected owned-code Clippy. Native WindowProxy/Location
lookups are weak and their private capabilities belong to the genuine lexical
JSGlobalObject, independent of proxy navigation. Per-caller weak caches preserve
cross-origin method identity and retained targets while allowing removed callers
and targets to collect. The Document getter routes by its genuine Window receiver
and closed state reflects retirement. A real heap root from Deno's strong module
context registry was removed; weak contexts preserve module-map lifetime while
retained. Reproducible SDK152 preparation/teardown patches reverse-verify exactly
against the pinned native artifact. Full native155 gates remain pending. The Chrome 155 target transport uses a pinned official Chrome 154 base
recipe with separately measured 155 headers/client hints; that distinction remains
in trace provenance. Audio is deferred in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27)
as requested. No captured evidence establishes it as Google's rejection cause.

## Original audit checkout and execution context

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

## Original parity repairs — 2026-10-09

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

At this stage of the original audit, these fixed the measured contracts, not every
Chrome behavior. The 2026-10-10 follow-up below supersedes several implementation
limits in this historical account. CSS parsing
is a style/group-rule scanner rather than a complete CSS parser. Ordinary text
input dirty/default value semantics and select default selectedIndex remain
incomplete. Frames still use detached DOM wrappers and mostly parent-created DOM
API constructors; cross-origin access enforcement is incomplete. Critical-CH
retries and Permissions-Policy delegation are not implemented. Native V8/backend
versions, viewport/language configuration and some API shapes can still differ.
Standalone recon/solver/Google ENID helpers have separate state paths; the default
Google web_search strategy still uses its sidecar path.

## Original completed repair reruns — 2026-10-09

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

These observations do **not** establish the server's rejection reason. At that
measurement, differences included viewport (native 1280×800, Chrome 1200×887), languages
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

## Original verification and limits — 2026-10-09

Rust workspace tests pass; all-feature MCP tests pass (64 tests), including the
GPU bridge path. New offline harness tests pass (5), including a localhost
challenge/retry/payload/cookie fixture. Clippy and rustfmt pass. JS lint/format
pass (the repo-wide lint retains existing vendored-shim warnings). The final
all-feature release-addon shim suite passes (82 passed, 1 skipped),
and end-to-end tests pass (20). The current debug-addon shim surface
suite aborts in tiny-skia's `scan/hairline_aa.rs` while rendering a screenshot;
that raster path is unchanged by this work and remains a separate debug-render
limitation; the release gate passes, but the debug surface suite does not.

## 2026-10-10 follow-up (verification in progress)

PR #26 also prepares turbo-surf 0.5.4. The compiled version constants/functions now
read `CARGO_PKG_VERSION`; npm/NAPI/Python manifests and the npm lockfile roots were
aligned, including the previously stale launcher lockfile and Python module version.

The engine now uses deno_core 0.412.0 / deno_v8 0.4.0, which links rusty_v8
150.4.0 with actual V8 **15.0.245.2** (Chrome 150 generation). The transport has
subsequently moved to pinned official Git sources that provide actual Chrome 154
TLS capabilities; the zero-difference localhost ClientHello evidence is recorded
below. The old local empty-anchor fork is removed. The newer standalone V8 cannot
simply be substituted into deno_core's supported facade; engine compatibility is
being investigated separately. See README dependency-source links.

The harness now records the actual V8 version and transport template independently
of the advertised browser profile, validates capabilities before visiting a live
URL, and supplies common viewport/screen/DPR/outer-window/language configuration
through the real runtime configuration paths.

Canonical turbo-test work adds context-owned trees, callbacks, node wrappers,
listeners/style/focus state and an independent DOM bootstrap for frames. The surf
adapter now delegates seeded document installation to that public canonical API;
its complete source is generated from a checked-in adapter template by the vendor
script, together with the committed upstream Rust/JS/CSS parser. Child ENV APIs and
async work are being installed/driven inside the child context. Origin-boundary
coverage must be verified separately; independent trees alone do not prove SOP.

CSS grammar/recovery uses Lightning CSS rather than accepting every block with the
old scanner. Linked stylesheets and image requests/decoding join the lifecycle
pump. Supported grammar and lifecycle tests do not imply Chrome's complete layout,
CSSOM, parser scheduling, navigation, worker or document-security implementations.

Form-control fixtures and a local Chrome reference both produce:
`before=["initial","initial text","a",1]`,
`after=["edited","new default","live","default","",-1]`.
This verifies dirty/default input and textarea values, ignored textarea value
attributes, first enabled single-select default selection and explicit empty
selection. Hidden/button/checkbox value modes retain reflected content attributes.
Complete input-type sanitization, form reset, checked-state and mutation timing need
separate coverage before claiming general form-control conformance.

The 0.4.5 turbo-test release belongs to merged upstream PR #24 and tag `v0.4.5` at
main merge commit `e09e134`. This subsequent canonical work is separate from that
immutable release. Automated platform/npm/crates jobs succeeded; npm now resolves 0.4.5 and its
`latest` tag with the published tarball integrity. The temporary release monitor
was retired after this verification.

## Chrome 154 transport wire comparison — 2026-10-10

The production client now uses the official pinned Chrome 154 stack: wreq
`c6fd8b1`, wreq-util `5715529`, btls `f5a8a48`, and its BoringSSL submodule
`f1f2556`; Rust 1.98.0 is pinned. The old empty trust-anchor patch is superseded
by upstream signature-algorithm GREASE, ML-DSA and real public Chromium root IDs.
Certificate verification uses the same Chromium root store as the advertised IDs.

The reusable localhost `transport-hello.mjs` comparison against installed Chrome
154.0.8037.98 found **zero normalized ClientHello differences**;
[TRANSPORT-HELLO.json](./TRANSPORT-HELLO.json) contains both captures and hashes.
Only protocol randomness is normalized: GREASE values, extension permutation and
opaque ECH payload length. Cipher/signature algorithm order, GREASE presence,
ML-DSA44/65/87, public trust-anchor IDs and extension capabilities match. No Google
traffic or browser cookies were used. The offline Rust ClientHello regression,
141 core unit tests, six request-context regressions and existing TLS/HTTP2 and
trust-anchor echo checks pass. This evidence does not prove full engine parity or
Google acceptance; native V8 remains separately measured.

Library publication is blocked. `cargo package -p turbo-surf-core --allow-dirty
--no-verify` fails before compilation because registry wreq 0.16.1 lacks the
`chromium-roots` feature; workspace patches do not propagate to crate consumers.
A binary built from the pinned workspace can use the newer APIs. Publication must
wait for compatible registry releases or a reviewed distributable dependency fork.


## 2026-10-10 follow-up integration evidence (still in progress)

The fresh all-feature localhost API audit at `/tmp/turbo-api-shape-oct10-latest-a`
reports eight difference paths, including broad DOM prototype member lists. This
count is not eight individual missing APIs. Existing Document/Node methods and
runtime Document state are being moved to their actual interfaces; unsupported
browser features remain visible in the audit.

The fresh live matrix at `/tmp/turbo-detection-oct10-parity-intermediate-b`
returned seven Chrome baseline result markers. Both native passes aborted during
homepage script loading, before SERP clearance could be tested. Application-layer
responses for the fetched scripts were HTTP 200; native script errors included
`document.getElementsByTagName is not a function` and
`WebKitCSSMatrix is not a constructor`. These are runtime integration failures,
not a new measurement of the server's rejection criterion. Canonical Document
projection and genuine private DOMMatrix/DOMPoint operations address these errors.
The shared geometry fixture matches the installed Chrome, including its
axis-aligned inverse precision. [Chrome's Blink geometry implementation](https://chromium.googlesource.com/chromium/src/+/refs/tags/154.0.8037.98/third_party/blink/renderer/core/geometry/dom_matrix.cc)
uses the underlying gfx transform inverse.

The isolated exact-engine source build completed and actually ran V8
`15.4.80.20-rusty`, matching Chrome's `15.4.80.20` engine generation. This is not a
production engine upgrade yet. SDK callback compatibility, renderer/addon gates
and stable WindowProxy semantics are being validated. The source preparation,
SDK patches, version smoke and native context tests are persisted under
`engine-154/`. The published default still selects V8 15.0.

Chrome's measured frame navigation semantics preserve contentWindow identity.
Retained Function constructors remain usable after same-origin navigation and
refer to the replacement document; cross-origin use throws TypeError. A
cross-origin WindowProxy exposes a null prototype and a restricted own-key list.
Earlier assumptions that all of these operations must throw were incorrect.
`window-operations.mjs` reproduces the normal Chrome measurements. Stable native
WindowProxy adoption remains open until native tests prove these contracts.


## Matching-engine live control — 2026-10-10 (intermediate)

The frozen `/tmp/turbo-surf-full-native154-intermediate` executable reports actual
V8 `15.4.80.20-rusty`, Chrome 154 transport, Chromium trust anchors, Metal and
CoreText. The complete matrix is saved at
`/tmp/turbo-detection-oct10-native154-intermediate` with independent fresh cookie
jars. Chrome baseline obtained six result markers; native baseline and native
interface tracing obtained zero. Both native runs executed the homepage and
initial search shell without JavaScript errors, then followed the retry to
`/sorry` with HTTP 429. Chrome interface tracing also reached `/sorry`, so traced
acceptance remains a confounded control. Matching the engine and TLS ClientHello
has not resolved native clearance.

The uninstrumented shared geometry probe using the same executable reports zero
differences at `/tmp/turbo-geometry-pair-native154/comparison.json`. This covers
real DOMMatrix/DOMPoint operations and precision, rather than browser layout.
The latest API audit still includes broad DOM member-list differences; subsequent
Document, namespace, timing, geometry static-method and crypto-contract changes
must be rebuilt before treating that artifact as current.

This frozen binary precedes stable WindowProxy runtime adoption. The new native
SDK tests and integrated navigation test now pass measured same-origin and
cross-origin contracts, including retained functions/documents. The exact-engine
build helper selects that architecture; the default published SDK remains V8
15.0. Final renderer/addon verification and a new frozen live matrix are pending.

## 2026-10-13 checkpoint — API audit complete, fresh Google results pending

The final uninstrumented localhost API audit is saved at
`/tmp/turbo-api-shape-oct13-final-native154/comparison.json`; its sanitized public
snapshot is [API-SHAPE.json](./API-SHAPE.json). It uses the frozen final executable,
SHA-256 `3dbf3a6ebf0308cc5e88df0250cfceca62b306791a3085dee483e6ab3a696095`,
with impersonation, Chromium trust anchors, Metal and CoreText enabled. Its actual
V8 is `15.4.80.20-rusty`; transport and reported Chrome major are 154, and the
canonical binding revision is `5050cf1`. The installed Chrome oracle auto-updated
to **155.0.8059.40**, V8 **15.5.35.20**. Raw CDP launched normal Chrome with a
nonzero debugging port and observed `navigator.webdriver=false`; no interface
wrappers were installed. This is a version-mismatched API control, not evidence
of complete Chrome 154 parity.

The audit reports **six difference paths**, each an array of interface members,
not six missing APIs. Four groups concern Document, Element, HTMLElement and
HTMLCanvasElement. Two concern SubtleCrypto methods and constructor statics
available in the installed Chrome 155 (`encapsulateBits`, `encapsulateKey`,
`decapsulateBits`, `decapsulateKey`, `getPublicKey`, `supports`). There are no extra
native members in these six groups. The counts remain unchanged from the interim
prefix executable. The collector measures its specified contracts and member
lists, not every method's behavior.

Remaining DOM member gaps include:

- Document: XPath APIs (`evaluate`, `createExpression`, `createNSResolver`), caret
  hit testing, editing commands, storage-access/private-token APIs, pointer lock,
  picture-in-picture, view transitions/animation timeline and browser metadata
  such as `lastModified`. Legacy aliases, `Symbol.unscopables` and newer custom
  element registry properties also differ.
- Element: ARIA element relations (`ariaControlsElements`, `ariaLabelledByElements`
  and related properties), `part`, `assignedSlot`, client/scroll geometry,
  pointer capture, animation/Typed OM/view-transition APIs and HTML serialization/
  sanitization methods (`getHTML`, `setHTML`, `setHTMLUnsafe`). Some missing member
  names are compatibility aliases or handler ownership differences, rather than
  wholly absent event dispatch.
- HTMLElement: `innerText`/`outerText`, `offsetLeft`/`offsetTop`/`offsetParent`,
  ElementInternals, editing contexts, focus groups, popovers and Typed OM.
  HTMLCanvasElement lacks `captureStream` and `transferControlToOffscreen`.

The following paragraph records the earlier audit's implementation state. The
subsequent live geometry bridge replaces its zero-layout values for the measured
block/replaced subset with native layout from published core 0.3.5, real CSSOM
exports, element scroll metrics and point-hit regions. Its initial 17-contract
proof uses default V8 150, independently measured against Chrome 155; genuine155
and fresh Google certification remain pending. [LIVE-GEOMETRY.md](./LIVE-GEOMETRY.md)
records exact coverage and open inline/caret/scroll/stacking/namespace limits,
including the measured fractional layout-unit difference.

The supported subset was narrower than these member lists might suggest. Native
namespace-aware elements and attributes, live Attr/NamedNodeMap and document
collections, supported import/adoption, scalar ARIA and HTML attribute reflection
have meaningful mutation/identity tests. DOMMatrix/DOMPoint arithmetic and the
shared geometry fixture match the measured oracle. Element rectangles still use
zero-layout compatibility values; offsetWidth/offsetHeight measure connected
text with the configured native font measurer, not complete CSS box layout.
Observers remain synthetic compatibility implementations. Existing classList
mutates the native class attribute but returns a fresh object and lacks complete
DOMTokenList branding/indexing/token validation; shared DOMTokenList behavior,
`part`, and actual ARIA relation state are explicitly pending follow-ups, not
shape-only replacements.

Current CSS controls match their measured scope: inline CSP/nonce/attribute versus
CSSOM writes, nonretroactive parser meta policy, linked/imported sheet loading,
origin-clean/CORS guards, MIME failures and trusted resource event order. The two
paired controls at `/tmp/turbo-css-security-prefix-final/comparison.json` and
`/tmp/turbo-stylesheet-origins-prefix-final/comparison.json` have zero semantic
errors and zero field differences against installed Chrome 155. Private native
CSS snapshots prevent page accessor/iterator overrides from redirecting the host
import loader or polluting its interface trace. These checks do not prove full
CSS layout, every CSP directive, or Google clearance.

Release checks exposed and repaired Document adoptedStyleSheets setter projection
and loss of constructibility/statics for the existing observer classes; four
focused render tests pass. A stale shadow test now verifies live native shadow
content stays outside serialized light DOM. The final executable includes these
repairs, per-context handler/CSS policy installation and private Event record/
strict host-dispatch hardening. All 55 canonical library tests pass.

The default backend completed **680 workspace tests**, strict workspace clippy and
format checks. The default release addon completed **83 shim tests with one skip**
and **20 E2E tests**. The all-feature native154 render/MCP gate completed **330
tests** and strict clippy with no failures. The native154 debug addon's raster
integration is still under investigation; these CLI and default-addon gates do
not establish that variant's complete addon readiness.

**A fresh Google matrix is pending.** The last matching-engine live matrix still
produced zero native SERP markers and HTTP 429 `/sorry`; no current evidence proves
clearance without a Chrome cookie sidecar. The official Chrome-for-Testing
154.0.8037.92 control reported V8 15.4.80.19 but timed out before a local page
navigation completed, so it provides version proof rather than a replacement
API or acceptance control.

## 2026-10-13 continuation — genuine155 and native crypto integration

The saved 155 source profile completed its engine build and SDK security checks.
Actual V8 is **15.5.35.20-rusty**, matching the installed Chrome 155 engine generation.
The validated archive SHA-256 is
`492099759166645f531f48e6cf0cb0a7838af9eb670945c0a6444e9693fc5901`;
bindings SHA-256 is
`c4e656c837689f6dbb1e10e78aeec1ccad0abf3c39e026cf5674da5688f0da82`.
This is engine provenance, not full browser API parity or Google acceptance.

[TRANSPORT-155.json](./TRANSPORT-155.json) preserves the local TLS and HTTP/2
controls. Measured ClientHello fields, H2 settings/window and navigation header
order have zero normalized differences with normal Chrome 155. At that saved transport checkpoint the target profile
reported155.0.8059.40, includes image/jxl in navigation Accept, and uses actual
major-seeded client-hint GREASE. Its pinned upstream recipe remains154. No claim
is made about all H2 frame priorities, HTTP/3, or image/jxl decoding.

At that checkpoint the DOM dependency was published turbo-dom0.5.2, after PR #8's tested tagged
merge `87c2077`. Surf resolves the registry crate without a DOM Git patch. This
removes the DOM publication blocker; compatible transport registry APIs remain
unpublished. Surf 0.5.4 is still unreleased.

Canonical DOMTokenList/part, scoped ARIA references, supported moveBefore and
innerText/outerText mutations have native offline tests. Rendered text uses
visibility/whitespace/block/table/details cascade rules; complete CSS layout and
line wrapping are still open. Official matching Deno WebCrypto dependencies now
provide real Rust-backed keys, digests, AES/HMAC/Ed25519 and ML-KEM operations.
The paired Chrome control rejects ML-KEM-512 even though the backend supports it;
public browser-profile admission and supports normalization now match the bounded
210 operation/name queries and 13 of 14 parameter controls.
[WEBCRYPTO.json](./WEBCRYPTO.json) records the one remaining hybrid-KEM support
difference: Chrome exposes genuine MLKEM768-X25519 (X-Wing), while the native
managed key backend does not yet implement it. Primitive composition matches the
known-seed Chrome public-key vector; complete key/API integration remains pending.
Twelve crypto contracts pass, including permission enforcement, private factories,
secure-context exposure and iframe result/error ownership. Three expression tests
verify the reusable async collector, guest result-slot isolation and runaway
budgets. Final integrated gates and fresh paired 155 controls are pending.

The saved Google consent control used normal Chrome 155 and clicked the observed
Reject-all button, then produced seven result markers for the actual query. It
used its own fresh cookie jar; no Chrome cookies were transferred to native. The
resumed pre-155 native matrix had exhausted hydration budgets and invalid input/
consent state, so it does not establish a successful native SERP run. Consent
dismissal and input checkpoints are now part of the reusable harness. A fresh
fully equipped native 155 run is still pending.

The isolated probe runner now serves identical localhost HTML/URL to both engines.
The old Chrome about:blank/native HTTPS pair had different secure contexts; its
global/Navigator enumeration counts cannot establish a version-independent gap.
The old member inventory remains dated evidence until the rebuilt155 audit.

The full gate exposed a startup-readiness regression with short guest budgets:
trusted environment/crypto initialization could be terminated before the private
input driver was installed. Trusted preparation is now completed and verified
before starting the unchanged guest execution budget. Seven host-input regressions
pass on default V8 150 and genuine V8 155, including a 100ms runaway authored init
that leaves hidden host globals, real crypto and a usable trusted input driver.
The corrected 155 renderer suite passes 126 tests; complete gates are rerunning.

The normal Chrome 155 secure-page capability capture reports hardwareConcurrency 8
and deviceMemory 16, while the old native profile reported memory 8. The desktop
155 source rounds/clamps memory within 2..32 GiB (Android 1..8). The local default
now uses 16, through the shared version registry and native render op; historical
154/153 defaults remain 8. The corrected default renderer 126 and core 150 library
tests pass, as does native 155 renderer 128; final addon/fresh control checks remain
pending. Source: [official 155 memory implementation](https://chromium.googlesource.com/chromium/src/+/refs/tags/155.0.8059.40/third_party/blink/common/device_memory/approximated_device_memory.cc).

Current checkpoint gates: **721 default workspace tests**, strict all-targets
Clippy and format checks pass. Genuine V8 155 with all four native features passes
**366 render/MCP tests**, plus **128 renderer and eight core fingerprint tests**
after the device-memory correction, and strict Clippy. The inherited extension
doctest is the sole ignored case. The offline harness has **34 passing tests and
one dependency-gated native integration skip**; headed Chrome iframe/worker/input/
consent capture regression passes. Both release addon builds/checks are underway.
The corrected frozen CLI SHA-256 is
`898b3532c20030ac0d68707b19a13cca7d465e034b9c144d7965bbcd5b65e1c1`;
its private build manifest records source/native-archive hashes and the checkpoint
scopes. Native Google acceptance and the full API/policy backlog remain open.


### October 13, 19:53 UTC — measured follow-up after the pushed checkpoint

PR26 checkpoint `d36521d` passes both Node20 and Node22 CI jobs. Its default and
genuine155 release addons each pass 83 shim cases (one intentional fixture skip)
and 20 E2E cases. These binaries predate the following source changes.

The frozen genuine155 CLI `898b3532c20030ac0d68707b19a13cca7d465e034b9c144d7965bbcd5b65e1c1`
was paired with real Chrome155 on identical authored localhost fixtures. The
updated API snapshot has four member-array differences containing 94 missing own
members (42 Document, 39 Element, two Canvas and 11 HTMLElement), down from the
historical121. The covered token/ARIA and text cases match; popover lifecycle
methods accounted for the 11 text/popover field differences. No Google requests
were made by these controls.

Real native X-Wing managed keys now pass the Chrome known-seed vector, imported
Chrome ciphertext and native-to-Chrome ciphertext controls. The shared collector
records zero differences across285 operation/name and14 parameter queries, with
real bits/key roundtrips for ML-KEM-768, ML-KEM-1024 and MLKEM768-X25519. The new
SDK155 focused gate passes32 cases:12 crypto, four hybrid public, five native
hybrid, eight host input and three expression cases. This establishes the covered
crypto contracts, not full WebCrypto or browser parity. CryptoKey clone and hostile
prototype-property access are under separate audit.

Temporary public Deno/core objects were removed from environment setup. The
embedder passes core as a private function argument and retains it in native
context slots. Interruption during iframe churn leaves no exposed bootstrap
capabilities and usable parent input; this regression passes on both V8 builds.

Canonical popover source at turbo-test `b18f1ed` passes79 library tests. Its shared
Chrome155 lifecycle result also passes in Surf, including the nine mode pairs. A
new child-realm scheduler regression fails with missing queued events; the actual
owner/scheduler seam is being repaired. Focus restoration, external light dismissal,
stateful cascade and top-layer painting remain separate open behaviors.

Resource integrity now has native SHA256/384/512 response-byte admission work.
Its initial tests pass raw invalid-UTF8 bytes, tampering, strongest-hash selection,
unknown tokens and CORS/opaque eligibility. Script/module/style/frame execution
controls pass byte admission; the integration test also exposed missing Window
capture propagation of element resource events. Follow-up validation is pending.
Implementation reference: [official Chrome155 integrity implementation](https://chromium.googlesource.com/chromium/src/+/refs/tags/155.0.8059.40/third_party/blink/renderer/platform/loader/subresource_integrity.cc),
[W3C SRI](https://www.w3.org/TR/2016/REC-SRI-20160623/).

There is still no verified fresh native155 Google SERP result and no100% parity
claim. Full checks and fresh paired controls must cover the final integrated
source before the next consent/query run.


### October 13 follow-up — provider hardening and private clone integration

The adversarial provider regression reproduced a leak of16 raw AES key bytes into
a page-installed inherited `keyData` setter during non-extractable import. The
licensed published deno_crypto0.273.0 source is now reproduced from its verified
archive plus an auditable patch under `rust/vendor`: native data-property writes
bypass inherited setters, and unregistered JS key snapshots are skipped entirely.
The fixed regression sees zero setter calls and zero exposed bytes. An inherited
Array index setter is also covered. See [provider provenance](../../rust/vendor/DENO_CRYPTO.md).

The private V8 clone serializer carries genuine key handles out of band; it never
exports material, calls page key properties or installs registry-symbol hooks.
Its covered cases include non-extractability/usages, cyclic graphs, aliases,
Map/Set/Date/typed bytes, fake keys, poisoned key properties, genuine KEM/Ed keys,
child realms and MessageChannel payload identity. A fresh Chrome155 control found
a further borrowed-Window receiver realm mismatch, which is being corrected.
Native Event rejection is also being integrated through the canonical private
brand helper rather than public constructors or `instanceof`. Transfer lists and
complete cross-session messaging remain open.

After the core clone integration, the default focused gate passes60 tests:
24 API, six crypto isolation/clone,12 crypto, four hybrid, eight host input, two
popover and four resource-policy tests. The latter cover actual SRI byte admission,
CORP origin/site/case/duplicate rules and active HTTPS mixed-content blocking with
loopback exemptions. The child popover scheduler regression now passes following
canonical `f2c596c`. All35 reusable harness offline tests pass without skips.
These are focused source gates; the final workspace/native155/release-addon gates
and final paired measurements still need to run.

The hardened provider is a workspace patch, so registry Surf consumers cannot
safely fall back to the unpatched provider. A compatible distributable provider
or upstream fix is an additional crate-publication prerequisite, alongside the
already-unpublished transport APIs. No Surf release tag has been pushed.

## Additional native lifecycle and resource fixes — 2026-10-13

The classic script pump now retains actual V8 exceptions instead of rebuilding
Errors from text. Main/child runtime and syntax errors report trusted Window
ErrorEvents with the original exception identity and realm, native filename/line/
column, and successful external script `load`. Inline exceptions do not create
resource-error events. Cross-origin no-CORS error reports are muted using the
actual final native response context, including redirect taint: `Script error.`,
null error, empty filename and zero location. CORS-authorized responses retain
normal reporting. This follows the [HTML classic-script/error algorithms](https://html.spec.whatwg.org/multipage/webappapis.html).

The reusable `classic-script-errors.mjs` two-origin loopback runner has a saved
normal Chrome155 control in `fixtures/classic-script-errors-chrome155.json`.
Current default native regressions pass; final rebuilt native155 pairing remains
pending. Window listener options/onerror and complete browser error reporting are
separate work, and this does not establish Google acceptance.

SRI regressions now assert that a rejected stylesheet leaves `link.sheet === null`
and that a rejected iframe script fires exactly one error. Authored resource
fixtures and their collector cover these outcomes with exact binary response
bytes, dependencies and policy headers. They do not claim full resource-policy
or streaming/scheduling coverage. The native audio implementation and its
explicit bounds are recorded in [AUDIO.md](./AUDIO.md); fixed waveform placeholders
have been removed.

## Window event integration checkpoint — 2026-10-13

The canonical Window checkpoint `d9801e5` passes 104 Rust and 63 npm tests and
independent headed Chrome155 phase/option/handler controls. Surf vendors the
committed code, privately captures final ErrorEvent/AbortSignal interfaces after
each environment bootstrap, and removes duplicate manual Window callbacks and
DOMContentLoaded dispatch. Its focused event/lifecycle/input/clone/resource
cohort passes 38 tests. Subsequent transfer development found an obsolete ENV
AbortSignal replacement that interferes with genuine signal validation; it is
removed and a branded signal/default-reason regression has been added, with the
final constructor capture and integrated rerun pending.

Current default workspace validation passed 810 tests (four generated extension
doctests ignored), strict Clippy and format. The small Clippy iterator refactor
was independently revalidated against the three native DSP and seven public
audio tests. Genuine155 final checks are running; subsequent Abort integration
repair makes pre-repair results a checkpoint rather than final certification.
Window/MessagePort transfers and advanced audio scheduling/effects are isolated
subagent work and are not part of that saved source checkpoint. The current
49-test focused clone/API/audio/native-DOM cohort passes true AB/RAB detachment,
private crypto/DOMException graph and User Timing semantics, and actual Blob
download content. Wider transfer types, browser interface serialization and
advanced audio behavior remain open until independently integrated and tested.

## Native referrer-policy checkpoint — 2026-10-14

The native transport now applies all eight Referrer-Policy values to actual
outgoing Referer headers. Document response/meta defaults, Request/fetch overrides
and response-header policy changes across redirects use the same implementation.
Credentials and fragments are removed, long URLs reduce to an origin, and
trustworthy downgrade checks include loopback exemptions. Initial blank child
documents inherit the private parent policy; custom referrers must be same origin.

The reusable `referrer-policy-chrome.mjs` / `referrer-policy-probe.js` collector
and `fixtures/referrer-policy-chrome155.json` record 33 independent authored
loopback requests against Chrome155. The checkpoint does not establish full
CSP form/base/report-only, COOP/COEP, HTTP cache, streaming/cancellation, storage
partitioning or Google clearance. The integrated default source gate passes three core policy tests and seven
native referrer/fetch-header/resource-integrity contracts. Final broader workspace
and genuine155 gates remain pending until the remaining implementation areas land.


## Resource admission and COEP checkpoint — 2026-10-14

The resource consumers now admit scripts through native scalar preparation state,
using authentic original document/element wrappers rather than guest properties,
markers or ongoing script-node polling. Inserted inline classics compile directly
in the owning V8 context and execute before insertion returns. Text replacement,
CharacterData-only edits, changed data-block types, parser-inert scripts, cloning,
shadow currentScript and retained/retired document adoption match the saved authored
Chrome controls. Ordered dynamic external scripts download concurrently and execute
in insertion order. Native queued completion resolvers remove the JS waiter map;
interruption restores currentScript and settles pending completion without invoking
guest JavaScript while termination is temporarily cleared.

COEP require-corp and credentialless now affect the shared transport and the
separate classic-script fetch path. CORP, CORS, SRI, opaque stylesheet identities,
computed linked colors and images are tested against actual responses. The authored
Chrome155 controls record 54 direct fetches and cookie headers, 15 DOM resource
requests, 12 redirect cases with 23 hops, inherited srcdoc policies/error realms,
and 12 policy-header syntaxes. The measured browser rejects RFC9651 date/display
string parameters; native parsing explicitly uses the maintained RFC8941 dialect.
IP addresses across different ports do not satisfy CORP same-site. Credentialless
requests suppress cookies after crossing an origin, including redirects back to
that origin; CORS requests retain their independent credential mode.

These controls used fresh normal Chrome 155.0.8059.79 / V8 15.5.35.23, without
borrowing cookies for native execution. The isolated native checkpoint uses the
actual default V8 SDK150 and preserves that identity. It does not certify native155,
full browser parity or Google acceptance. COOP/crossOriginIsolated, navigation
embedding compatibility, permissions policy, reporting endpoints, complete CSS
cascade/color-scheme, document.write, import maps, speculation rules and wider
resource/lifecycle scheduling still need independent controls and implementation.


## CSP metadata and frozen base URLs checkpoint — 2026-10-14

Normal Chrome155 controls distinguish base insertion from CSP meta postconnection
processing. In a compound fragment, a new base freezes before the fragment's CSP
meta is applied, even when that meta precedes the base. With parser insertion,
an earlier head meta does constrain the later base. A later policy cannot change an
accepted frozen base retroactively; writing href subsequently rechecks it. Body
metas are ignored, removed/tightened/loosened head metas retain every previously
applied restriction, and default-src does not fall back to base-uri.

The implementation uses captured authentic DOM callbacks, native URL/CSP checks
and a private document record. Queued disconnected attribute construction records
are ignored using original native ancestor witnesses. Parsed metas activate at
parser completion even when the document contains no scripts. Frame script host
allowlists and module bases obey initial base admission; Request/XHR, scripts and
iframe URL resolution use the private base getter. A retired retained document
keeps its existing frozen URL until mutation; rewritten/replaced bases then fall
back, matching the fresh Chrome control. A document-owned metadata-only consumer
keeps that behavior while native script preparation remains disabled.

Four reusable authored controls cover six live base cases, six meta mutation
cases, six parser/fragment/namespace cases and two retired-document cases. A
separate native regression poisons public tree hooks and confirms zero reads.
Canonical HTMLMetaElement reflection now updates native attributes instead of
creating ordinary properties; its normal Chrome descriptor/brand/conversion
control and 159-binding cohort pass. These are bounded default SDK150 checks;
The later merged current-engine checkpoint validates native155 below.
Form-action, reporting and broader parser/CSP resource scheduling remain open.

### Current engine and mutation-policy follow-up

The real source-built155.0.8059.79 engine reports15.5.35.23-rusty and passes
native version/archive/binding checks and WindowProxy/security smoke. The merged
renderer passes231 focused contracts (203library,6CSP/base,1cached teardown,
9input,2storage,4Window ownership and6session) with strict affected Clippy.
The preserved155.20 artifact still reproduces historical checkpoints. These
results close the measured engine-version difference; full browser/transport
parity and Google acceptance remain unverified.

A fresh same-origin/opaque srcdoc control found a pump ordering bug: polling
could enqueue a native script, then advance its parent's delayed timeout before
executing that script. Queued scripts now take precedence over virtual timer
advancement. Native bootstrap base reads authenticate the actual Document and
read private Location state, avoiding cross-origin descriptor interception.
Both own Location descriptor and live base reads survive guest attribute/query
poisoning in the saved Chrome control. The default Location facade uses private
receiver records and captured URL parsing for corresponding getter behavior.

Fresh Chrome shadow-slot controls now record named/manual assignment, stealing,
foreign-node adoption, fallback flattening, closed-root assignedSlot visibility,
failed conversion and native descriptors/receiver brands. Slotchange coalesces
after MutationObserver callbacks and before the later Promise reaction. The
authored probe/control are reusable. Native named/manual distribution, closed-root
visibility, fallback flattening and notification order now pass the control,
including foreign-document stealing/adoption and reentrant observer mutations.
Real collection confirms clearing manual assignment releases the old slot edge.
A4096-level fallback walk uses an iterative native traversal. The complete
canonical cohort passes165 contracts. The combined renderer checkpoint passes264
contracts on defaultSDK150 and actualV815.5.35.23. Shadow composed layout, custom-element
lifecycle and broader scheduling remain open.
The native isolation checkpoint below covers the measured COOP/COEP and
Permissions Policy paths; its new merged native155 validation is pending.

## Native isolation, policy and credentialless partition checkpoint — 2026-10-14

A native private capability now determines isolated shared memory from actual secure navigation, a valid same-origin COOP response, compatible COEP and inherited Permissions Policy. Guest crossOriginIsolated or SharedArrayBuffer assignments cannot grant it. The measured42 top-level header cases and79 iframe cases match fresh normal Chrome155 controls. Incompatible child COEP blocks the committed embedded document, and cross-origin admission requires CORP or a genuine credentialless iframe. Containers delegate foreign self-default features through native attributes; ancestor/header denial cannot be undone by a child.

Eight controls exercise real shared memory delivery. Same-origin isolated frames, including credentialless siblings, share actual backing stores. Foreign agent-cluster receivers report trusted asynchronous messageerror events; an unisolated sender rejects serialization. Native snapshots carry group/origin ownership and include fixed admitted shared-memory bytes in queue accounting, retaining transfer failure atomicity and stable accounting after shared WASM growth. Ordinary unisolated documents still allocate shared WebAssembly memory but cannot clone/message it. Local structuredClone rejects at deserialization after a regular ArrayBuffer transfer has completed, matching Chrome.

Credentialless navigations use an actual private cookie jar. Set-Cookie, document.cookie, fetch, images, modules and local/session storage use the same top-document nonce, so credentialless siblings share their private state while normal frames retain the ordinary state. The source-pinned control measures initial navigation headers, real subsequent fetch headers, response cookie ingestion and storage separation. Native owner teardown reclaims the partition's storage areas. No browser cookies are imported. Policy records and backing stores contain no queued unwrapped DOM node handles.

The79-feature PermissionsPolicy interface has actual native document brands, descriptors and response-header parsing; FeaturePolicy aliases its constructor. Native permission queries report denied for disabled policies and clipboard operations reject disabled access. The3 effect controls correct the stale midi default to prompt. This does not implement a browser permission prompt, OS clipboard or every controlled API.

These are tested defaultSDK150 contracts and normalChrome155/V815.5.35.23 observations. Final merged/native155 gates and live Google validation remain separate. COOP popup browsing-context-group separation, reporting, wildcard-origin/dynamic/deep policy cases, client-hint delegation, additional controlled APIs, broader storage partitioning/access, CSP form-action/Trusted Types and wider parser/resource lifecycle remain pending.

## Retained Window queue admission checkpoint — 2026-10-14

Retained native cross-context Window snapshots are bounded by count and actual fixed snapshot bytes (4096/64MiB defaults). Admission runs after payload getters but before native transfers commit. Recursive getters cannot overrun the current queue; a rejected snapshot preserves ArrayBuffer and MessagePort ownership. A retired or origin-mismatched receiver still completes specified transfers and discards delivery without consuming queue capacity. Actual shared backing-store bytes count toward admission and remain fixed after WASM growth; delivery releases them. Four deterministic host-budget regressions plus the existing messaging/isolation/frame cohort pass. Same-context Window deliveries retain the existing private V8 timer/event path; this is not a claim that every browser task queue or scheduling behavior is complete.

## Permission status and container policy checkpoint — 2026-10-14

Fresh normal Chrome155.0.8059.79 / V8 15.5.35.23 controls measure the unsorted79-feature enumeration order,12 wildcard/default-port/duplicate origin policies, PermissionStatus receiver and prototype brands, constructor identity, own descriptors and change-event dispatch. Native permission query results now inherit an authentic EventTarget and retain private document-owned records. Fake/proxied receivers cannot read them. Dynamic iframe featurePolicy reflects its current allow attribute and source origin without rewriting the policy captured when its child document committed; four header/delegation controls cover this distinction.

This isolated batch passes31 focused default SDK150 policy/resource/Window/lifecycle contracts and strict affected Clippy. It adds no queued native DOM handles: permission status and iframe interface records retain genuine document/element wrappers through ordinary private V8 edges, with existing native ownership accounting. Genuine native155 integrated validation remains separate. Deep descendants, popup group transitions, automatic permission changes, complete OS-controlled permission effects and wider CSP/fetch/storage remain open.

## Permission descriptors and descendant admission checkpoint — 2026-10-14

The reusable permissions-query control measures asynchronous errors for missing/invalid descriptors and wrong receivers, accepted/rejected queried names, result labels, push options, inherited subtype dictionary read order, original getter exception identity and cross-realm Promise/status ownership. Actual Chrome Permissions inherits Object rather than EventTarget; PermissionStatus separately inherits the authentic EventTarget. Native private query records now permit genuine foreign-realm receivers while rejecting fake/proxied receivers and use captured Promise/error intrinsics. These states describe the declared fresh profile; OS permission prompts and operations are not implemented.

The policy-deep controls exercise twelve real two-level document trees: ancestor denials cannot be re-enabled below them, cross-origin feature delegation remains subject to each ancestor, child response policies intersect, and denied isolation stays denied across descendants. All completed fresh Chrome controls have zero errors and source/case/collector hashes. The final isolated checkpoint passes42 defaultSDK150 policy/API/Window contracts and strict affected Clippy. An early5s deep-case budget failure was followed by stage tracing and three passing completed runs; it does not establish a general iframe timing bound. Integrated native155 remains separately required. Exploratory descriptors mutating their subtype name during conversion did not settle in the normal Chrome collector; no completed control or parity claim is made for that path.

## Response ancestor admission checkpoint — 2026-10-14

Native iframe document admission now enforces response CSP frame-ancestors and X-Frame-Options before authenticated response bytes reach the new document. It reads every genuine native parent context/origin, fails closed for retired/missing ancestors and uses a bounded context walk. Enforced frame-ancestors headers intersect and replace legacy framing fields; default-src and report-only headers do not supply this directive. Credentialless exemption for COEP does not exempt framing restrictions. Blocked documents use the existing opaque, script-disabled admission state.

Fresh normal Chrome155.0.8059.79/V8.23 controls cover40 direct cases: self/wildcard/scheme/explicit origins, source path behavior, duplicate/header intersections, report-only non-enforcement, legacy invalid/conflicting tokens, and credentialless denial. Eight real two-level trees prove same-origin with the immediate parent is insufficient when a grandparent is foreign. Regression tests verify exact source/case/collector/child hashes, zero collector errors and complete case counts before comparing results. The isolated defaultSDK150 batch passes35 focused policy/resource/Window/frame contracts and strict affected Clippy. Genuine native155 integrated validation remains required. CSP violation events/reporting, CSP sandbox, full form/navigation policy and popup transitions remain pending.

## CSP sandbox script/origin checkpoint — 2026-10-14

Native response CSP sandbox flags now restrict scripts and origins for top and child documents before page code runs. Intersecting response headers and inherited iframe sandbox flags remain distinct from ignored report-only policies. The nominal Location origin stays the response URL origin; global/message origins, cookies and storage use the actual native opaque principal. Opaque document self Permissions Policy is distinct from an explicitly supplied invalid null URL target, and empty opaque allowlist reflection matches the fresh Chrome control. An opaque top document can still be cross-origin isolated under COOP/COEP; the control proves actual shared backing-store cloning rather than trusting the public flag.

The module loader retains only private Rust boolean origin/site cells and snapshots them before each request; it cannot keep the old nominal document origin across a sandboxed installation. Four real module transport controls compare HTTP Origin and Cookie headers. Same-origin CORS script requests carry Origin; sandboxed opaque module requests carry null and omit the normal jar cookie. Thirteen real iframe and nine top-document controls measure script/handler blocking, storage/cookie errors, native principal/message origins, policy reflection and shared-memory behavior. Completed controls have zero errors and exact source hashes. The isolated defaultSDK150 checkpoint passes39 render/script/frame/input contracts plus8 core request-context checks, with strict affected Clippy/fmt; native155 integration remains required.

Full sandbox form, popup, navigation and download flags remain unsupported and the unsupported-directive diagnostic continues to list sandbox honestly. CSP violation events/reporting remain open. This work also exposed the independent missing HTMLScriptElement.text setter: assigning it creates an own data property rather than updating native source/text children, so the appended script does not execute. Its source-pinned script-text control and native mismatch receipt are retained; a canonical fix is being prepared separately.
