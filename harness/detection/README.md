# Chrome versus turbo-surf detection harness

This diagnostic compares real Chrome with the native V8/rtdom engine using fresh,
independent cookies. Chrome is an oracle; its cookie values are never imported by
the native run. The native example does not invoke a solver or sidecar, or read an
ENID cache. The default flow visits the homepage, moves/clicks/focuses the search
box, types a query and presses Enter. For Google it first clicks the visible
"Reject all" consent button when present. Both initial scripts and subsequent native
JS navigation retries execute in persistent `PageSession`s.

## Build and run

On a Mac with a real Metal GPU:

```bash
npm install
npm install --prefix scripts/browser-sidecar
cd rust
cargo build --release -p turbo-surf-mcp --features trust-anchors,gpu-metal,coretext --example detection_trace
cd ..
npm run harness:detection -- --live --binary rust/target/release/examples/detection_trace --probes --debugger
```

Use an optimized binary for live timing conclusions: debug Rust CSS/layout work
can exhaust the execution budget. Build information records `debugAssertions`,
alongside the exact executable hash. The stock workspace engine remains V8150;
the [experimental155 SDK](./engine-155/README.md) uses its separate build helper
and `.native-builds/sdk155-8059.79/target/release/examples/detection_trace`.

Chrome must be installed. The default oracle uses raw CDP and Node 22 or newer (its native WebSocket API); Playwright/Patchright are optional legacy collectors. Missing binaries/dependencies and launch failures are
reported; live work is skipped unless `--live` is explicit. A sandbox that blocks
Chrome launch, DNS, localhost binding or Metal access must allow those operations
for the corresponding run. Compiled GPU support alone does not prove real GPU
readback: compare the recorded WebGL draw against Chrome.

Each run freezes a copy of the native executable so another Cargo build cannot
replace it between passes. It checks the reported `impersonate`, `trustAnchors`,
`gpuMetal` and `coretext` features. The default native identity is pinned in both
HTTP and JS to avoid the per-host profile mismatch in the wreq path. HTTP
Accept-CH negotiation is shared across each native session's navigations and page
fetches. Script, iframe, beacon and fetch requests use their own metadata instead
of top-level navigation defaults. GPU renderer identity comes from the actual
Metal adapter used for pixel execution.
The Chrome 155 target profile reuses the pinned upstream 154 TLS/H2 recipe and Chromium root store, with separately measured155 headers and seeded client hints. Both target and base recipe versions are recorded. For intentionally reduced
builds/platforms use `--allow-reduced`, and retain that fact in comparisons.

The default four passes are Chrome/native baseline, then Chrome/native interface
tracing. `--native-first` reverses each pair; `--repeat 2` repeats the matrix with
fresh state. Change order/repeat when testing instrumentation or time/rate effects.
A rejected result is data, not a harness failure. Crashes/timeouts/launch failures
set a nonzero exit code, preserve logs, and allow the other passes to complete.
`captureComplete` describes collector completion. Per-page hydration diagnostics,
readiness and actual navigation determine whether execution completed. A consent
`clicked` flag records accepted input dispatch; verify the response, cookie effects
and subsequent page state before treating consent as completed.

```bash
node harness/detection/run.mjs --live --out /tmp/search-trace --native-first --repeat 2
node harness/detection/run.mjs --live --url https://example.test/ --no-interaction --success-selector '#ready'
node harness/detection/run.mjs --live --steps /tmp/my-steps.json --tz America/Los_Angeles
node harness/detection/compare.mjs /tmp/search-trace
```

Two independent localhost Chrome oracles capture frame behavior without a Google
request or cookie transfer:

```bash
node harness/detection/frame-location.mjs /tmp/frame-location-oracle
node harness/detection/frame-policy.mjs /tmp/frame-policy-oracle
```

They record cross-origin Window/Location descriptors and navigation, iframe moves,
retained documents/callbacks, sandbox script permissions, and opaque data frames.
Each writes the actual Chrome version and structured results. Matching native
regressions live in `turbo-surf-render/tests/render.rs`; they use native contexts
and localhost fixtures rather than the Chrome process.

Other options: `--binary PATH`, `--query TEXT`, `--selector CSS`, `--wait MS`,
`--width PX`, `--height PX`, `--dismiss-selector CSS`, `--page-budget MS`,
`--capture-timeout MS`

Native microtask controls use the same fresh normal Chrome collector:

```bash
node harness/detection/authored-dom-chrome.mjs harness/detection/microtask-order-probe.js harness/detection/fixtures/microtask-order-chrome155.json
node harness/detection/authored-dom-chrome.mjs harness/detection/microtask-frame-probe.js harness/detection/fixtures/microtask-frame-chrome155.json
cd rust
cargo test -p turbo-surf-render --test microtask_contracts
```

The offline contracts compare queue ordering and iframe retirement with the saved
controls, and verify trusted exception delivery and watchdog recovery.

Original inline module source units and event behavior have a separate control:

