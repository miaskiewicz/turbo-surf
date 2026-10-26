# turbo-surf — Rust port

Native-speed core of turbo-surf. Premise: turbo-dom ships as a pure Rust crate,
so the browserless crawler is Rust too. The only piece that *must* stay JS is the
`@playwright/test` drop-in façade (agents `import` it inside their own Node
process); it's a thin shim over the napi addon — all the muscle is Rust.

turbo-dom is consumed through the public `turbo-dom-parser` alias and its pure-Rust
`rtdom::Tree` (handle-based `u32` DOM, no napi/wasm boundary). The workspace
requires `turbo-dom` 0.6 (the consolidation release, published with
turbo-dom-layout / -raster / -browser-env 0.6.0; until then the pre-publish
`[patch.crates-io]` table resolves them from a turbo-dom Git rev). Since 0.5.7
(tagged merge `5ec17065`) it carries [PR15](https://github.com/miaskiewicz/turbo-dom/pull/15)
and PR13/PR14 custom states, checkedness and detached contextual fragments.
The release also repairs scalar/original-UTF16
attribute case modifiers and measured HTML name/type defaults; the shared native
search selector now excludes hidden query fields correctly.
The release adds native custom states and live checked-state reads to the original
0.5.6 capabilities, including original UTF-16
payload/name/parser APIs, native mutation batches and bounded storage collection.
The earlier published0.5.6 annotated release tag identifies PR #12 merge commit
`ad52ba51bac82342d81ecb2fed98b8fa6b9dbf10`; npm platform/WASM metadata,
the successful release workflow and official registry checksums were verified.
The canonical binding consumes collection through the public API with genuine
weak wrapper roots and native job pins. Real V8 GC notifications schedule a
bounded scan after completed host operations; no Tree traversal runs inside the
GC callback. Limits are 250,000 nodes, 2,000,000 edges and 64 MiB retained payload
per Tree, with at most 256 Documents. Rejection leaves that Tree unchanged and
is retained in host diagnostics; earlier Trees in a multi-document scan may have
committed. Native DOM state is isolate-owned;
PageSessions enter their V8 isolate and native world for each future poll, then
restore the caller before yielding.

## Crates

| Crate | Scope |
|-------|-------|
| `turbo-surf-core` | Tier 1 — net / cookies / robots / url / frontier / crawl scheduling / cache / measure |
| `turbo-surf-page` | Tier 2 — `TurboNavigator` (fetch+parse over `rtdom::Tree`) |
| `turbo-surf-view` | extraction & views — extract / visible / aria / ax / locator / markdown / text / schema / query / xpath / hydration / dom-ops / actions |
| `turbo-surf-render` | Tier 3 — `deno_core` isolate + the rtdom↔V8 DOM binding (JS execution / hydration); native live layout over `turbo-dom-layout` |
| `turbo-surf-py` | PyO3 binding and PyPI wheels |
| `turbo-surf-transform` | swc TS/JSX → classic JS for the render tier |
| `turbo-surf-napi` | the `.node` addon — in-process bridge from the core to Node (+ stateful `Session`) |
| `turbo-surf-mcp` | stdio JSON-RPC MCP server — native binary, full 80-tool surface over a stateful session |

Native layout/paint, canvas rasterization and system text measurement (optional
Metal WebGL and CoreText) live in turbo-dom's `turbo-dom-raster` 0.6.0 crate, which
mcp/napi/py depend on (forwarding its `gpu-metal` / `coretext` features, and its
`avif` / `jxl` image decoders, which are on by default); the layout engine is
`turbo-dom-layout` 0.6.0. turbo-surf no longer depends on
turbo-html2pdf.

`cargo test --workspace` runs the offline engine suite. Required gates also include
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`;
the root npm test command checks versions, the addon/shim and detection harness too.

The render tier consumes `turbo-dom-layout` 0.6.0 (the layout half of
turbo-html2pdf-core 0.3.9, relocated verbatim into turbo-dom) for native live-tree
box geometry, transforms, element scroll metrics and point-hit regions: its live-DOM
input (`turbo_dom_layout::live`) styles the page's own composed tree in place from the
rules surf matched per tree scope, and its `cssom_view` modules (moved from this crate)
compute the CSSOM-View answers. Each read lays out the DOM as it is at that read:
a capture that a mutation outdated (the tree's version moved) is taken again rather
than laid out stale. Installed fonts come from turbo-dom-layout's process-wide
`FontRegistry::system(SystemFontPolicy::Html)`. It reads private actual CSSOM state
without guest getters or HTML rewriting. Supported
contracts and remaining inline/caret/scroll/layout limits are recorded in
[LIVE-GEOMETRY.md](../harness/detection/LIVE-GEOMETRY.md).

## Registry dependency identity

The seven direct Rust dependency aliases retain the existing source/API names.
Until the turbo-dom consolidation crates are published, `rust/Cargo.toml` carries a
pre-publish `[patch.crates-io]` table (Git revs of the Deno 0.x.1 candidates,
`turbo-dom-v8` and the turbo-dom 0.6.0 crates); it is removed at release. The
named distribution family is (turbo-surf publishes twelve; the V8 binding comes
from turbo-dom):

| Registry package | Exact version | Direct Surf alias or graph role |
| --- | --- | --- |
| `turbo-surf-wreq` | 0.16.1 | `wreq` |
| `turbo-surf-wreq-util` | 0.2.0 | `wreq-util` |
| `turbo-surf-btls-sys` | 0.5.6 | transport native dependency |
| `turbo-surf-btls` | 0.5.6 | transport dependency |
| `turbo-surf-tokio-btls` | 0.5.6 | transport runtime dependency |
| `turbo-dom-v8` | 150.4.0 | the native V8 binding (turbo-dom's rusty_v8 fork; the only V8) |
| `turbo-surf-deno-v8` | 0.4.1 | `v8` (the shared facade) |
| `turbo-surf-serde-v8` | 0.321.1 | shared value conversion |
| `turbo-surf-deno-core` | 0.412.1 | `deno_core` |
| `turbo-surf-deno-bench-util` | 0.253.1 | upstream test/benchmark identity |
| `turbo-surf-deno-crypto` | 0.273.1 | `deno_crypto` |
| `turbo-surf-deno-webidl` | 0.259.1 | `deno_webidl` |
| `turbo-surf-deno-web` | 0.290.1 | `deno_web` |

The distribution generators retain upstream versions, native archive URLs,
licenses and reviewed source pins while mapping every internal dependency to the
same named family. Transport uses wreq `4ed3cf0c`, wreq-util `5715529`,
btls `f5a8a48` and BoringSSL `f1f2556`; the V8/crypto sources retain native weak-slot
teardown and private-key construction/clone hardening. Reproduction, source pins
and actual-archive/registry consumer procedures are in
[release-deps](release-deps/README.md) and
[DENO_CRYPTO.md](release-deps/deno/DENO_CRYPTO.md); the V8 fork's patches, pins and
generator live in turbo-dom (`crates/turbo-dom-v8`).

Stock releases still embed V8 150, not Chrome 155. The experimental native155 SDK
is an explicit, separately built and validated profile; its package aliases do
not change the engine version by themselves. Source, distribution, registry and
native-profile validation remain separate evidence scopes.

## Tier 1 — `turbo-surf-core`

Direct ports of the JS modules, same behavior and edge cases:

- `url` — `resolve` / `canonicalize` (tracking-param strip, query sort, frag drop) /
  `is_http_url`. Frontier dedupe basis.
- `frontier` — canonical-dedup URL queue with depth + ring cursor.
- `robots` — robots.txt parse, per-agent grouping, longest-match Allow/Disallow with
  `*`/`$` wildcards (hand-rolled glob, no regex dep), TTL cache; injected
  `RobotsFetcher` → offline-testable.
- `cookies` — RFC 6265 subset `CookieJar` (domain/path scope, Secure, HttpOnly,
  Expires/Max-Age, SameSite; `storageState` round-trip). Times are `f64` ms (session
  cookie = `f64::INFINITY`). Script-visible cookie access excludes HttpOnly cookies
  and rejects script overwrites of them. Self-contained HTTP-date parser (no chrono).
- `net` — `fetch_html` over wreq/BoringSSL by default (`impersonate`), or reqwest/rustls
  with that feature disabled. Charset sniff, byte
  cap, content-type gate, CookieJar round-trip, manual per-hop redirect follow, and a
  **shared pooled `build_client()`** passed via `FetchOptions::client` so connections
  + TLS sessions are reused across pages. `request_context` derives destination/mode,
  referrer/Origin and credentials policy, with PSL site classification and session-scoped
  Accept-CH opt-ins. `FetchOptions::client_hints` shares this state with page execution.
- `http_cache` / `compression_dictionary` — session-owned HTTP representations and
  partitioned, origin-scoped compression dictionaries; genuine selected hash/ID
  negotiation and bounded incremental Brotli/Zstandard decoding. The renderer owns
  policy-aware background Link
  discovery; see [controls and bounds](../harness/detection/COMPRESSION-DICTIONARIES.md).
- `crawl` — frontier-driven scheduling: global + per-host concurrency, per-host
  politeness, retry/backoff, depth/page caps, robots gate. Fetch+parse seam is the
  `Navigator` trait (tier-2 `Page` implements it).
- `cache` / `measure` — `ResponseCache` (304/storageState) and crawl summaries.

## Tier 2 — `turbo-surf-page`

`TurboNavigator` implements `crawl::Navigator` — fetches via `net::fetch_html`,
parses with `Tree::parse`, projects a `Nav` (title + absolute-resolved `<a href>`s).
The tier-1 `crawl::crawl(opts, nav)` driver runs unchanged over it. `parse_nav` is
pure (no network) and offline-tested end to end against the scheduler.

## views — `turbo-surf-view`

The extraction/interaction surface over the same `rtdom::Tree`: `extract`, `visible`
(cascade), `aria`/`ax`/`aria_snapshot`, `locator` (by_role/text/label), `markdown`,
`text`, `schema`, `query`, `xpath`, `hydration`, `dom_ops` (checked/editable/css/
select), `actions` (fill/submit/click-intent). All pure + offline-tested; a
differential `tests/parity.rs` checks them against a committed JS golden.

## Tier 3 — `turbo-surf-render`

The JS-execution path, end to end over a **real DOM**. The page's own scripts run on
a `deno_core` V8 isolate against a genuine `document`, mutate the turbo-dom tree in
place, and the render returns the hydrated HTML (the Lane B contract).

- Boots a **`deno_core` V8 isolate** (true isolate — host heap unreachable from
  guest; a runaway-execution **budget** watchdog terminates a wedged script).
- **The DOM is a native `rtdom`↔V8 binding** — `browser_env` from the
  [`turbo-dom-browser-env`](https://github.com/miaskiewicz/turbo-dom) crate (the
  battle-tested binding turbo-test also runs React + Testing Library on; it moved
  there from turbo-test). A JS DOM node is a V8 object holding a turbo-dom handle in
  an internal field; methods/accessors are native callbacks straight onto `Tree`. No
  JS-DOM-in-JS-VM indirection. turbo-surf-render depends on it (feature
  `host-internals`) and keeps no copy: page loads use its `install_html` /
  `install_independent_*` / `document_html`, and surf's native services over its
  documents (`src/live_layout_dom.rs`, `src/compression_dictionary_dom.rs`) use its
  host-only `host_internals`. Binding changes land in turbo-dom.
- The runtime in [`src/runtime.rs`](crates/turbo-surf-render/src/runtime.rs) grafts
  that binding onto deno_core's context, then layers the non-DOM `window` env a real
  page needs (`navigator`, `location`, virtual timers + real microtasks, `fetch`/XHR
  **over the tier-1 net stack**, `URL`, `crypto`(+`subtle`), `MessageChannel`,
  `ReadableStream`, `AbortController`, `BroadcastChannel`, `WebSocket`,
  `document.cookie` bridged to the shared `CookieJar`, observers, history) + the
  hydration pump (executes injected `<script>` chunks, drains to quiescence).
  Host-owned hydration diagnostics retain the last lifecycle phase and completed
  post-load pump count, including on watchdog interruption. Bounded pump completion
  is not a global-idle guarantee. The detection harness can opt into private native
  Inspector CPU profiles without changing the execution budget.
- **`fetch` is real** (async, event-loop-driven; relative URLs resolve against the
  page base) — promise/`await`/timer-driven hydration settles before serialization.
- `run_with_dom` (the sync evaluation path) retains its isolate and native DOM for
  repeated calls with identical HTML. Changing the HTML creates a fresh isolate,
  clearing lexical declarations, globals and modified intrinsics. Fresh page hydration
  also uses independent runtimes. `render_page_pooled` keeps its public compatibility
  name but currently opens a fresh runtime on every call; the former cross-page boot
  optimization was removed to preserve isolation.
- `transform` (`turbo-surf-transform`, swc) turns a TS/JSX bundle into classic JS
  so it runs under the tier.

## glue — `turbo-surf-napi` + the Playwright shim

`turbo-surf-napi` is the `.node` addon: a stateless functional surface (markdown /
text / links / extract / getBy / accessors / actions / evaluate / render / transform)
plus async `fetchHtml` / `request` / `crawl` and a stateful `Session` (retained tree,
worker thread), plus `nodeSnapshot` (a one-crossing batch of a node's state reads).
The `rust/playwright-shim/` package is the `@playwright/test` drop-in backed by it
(Page / Locator / expect's five assertion classes / BrowserContext / fixtures /
`chromium`, plus a `--import register` resolve redirect so vanilla specs run on the
no-browser Rust engine, unedited). Coverage map:
[`playwright-shim/LIMITATIONS.md`](./playwright-shim/LIMITATIONS.md).

## Build / test

```
cd rust
cargo test --workspace        # full offline suite
cargo clippy --workspace --all-targets
cargo fmt
cargo build --release -p turbo-surf-napi   # build the .node addon (then node addon tests / harness)
```

## Detection diagnostics

See [`../harness/detection/README.md`](../harness/detection/README.md) for a fresh-jar
Chrome/native comparison with all native fingerprint/raster features enabled.
The [Chrome parity runbook](../harness/detection/CHROME-PARITY-RUNBOOK.md) lists
the SDK setup, every required opt-in, executable/addon selection and validation order.
`PageSession::open_with_init_script` installs opt-in diagnostics before page scripts;
`TURBO_SURF_HTTP_TRACE` records native application HTTP requests, responses and decoded
bodies. These are raw local diagnostics, not a TLS packet capture. Current Google
observations and unresolved gaps are in
[`../harness/detection/FINDINGS.md`](../harness/detection/FINDINGS.md).
