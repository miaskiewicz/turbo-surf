# Chrome versus turbo-surf detection harness

This diagnostic compares real Chrome with the native V8/rtdom engine using fresh,
independent cookies. Chrome is an oracle; its cookie values are never imported by
the native run. The native example does not invoke a solver or sidecar, or read an
ENID cache. The default flow visits the homepage, moves/clicks/focuses the search
box, types a query and presses Enter. Both initial scripts and subsequent native
JS navigation retries execute in persistent `PageSession`s.

## Build and run

On a Mac with a real Metal GPU:

```bash
npm install
npm install --prefix scripts/browser-sidecar
cd rust
cargo build -p turbo-surf-mcp --features trust-anchors,gpu-metal,coretext --examples
cd ..
npm run harness:detection -- --live --probes --debugger
```

Chrome must be installed. Missing binaries/dependencies and launch failures are
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
`trust-anchors` requires the workspace's vendored wreq. For intentionally reduced
builds/platforms use `--allow-reduced`, and retain that fact in comparisons.

The default four passes are Chrome/native baseline, then Chrome/native interface
tracing. `--native-first` reverses each pair; `--repeat 2` repeats the matrix with
fresh state. Change order/repeat when testing instrumentation or time/rate effects.
A rejected result is data, not a harness failure. Crashes/timeouts/launch failures
set a nonzero exit code, preserve logs, and allow the other passes to complete.

```bash
node harness/detection/run.mjs --live --out /tmp/search-trace --native-first --repeat 2
node harness/detection/run.mjs --live --url https://example.test/ --no-interaction --success-selector '#ready'
node harness/detection/run.mjs --live --steps /tmp/my-steps.json --tz America/Los_Angeles
node harness/detection/compare.mjs /tmp/search-trace
```

Other options: `--binary PATH`, `--query TEXT`, `--selector CSS`, `--wait MS`
(Chrome wall-time dwell; native drains its existing lifecycle/virtual-timer pump).
`--steps` is a JSON array replacing default interactions. Supported steps are
`{move:{selector}}`, `{click:selector}`, `{focus:selector}`, `{type:text}`,
`{press:key}`, `{wait:ms}`. The Chrome input delay is 110ms; native uses its
existing human-input driver. These gestures are comparable intent, not identical
motion/clock samples. Consent actions can be supplied as explicit steps.

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
  harness hashes, per-process exit/timeout status. This records provenance; rebuild
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
- Chrome trace: context requests/headers/payloads, request/response correlation,
  timings, failure records, decoded document/script/XHR/fetch response files,
  page errors, frames and main-page websocket messages. The instrumented pass also
  enables CDP Network and retains ExtraInfo headers/cookie blocking reasons; when
  correlation is unambiguous those headers replace the automation header view,
  with the original view retained. Baseline automation headers can omit browser
  generated fields. A rapid navigation can
  invalidate response-body retrieval; the failed body is explicitly marked.
- `--debugger`: instrumented Chrome only; CDP `Debugger.scriptParsed` saves inline,
  eval and dynamic sources by SHA256 for the attached page. The debugger itself
  changes the environment; never use its verdict as the baseline control.
- `comparison.json`: endpoint counts, query/payload field names/hashes, header
  scope, observed interface counts, differing argument/result samples and stacks,
  pages/errors/features and shared-probe results. The raw files retain full payloads.
  Truncated native JSONL records are counted explicitly in `traceCoverage` rather
  than being treated as complete coverage.

Interface tracing wraps configurable getters/functions and selected setters
before page scripts, preserving native receivers. It records first stacks and
bounded primitive samples (160 characters, 12 variants, 3000 distinct access keys).
It reports skipped descriptors/dropped records. Instrumented Chrome additionally
uses CDP Network; raw redirect events remain available if header correlation is
ambiguous. Wrapping changes function identity
and source: this is an **intrusive diagnostic**, and our Chrome traced pass can be
rejected even when baseline passes. Snapshot/driver calls also appear in these
observations; use the recorded stack/source to distinguish harness queries.

Coverage is intentionally explicit: data-property reads, nonconfigurable fields,
worker execution and native child-realm accesses are not exhaustively
trapped. Chrome child-frame snapshots are separate. Fast destroyed contexts may
lose their final interface snapshot. Native serialized documents/network bodies
preserve the received scripts, but arbitrary generated eval/Function sources are
not inventoried by a native debugger. There is no TLS/HTTP2 packet capture here.
Do not infer complete browser parity, complete collector coverage or the server's
rejection reason from an absence of recorded calls.

## Existing detection tools

`--probes` runs the shared capability, BotGuard-integrity and WebGL collectors after
verdict snapshots where possible, plus a separate uninstrumented blank-page pair
so rejection/trace wrappers do not prevent or contaminate capability measurement.
These are our queries, distinct from observed site accesses. The capability
collector checks parsed hidden/textarea/select values and form ownership, independent intrinsic prototypes and Function globals as well as
CSS rule parsing; standalone blank iframe realms are real V8 contexts. Blank-page secure
context/UA-data and viewport can differ; retain that context in interpretation.

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

```bash
npm run test:detection
cd rust
cargo test -p turbo-surf-render --test detection_trace -p turbo-surf-core --test http_trace
```