```bash
node harness/detection/authored-dom-chrome.mjs harness/detection/dom-utf16-module-probe.js harness/detection/fixtures/dom-utf16-module-chrome155.json
cd rust
cargo test -p turbo-surf-render --test dom_utf16 --test dom_utf16_frames --test script_text
```

The native155 SDK runs the real frame/module cases; those cases return early on
the default SDK. Successful inline modules are observed through their actual body
execution, because normal Chrome does not fire load for successful inline scripts.
(Chrome wall-time dwell; native drains its existing lifecycle/virtual-timer pump).
`--steps` is a JSON array replacing default interactions. Supported steps are
`{move:{selector}}`, `{click:selector}`, `{focus:selector}`, `{type:text}`,
`{press:key}`, `{wait:ms}`. The Chrome input delay is 110ms; native uses its
private native host-input driver, captured in a context slot before page scripts.
Input respects cancellation and its execution budget; timed-out queued edits are
canceled, and the session remains usable. These gestures are comparable intent, not identical
motion/clock samples. The default Google dismissal selector is `#W0wltc`;
an empty `--dismiss-selector ''` disables it. Other sites default to no dismissal.
The harness runs explicit wait steps and the post-dismissal wait against real
monotonic elapsed time, pumping pending native callbacks between sleeps. The
underlying host input driver uses virtual timers; advancing those timers alone
would allow search to replace the document before delayed consent responses and
their cookies arrive. Explicit waits accept integers from 0 to 60000 ms.
The conditional click is recorded before typing; its presence alone does not
prove that the consent dialog closed. Snapshots retain the query value, focus and
consent state so an unsuccessful input flow is distinguishable from a rejected
search. Native captures checkpoint partial `trace.json` after each phase and
record `captureComplete=false` until successful completion. Comparison output
retains hydration budget exhaustion and input errors. The page budget defaults
to 20000 ms (maximum 120000); each subprocess timeout defaults to 180000 ms
(maximum 600000). Increasing these limits is diagnostic, not proof of parity.
`--probe PATH` appends a shared JavaScript collector (repeatable); its source hash
is retained in the manifest. Collectors run after the verdict snapshot and also
on a separate shared localhost document pair, so probe mutations cannot establish clearance.

Both runtimes use the same configuration for the real inner viewport, browser outer
window, screen and device scale. Chrome's window bounds are set through its browser
API; the native runtime receives the corresponding fingerprint configuration. The
default is 1280×800 inside a 1280×888 window, a 1920×1080 screen and scale 2. Browser
preferences preserve `navigator.languages` as `['en-US','en']`; the native HTTP profile
uses the same ordered list. Each snapshot records the resulting runtime values, so
configuration mismatches remain visible. The frozen binary's `--build-info` preflight
checks compiled features before any target requests and records the actual V8 version
and transport Chrome template separately from the reported navigator identity.

The reusable localhost lifecycle/resource control uses the same HTML and PNG fixture
as `tests/lifecycle_resources.rs`:

```bash
node harness/detection/lifecycle.mjs --run --out /tmp/turbo-lifecycle
```

It compares ready-state events, deferred scripts, stylesheet imports, parsed controls,
valid/invalid image decoding, dynamic image/link loads and the runtime environment.
Host events record trust, targets, dispatch phase and state after dispatch.
The interface tracer preserves native host-pump observations separately as
`runtimeInterfaces`; `interfaces` compares page call sites. This attribution uses
the immediate source location in the captured stack, and is a diagnostic hint rather
than a security boundary. Older traces without attribution retain their original totals.
It preserves both raw traces and exact difference paths in `comparison.json`. The
control covers those contracts; it does not simulate incremental HTML parsing,
every resource type or a complete browser loader.

A separate server-gated async/defer fixture proves that a pending classic async
script permits DOMContentLoaded and delays load. It records native script load
events and catches duplicate property-handler delivery:

```bash
node harness/detection/script-lifecycle.mjs --binary /path/to/detection_trace --out /tmp/turbo-script-lifecycle
```

The same HTML is exercised by the Rust lifecycle regression. Explicit ES modules
have separate shared localhost fixtures:

```bash
node harness/detection/modules.mjs --run --binary /path/to/detection_trace --out /tmp/turbo-modules
node harness/detection/modules.mjs --run --evaluation --binary /path/to/detection_trace --out /tmp/turbo-module-evaluation
node harness/detection/modules.mjs --run --frames --binary /path/to/detection_trace --out /tmp/turbo-frame-modules
```

The first checks real import graphs, source URLs, null module `currentScript`,
async modules gated on DOMContentLoaded, HTTP/MIME failures and once-only trusted
script events. `--evaluation` checks top-level await completing after document
load, actual Window `ErrorEvent`s for parse/runtime exceptions and graph caching.
`--frames` uses identical native/Chrome child sources and checks independent realms,
per-context caches, child DOM ownership and module/DCL/load ordering. Each mode
can run `--chrome-only`. Outputs retain raw traces, fixture/source hashes,
binary/build identity and semantic failures. The exact native154 SDK supplies the
actual evaluation-stage hook and independent child module maps; the published Deno
fallback has partial TLA/error lifecycle support and rejects child modules.
Incremental HTML parser races remain outside these controls.

Two additional controls exercise actual script credentials and response policy:

```bash
node harness/detection/script-credentials.mjs --binary /path/to/detection_trace --out /tmp/turbo-script-credentials
node harness/detection/csp.mjs --binary /path/to/detection_trace --out /tmp/turbo-csp
node harness/detection/redirect-policy.mjs --binary /path/to/detection_trace --out /tmp/turbo-redirect-policy
```

The credentials control serves two localhost origins and records cookie presence,
Origin headers, redirect hops, final module source/import identity, anonymous versus
`use-credentials` graphs, and JavaScript MIME/nosniff failures. The CSP control serves
an actual policy header, permitted nonce/hash scripts and a permitted module dependency,
then verifies that denied scripts, fetch, stylesheet, image and iframe URLs never reach
the server. Both freeze the executable, retain source/build hashes and raw traces, and
support `--chrome-only` with the raw CDP browser backend. The redirect control checks
same-origin Authorization preservation, permanent cross-origin stripping, manual
opaqueredirect filtering, error-mode rejection and actual per-hop preflight for an
unsafe A→B→A request. Denied preflight must prevent contact with the unsafe method.
Their semantic checks do not certify every fetch option or CSP directive. Incremental parser races, strict-dynamic, reporting,
Trusted Types, integrity and navigation/worker policy coverage remain separate gaps.

CSS controls exercise inline policy separately from network stylesheet access:

```bash
node harness/detection/css-security.mjs --binary /path/to/detection_trace --out /tmp/turbo-css-policy
node harness/detection/stylesheet-origins.mjs --binary /path/to/detection_trace --out /tmp/turbo-css-origins
```

The first checks native nonce authorization, blocked authored attributes versus effective
CSS declarations, CSSOM mutation, Attr value changes, removal and dynamic styles. The second
serves two origins and checks real CORS/credentials, opaque rules, inline imports, redirects
and MIME/nosniff error events. Both freeze the supplied executable, report its actual engine
and feature metadata, retain the installed Chrome version, and hash the shared fixture.
`--chrome-only` and `--native-only` allow independent diagnostics. These finite controls do not
claim complete CSS grammar, media-query, layout or CSP conformance.

The parsed-document fixture compares native tree ownership, inert scripts, metadata,
`importNode`, parser interface shapes, and default sanitization on identical HTML:

```sh
node harness/detection/document-parser.mjs --run --binary /path/to/detection_trace --out /tmp/turbo-document-parser
```

`--chrome-only` records an independent browser reference. Each run freezes the native
executable and records its build information, source hash, traces, and a field comparison.
HTML parsing is implemented; XML and custom sanitizer configurations throw explicit
`NotSupportedError`s. Native template-content imports now preserve ownership;
unsupported imported node kinds still fail explicitly. This fixture does not
certify arbitrary HTML sanitization or XML support.

Native follows up to five JS navigation hops and ten HTTP redirects per fetch.
Where Enter does not perform browser default submission, it records a GET form
fallback separately. The fallback now reads control `.value` properties, including parsed hidden
inputs. It remains a harness accommodation for default form submission, rather
than proof that every native form action matches Chrome. `--no-interaction` does not submit the fallback.

## Artifacts

Default output is `harness/detection/artifacts/<timestamp>` (gitignored). Use a
fresh directory; an existing run is refused. New directories are private and raw
files use mode 0600 on Unix. Cookies, tokens, HTML, query strings and POST bodies
can contain session secrets; keep raw artifacts local.

- `manifest.json`: checkout commit/branch, dirty diff hash, executable hash/mtime,
  harness hashes, complete streamed dirty-diff hash and untracked source hashes, actual native build diagnostics, per-process exit/timeout status. This records provenance; rebuild
  first because version numbers/mtime alone cannot establish source correspondence.
- `<runtime>-baseline/` and `<runtime>-interfaces/trace.json`: page snapshots,
  cookie names (Chrome) / raw jar (native), errors, feature flags and observations.
- Native `network.jsonl`: opt-in application-layer requests, response headers,
  decoded response bodies, HTTP protocol and correlated IDs/timestamps. It covers
  page fetch/XHR/beacons/scripts that use the native net stack. Request headers
  include the derived request context, negotiated client hints and cookies, but
  exclude transport-inserted headers such as Accept-Encoding and wire order.
  Manual redirects expose intermediate headers/cookies; automatic client redirects
  elsewhere do not. Body reads retain the engine's normal caps and charset decode.
- Chrome trace: raw CDP captures requests, headers, payloads, redirect chains,
  timing, failures, decoded document/script/XHR/fetch response files, errors,
  frames and websocket messages. Network ExtraInfo is enabled in baseline and
  traced passes. Session-qualified correlation keeps worker/frame request IDs
  separate. Attached out-of-process frames and dedicated/shared/service workers
  are recorded separately; source/body retrieval failures remain visible.
- `--debugger`: the instrumented passes enable the real V8 Inspector in both
  engines. Chrome records compiled inline, eval, Function and module sources by
  SHA256 per attached session. Native saves per-isolate `native-js/isolate-N/`
  inventories and source files, including genuine child contexts on engine 154.
  Inventories report source limits, dropped records, pending retrievals and
  errors. Debugger attachment changes execution; use baseline as the control.
- `comparison.json`: endpoint counts, query/payload field names/hashes, header
  scope, observed interface counts, differing argument/result samples and stacks,
  separate main/child/worker observations, pages/errors/features and shared-probe
  results. `traceCoverage` retains incomplete network records and native script
  inventories. Raw files retain full payloads and sources.

Interface tracing wraps configurable getters/functions and selected setters
before page scripts, preserving native receivers. It records first stacks and
bounded primitive samples (160 characters, 12 variants, 3000 distinct access keys).
It reports skipped descriptors/dropped records. Instrumented Chrome additionally
uses CDP Network; raw redirect events remain available if header correlation is
ambiguous. Wrapping changes function identity
and source: this is an **intrusive diagnostic**, and our Chrome traced pass can be
rejected even when baseline passes. Snapshot/driver calls also appear in these
observations; use the recorded stack/source to distinguish harness queries.

Coverage is explicit: data-property reads and nonconfigurable fields cannot all
be trapped. Main, child and worker snapshots remain separate. Fast destroyed
contexts can lose their final interface snapshot. Native worker execution is
unsupported; the published V8 150 fallback does not register child debugger
contexts. The native Inspector inventories generated sources without replacing
`eval` or `Function`. Limits and errors are reported, so absence of a call is not
proof of complete coverage or the server's rejection reason. This runner does
not capture TLS/HTTP2 packets; the localhost ClientHello control is separate.

The installed Chrome updated to 155.0.8059.40 on 2026-10-13. The genuine
[155 source profile](./engine-155/README.md) builds V8 15.5.35.20-rusty. The
HTTP profile targets 155 with measured seeded brands and header order; its upstream
TLS/H2 recipe is still 154. Record the actual browser, V8, target and base transport
versions for each run. An unchanged official Chrome-for-Testing 154.0.8037.92
has V8 15.4.80.19 (native is 15.4.80.20-rusty), and its standard launch currently
stalls network CDP operations on this Mac with sandbox-extension errors. It is
not a usable network oracle here. `--chrome-path` selects another installed
browser explicitly; it does not change engine or User-Agent versions.

## Existing detection tools

`--probes` runs the shared capability, API-shape, BotGuard-integrity and WebGL collectors after
verdict snapshots where possible, plus a separate uninstrumented blank-page pair
so rejection/trace wrappers do not prevent or contaminate capability measurement.
These are our queries, distinct from observed site accesses. The capability
collector checks parsed hidden/textarea/select values and form ownership, independent intrinsic prototypes and Function globals as well as
CSS rule parsing; standalone blank iframe realms are real V8 contexts. Both runtimes now use identical localhost HTML and URL for the isolated pair,
so secure-context exposure is comparable. Old about:blank Chrome / HTTPS native
pairs had different secure contexts; their enumeration differences need remeasurement. Both captures record
the effective viewport/window/screen/language configuration.

You can run them independently:

```bash
cd rust
cargo run -p turbo-surf-mcp --features trust-anchors,gpu-metal,coretext --example fp_snapshot -- --render ../scripts/browser-sidecar/probes/webgl-fp.js
cd ..
node scripts/browser-sidecar/run-probe.mjs scripts/browser-sidecar/probes/webgl-fp.js about:blank --tz America/Los_Angeles
```

The botto harness runs the sibling project's actual collector/checks. Force the
current addon instead of letting its loader select a stale release:

```bash
cd rust
cargo build -p turbo-surf-napi --features turbo-surf-core/trust-anchors,gpu-metal,coretext
cd ..
cp rust/target/debug/libturbo_surf_napi.dylib /tmp/turbo-surf-full.node
codesign --force --sign - /tmp/turbo-surf-full.node
node harness/botto-detect/run.mjs --addon /tmp/turbo-surf-full.node --json
# Also force current bindings when running the Playwright shim:
TURBO_SURF_NAPI_PATH=/tmp/turbo-surf-full.node npm run test:shim
```

`probe`, `probe_mint`, `browserless_google_serp`, `fp_snapshot`, the legacy Chrome
`net-trace.mjs`, and botto remain available. MCP recon helpers use different page
script/cookie/navigation paths from this harness; their cookie/marker heuristics
are not independent Google attestation. See [FINDINGS.md](./FINDINGS.md).

Offline checks:

The API contract audit uses a minimal localhost page and fresh, uninstrumented
Chrome/native sessions. It captures method names/arities, descriptors, ownership
chains, constructor behavior, prototype tags, and invalid-receiver errors. The
constructibility check never invokes the tested constructor; network APIs are
inspected without calling them. It makes no search-engine requests.

```bash
# Build detection_trace using the feature command above, then:
node harness/detection/api-shape.mjs --run --out /tmp/turbo-api-shape
# A specific executable is accepted with --binary /absolute/path/detection_trace.
```

`comparison.json` retains both complete measurements and sorted difference paths,
the Chrome version, native engine/template metadata, and executable/collector
hashes. The binary is frozen before collection. These are API contract differences,
not evidence that a site queried them or used them to reject a request. A
`[native code]` string is only the measured string; it does not prove native
implementation. Raw localhost trace artifacts remain in the chosen output directory.

```bash
npm run test:detection
cd rust
cargo test -p turbo-surf-render --test detection_trace -p turbo-surf-core --test http_trace
```

### Localhost TLS oracle

Capture the production native ClientHello and a fresh local Chrome ClientHello
without HTTP traffic, cookies, certificate verification shortcuts, or Google requests:

```bash
cd rust
cargo build -p turbo-surf-core --example transport_hello
cd ..
node harness/detection/authored-transport-chrome.mjs rust/target/debug/examples/transport_hello /tmp/turbo-transport-oracle
```

The direct runner launches normal installed Chrome with a fresh temporary profile,
records its actual version and the native executable SHA256, and shares no cookies.
`comparison.json` contains parsed cipher/extension metadata, TLS versions, ALPN
order, key-share groups/lengths and public root IDs. Separate native/Chrome binary
records preserve the exact ClientHello bytes, including randoms, original extension
order, key shares and ECH payloads. The normalized comparison removes GREASE values,
extension permutation and randomized ECH payload length while retaining GREASE
presence, algorithm order and exact trust-anchor IDs.

[TRANSPORT-155.json](./TRANSPORT-155.json) records the fresh Chrome 155.0.8059.80
TLS and HTTPS/H2 controls, with zero differences in the measured normalized fields.
The raw ClientHello fixtures are hash checked by `transport-hello.test.mjs`.
This is localhost capability evidence; it does not establish complete transport or
browser parity, Google acceptance, or successful native SERP rendering. The native
upload admission regression additionally captures normal/upload ClientHellos and
checks that protocol admission preserves the ordinary ALPN offer. The underlying
TLS/H2 recipe is still the upstream 154 recipe and remains identified separately.

The authored HTTPS upload controls use only their checked fixture certificate
(SPKI exception in normal Chrome, explicit certificate store in the native host):

```bash
node harness/detection/authored-upload-chrome.mjs harness/detection/upload-probe.js /tmp/upload-control.json
node harness/detection/authored-upload-chrome.mjs harness/detection/stream-transfer-probe.js /tmp/stream-transfer-control.json
```

The upload probe covers streaming before source EOF, forced CORS preflight,
HTTP/1 admission, 303/307 redirects, cancellation, bounded backpressure and early
responses. A separate stalled-peer probe verifies immediate AbortSignal rejection
without waiting for bounded native transport cleanup. The 20-case stream-transfer
control records genuine Readable/Writable/Transform, Window/MessagePort, error and
failure-atomicity behavior. Stream transfers are integrated with additional adversarial
and GC controls; see [STREAM-TRANSFERS.md](./STREAM-TRANSFERS.md). The original
20-case capture is one subset of those controls.

The shared native iframe fixture exercises detached nodes, fragment insertion,
trusted load events, removal and reinsertion. It uses the same baseline/instrumented
trace capture and executable provenance as the search diagnostic:

```bash
node harness/detection/iframe-lifecycle.mjs --out /tmp/surf-iframe-trace
```

For the normal-Chrome WindowProxy operation oracle (same/cross-origin navigation,
retained Function compile/run, prototype/reflection permissions), run:

```bash
node harness/detection/window-operations.mjs /tmp/surf-window-operations
```

Both collectors serve only localhost fixtures. The WindowProxy collector writes
measured Chrome behavior; it does not switch the native runtime or claim complete
browser security conformance. The isolated exact-engine SDK primitive regressions
are documented in [engine-154/README.md](engine-154/README.md).

The reusable Document probe checks live/static collections, native indexed and named
reflection, compatibility mode, doctype metadata, scalar reflection, handler order,
node adoption identity/listeners and private nonce cloning against normal Chrome:

```bash
node harness/detection/api-shape.mjs --run --probe harness/detection/document-basics-probe.js --out /tmp/surf-document-basics
node harness/detection/api-shape.mjs --run --probe harness/detection/html-reflections-probe.js --out /tmp/surf-html-reflections
node harness/detection/api-shape.mjs --run --probe harness/detection/document-character-data-probe.js --out /tmp/surf-document-character-data
```

These probes use localhost fixtures and do not transfer browser cookies.
The character-data probe covers native processing instructions, UTF-16 offsets,
HTML CDATA rejection, clone/import/adoption, node-value conversions and the
supported HTML XML-metadata defaults and setters.

The itemized parity backlog is [PARITY-GAPS.md](./PARITY-GAPS.md). Native source
profiles are revision pinned: [Chrome155](./engine-155/README.md) and the historical
[Chrome154](./engine-154/README.md). Changing HTTP/navigator configuration does
not change the actual V8 engine. Use the155 helper and its validated native build
for a version-matched155 comparison.

Isolated probes use identical HTML and a shared loopback origin for both runtimes.
Fresh Chrome `about:blank` is insecure; comparing its secure-only API enumeration
with native `https://probe.test/` inflated apparent gaps in older probe artifacts.
The corrected probe configuration records its fixture bytes/hash and actual URL.

### Resource admission and script errors

`resource-policy.mjs` starts two loopback origins and compares exact raw-byte SRI
hashes, strongest-digest selection, opaque/CORS admission, CORP header policies,
script/module/dependency/iframe execution, stylesheet installation and trusted
resource events. `classic-script-errors.mjs` distinguishes downloaded resources
from execution errors: exceptions keep their identity and source realm, report
trusted Window errors, and fetched scripts still fire `load`. Both runners freeze
the selected executable and record native/Chrome versions and collector hashes.

```bash
node harness/detection/resource-policy.mjs --run --binary /absolute/path/detection_trace --out /tmp/surf-resource-policies
node harness/detection/classic-script-errors.mjs --run --binary /absolute/path/detection_trace --out /tmp/surf-script-errors
```

These are authored localhost controls. They do not establish complete CSP,
COEP/COOP, mixed-content image upgrades, streaming, or browser scheduling parity.
[The saved script-error control](./fixtures/classic-script-errors-chrome155.json)
records normal Chrome 155 behavior without importing its cookies.

Authored localhost controls can set `disableExtensions: true` in their JSON
configuration. This adds only `--disable-extensions`, rejects non-loopback target
URLs and records `effectiveExtensionFlags` in the trace. The prototype/API
comparison and worker/iframe regression use this explicit isolation so installed
system extensions cannot delay their control targets. Normal live Chrome launches
keep their default extension behavior.

`range-live-probe.js` and its saved Chrome 155.0.8059.79 / V815.5.35.23 fixture
cover live boundary changes for text, splitting, insertion/removal, replacements,
normalization, moves and adoption. The fixture retains the exact collector hash;
canonical Rust tests also check cross-realm behavior, hostile public getters and
actual garbage collection. Remaining native mutation seams and layout geometry
are listed in `PARITY-GAPS.md`.

## Native retention diagnostics

The authored iframe GC regression compares actual weak-object liveness against
Chrome. To capture a native failure's root paths without changing its oracle:

```sh
cd rust
TURBO_SURF_GC_HEAP_SNAPSHOT=/tmp/frame.heapsnapshot cargo test -p turbo-surf-render --test frame_gc_contracts -- --test-threads=1
cd ..
python3 harness/detection/heap-retainers.py /tmp/frame.heapsnapshot
```

The host diagnostic writes a snapshot only if the release comparison fails. The
analyzer starts at objects marked by `__nativeLifecycleDiagnostic`, excludes weak
edges and conditional WeakMap pair edges, and omits WeakRef job retention by
default. Use `--include-kept-objects` to inspect that temporary root separately.
Shortest unconditional paths identify strong native roots; they do not completely
model ephemeron reachability. The removed-iframe initializer regression was traced
through the main Window's private initializer to the last child Document. Heap
snapshots remain local; they can contain page memory.

## In-flight work and restart recovery

Keep unfinished source candidates and their exact patches, source hashes and test
receipts under repository-local `.native-builds/recovery/`, which is ignored.
Temporary-directory candidates can disappear across restarts. Save reviewed
canonical changes in turbo-test before re-vendoring; preserve an unfinished source
packet until its tested equivalent is committed. Raw live traces stay under the
ignored artifacts directory. Remove obsolete owned build outputs after integration.

The authored `element-interface-probe.js` records namespace/local-name constructor
identity and borrowed control brands across active/inert HTML and XML documents.
Its exact Chrome source hash and 723 observations are saved in the canonical
binding's `element_interface_chrome155.json`. Current tests compare all HTML/null
interface fields and foreign control brands. SVGScriptElement/SVGImageElement
projection and resources remain recorded gaps; their observations are retained.

### Authored COEP and script preparation controls

These collectors launch fresh normal installed Chrome against localhost. They do
not provide cookies or other session state to turbo-surf. Each saved JSON records
actual browser/V8 identity and collector/probe hashes; redirect/resource collectors
also record real request hops and relevant fixture hashes. Run from the repository:

```sh
node harness/detection/coep-chrome.mjs harness/detection/fixtures/coep-chrome155.json
node harness/detection/coep-dom-chrome.mjs harness/detection/fixtures/coep-dom-chrome155.json
node harness/detection/coep-redirect-chrome.mjs harness/detection/fixtures/coep-redirect-chrome155.json
node harness/detection/coep-header-chrome.mjs harness/detection/fixtures/coep-header-chrome155.json
node harness/detection/coep-inherit-chrome.mjs harness/detection/fixtures/coep-inherit-chrome155.json
node harness/detection/authored-dom-chrome.mjs harness/detection/dynamic-script-probe.js harness/detection/fixtures/dynamic-script-chrome155.json
node harness/detection/dynamic-script-ordering-chrome.mjs harness/detection/fixtures/dynamic-script-ordering-chrome155.json
node harness/detection/authored-dom-chrome.mjs harness/detection/retired-script-probe.js harness/detection/fixtures/retired-script-chrome155.json
cargo test --manifest-path rust/Cargo.toml -p turbo-surf-render --test coep_resources --test dynamic_script_preparation
```

The first control contains 54 direct fetch cases; the DOM control records 15
requests; redirect controls contain 12 cases/23 actual hops. Field parsing has 12
syntax controls and inherited srcdoc fetches preserve child-realm error brands.
Native regressions use only saved authored controls and localhost servers. Do not
change oracle values to match native failures: recapture after changing any source,
and keep the saved actual engine identity.


### CSP metadata and base URL timing controls

Use the generic fresh normal Chrome collector for each source-pinned authored probe:

```sh
node harness/detection/authored-dom-chrome.mjs harness/detection/csp-base-probe.js harness/detection/fixtures/csp-base-chrome155.json
node harness/detection/authored-dom-chrome.mjs harness/detection/csp-meta-probe.js harness/detection/fixtures/csp-meta-chrome155.json
node harness/detection/authored-dom-chrome.mjs harness/detection/csp-base-order-probe.js harness/detection/fixtures/csp-base-order-chrome155.json
node harness/detection/authored-dom-chrome.mjs harness/detection/csp-base-retired-probe.js harness/detection/fixtures/csp-base-retired-chrome155.json
cargo test --manifest-path rust/Cargo.toml -p turbo-surf-render --test csp_document_policy
```

The saved controls distinguish parser insertion, compound fragment insertion,
policy changes after accepted base freezing, HTML namespace/head membership and
retained inactive documents. See [RESOURCE-LIFECYCLE.md](./RESOURCE-LIFECYCLE.md)
for genuine task roots and the metadata-only retirement consumer.

### Authored isolation, permissions and credentialless controls

The reusable header collector executes each saved response-header case in a fresh normal Chrome profile. The frame collector runs identical source-pinned child programs; its optional fourth argument selects the child program. Native replay is in `tests/isolation_policy.rs` and requires only localhost.

```bash
node harness/detection/policy-header-chrome.mjs harness/detection/fixtures/isolation-cases.json harness/detection/isolation-probe.js harness/detection/fixtures/isolation-chrome155.json
node harness/detection/policy-frame-chrome.mjs harness/detection/fixtures/policy-frame-cases.json harness/detection/policy-frame-probe.js harness/detection/fixtures/policy-frame-chrome155.json
node harness/detection/policy-frame-chrome.mjs harness/detection/fixtures/policy-shared-cases.json harness/detection/policy-shared-probe.js harness/detection/fixtures/policy-shared-chrome155.json harness/detection/policy-shared-child.js
node harness/detection/policy-frame-chrome.mjs harness/detection/fixtures/policy-credentialless-cases.json harness/detection/policy-credentialless-probe.js harness/detection/fixtures/policy-credentialless-chrome155.json harness/detection/policy-credentialless-child.js
```

Controls cover native isolation capability, real iframe admission and delegation, shared backing-store deliveries/messageerror, and actual credentialless navigation/fetch cookies with local/session storage separation. They do not establish popup browsing-group separation, every controlled API, complete dynamic/deep policy or Google acceptance.

### Permission descriptors and descendant policies

The authored collectors use fresh normal Chrome profiles against localhost. Rebuild the source-pinned controls with:

```sh
node harness/detection/authored-dom-chrome.mjs harness/detection/permissions-query-probe.js harness/detection/fixtures/permissions-query-chrome155.json
node harness/detection/policy-frame-chrome.mjs harness/detection/fixtures/policy-deep-cases.json harness/detection/policy-deep-probe.js harness/detection/fixtures/policy-deep-chrome155.json harness/detection/policy-deep-child.js
```

The offline `isolation_policy` tests compare actual PageSession outputs to these saved controls. Permission query covers conversion/error timing, property read order, genuine cross-realm receivers and result ownership. Descendant controls cover two real frame levels and twelve header/container/origin combinations. This coverage does not certify popup transitions, OS permission prompts or complete browser lifecycle parity.

### Response ancestor controls

```sh
node harness/detection/policy-frame-chrome.mjs harness/detection/fixtures/policy-ancestors-cases.json harness/detection/policy-frame-probe.js harness/detection/fixtures/policy-ancestors-chrome155.json
node harness/detection/policy-ancestors-chrome.mjs harness/detection/fixtures/policy-ancestor-tree-cases.json harness/detection/policy-deep-probe.js harness/detection/fixtures/policy-ancestor-tree-chrome155.json harness/detection/policy-ancestor-tree-child.js
```

The descendant collector selects authenticated leaf response headers independently of the middle frame. Offline regressions verify source hashes and case counts before testing actual native iframe admission. Violation reporting and full navigation policy are separate unfinished behavior.

### CSP sandbox controls

```sh
node harness/detection/policy-frame-chrome.mjs harness/detection/fixtures/policy-sandbox-cases.json harness/detection/policy-sandbox-probe.js harness/detection/fixtures/policy-sandbox-chrome155.json harness/detection/policy-sandbox-child.js
node harness/detection/policy-header-chrome.mjs harness/detection/fixtures/policy-sandbox-top-cases.json harness/detection/policy-sandbox-top-probe.js harness/detection/fixtures/policy-sandbox-top-chrome155.json
node harness/detection/policy-sandbox-module-chrome.mjs harness/detection/fixtures/policy-sandbox-module-cases.json harness/detection/policy-sandbox-module-probe.js harness/detection/fixtures/policy-sandbox-module-chrome155.json
```

The module collector returns the actual server-observed Origin and Cookie fields from an ES module; it seeds only its own localhost cookie. The top control proves genuine shared backing-store behavior in the opaque isolated case. The script-text probe separately records an open reflected script-source setter gap. Full sandbox navigation/form/popup behavior and reporting are not certified by these controls.

## Current parity and lifecycle tracking

[PARITY-GAPS.md](./PARITY-GAPS.md) is the canonical open checklist. Every residual
category outside completed PR26 scope is tracked in
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28), with audio in
[issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27).
[API-INVENTORY.json](./API-INVENTORY.json) retains exact missing/extra member and
descriptor paths for its frozen executable; rerun `api-shape.mjs` after integration
before using its historical counts as current results.

After the paired capture, regenerate the complete durable backlog, including global
ownership, callable descriptors and raw property order:

```bash
node harness/detection/api-inventory.mjs /tmp/turbo-api-shape/comparison.json harness/detection/API-INVENTORY.json
```

The expanded audit also measures forms, collections, Blob/File, FormData,
CustomStateSet and ValidityState. Absent interfaces remain separate from missing
members on measured interfaces. [HOST-MEDIA.md](./HOST-MEDIA.md) describes the
independent OS-preference/media-query control and its platform-specific bounds.

The reusable authored GC collector runs a localhost probe in a fresh normal
Chrome profile, invokes actual CDP heap collection between jobs, then compares
retained objects before and after releasing guest roots:

```bash
node harness/detection/authored-gc-chrome.mjs harness/detection/abstract-ranges-lifecycle-cases.json harness/detection/abstract-ranges-lifecycle-probe.js /tmp/abstract-ranges-gc.json
```

The probe returns `prepared` and installs `__authoredLifecycle.read()` and
`.release()` for the trusted collector. Records retain browser/V8 and exact source
hashes. Production `tests/abstract_ranges.rs` verifies static boundary behavior,
real iframe receiver brands and actual native weak-object liveness against those
independent controls. A passing GC control certifies its retained/released graph,
not universal DOM collection behavior.

Native form-navigation verification can prohibit the historical synthesized GET
fallback with `--require-native-form-navigation`. A real accepted form response
retains its native installed URL/status/referrer and monotonically counted document
identity, including same-URL POSTs. Subsequent page-script redirects are still
followed; the accepted current document is not fetched again. Native trace tests
can select a freshly built executable with `TURBO_SURF_DETECTION_BINARY`.

`--no-form-fallback` allows genuine form commits and page-script navigation while
disabling synthesized GET serialization. Use it for a native search verdict when
the site may choose either legitimate navigation path. The stronger
`--require-native-form-navigation` is useful when testing specifically the native
form default action. Neither option imports Chrome cookies.

Interface rows include the first synchronous call's `durationMs`, cumulative
`totalDurationMs` and `maxDurationMs`; retained samples include their own duration.
The captured performance clock brackets the original function/getter/setter,
including throws, before argument/stack recording. Nested calls are inclusive, so
aggregate durations may overlap. Promise results retain identity and are never
awaited; these measurements cover synchronous invocation only.

The localhost activation oracle uses real Chrome input and records outgoing
`Sec-Fetch-User` headers alongside public activation state:

```bash
node harness/detection/activation-headers-chrome.mjs /tmp/activation-headers.json
node --test harness/detection/activation-headers.test.mjs
cd rust
cargo test -p turbo-surf-render --test user_activation --test form_navigation --test host_input
```

Its 14 cases cover script submission, untrusted events, trusted clicks/keys,
pointerdown cancellation, real elapsed expiry, Location handlers, HTTP redirects,
same-origin sticky state and cross-origin reset. The checked-in Chrome control is
source-pinned. `native.test.mjs` separately checks actual CLI request headers and
automatic retries against localhost servers using the selected detection binary.

Abort state and stream reaction order have a separate reusable Chrome control:

```bash
node harness/detection/authored-dom-chrome.mjs harness/detection/abort-bridge-order-probe.js /tmp/abort-bridge-order.json
```

The full Chrome fixture and pre-adapter Deno phase order are retained independently.
`event_contracts` checks Chrome state/reasons/timeouts and native phase-order
preservation; the measured remaining queue-order difference is listed in
[PARITY-GAPS.md](./PARITY-GAPS.md). Two official Deno event modules receive a
hash-guarded adapter for private dense-array operations. Their original dispatch,
timeout and dependent-signal algorithms remain in place, and public Array is
unchanged.
