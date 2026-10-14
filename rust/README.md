# turbo-surf — Rust port

Native-speed core of turbo-surf. Premise: turbo-dom ships as a pure Rust crate,
so the browserless crawler is Rust too. The only piece that *must* stay JS is the
`@playwright/test` drop-in façade (agents `import` it inside their own Node
process); it's a thin shim over the napi addon — all the muscle is Rust.

turbo-dom is consumed through the public `turbo-dom-parser` alias and its pure-Rust
`rtdom::Tree` (handle-based `u32` DOM, no napi/wasm boundary). The development
workspace now resolves the published `turbo-dom` 0.5.3 crate, including native mutation-event support,
fragment/document APIs and compact ordinary-node/attribute storage.
The 0.5.3 release tag is on PR #9 merge commit `8112bbf`; registry artifacts were verified.
The additive reclamation API is published in 0.5.4; consumer integration is in progress.

## Crates

| Crate | Scope |
|-------|-------|
| `turbo-surf-core` | Tier 1 — net / cookies / robots / url / frontier / crawl scheduling / cache / measure |
| `turbo-surf-page` | Tier 2 — `TurboNavigator` (fetch+parse over `rtdom::Tree`) |
| `turbo-surf-view` | extraction & views — extract / visible / aria / ax / locator / markdown / text / schema / query / xpath / hydration / dom-ops / actions |
| `turbo-surf-render` | Tier 3 — `deno_core` isolate + the rtdom↔V8 DOM binding (JS execution / hydration) |
| `turbo-surf-raster` | native layout/paint, canvas rasterization and system text measurement; optional Metal WebGL and CoreText |
| `turbo-surf-py` | PyO3 binding and PyPI wheels |
| `turbo-surf-transform` | swc TS/JSX → classic JS for the render tier |
| `turbo-surf-napi` | the `.node` addon — in-process bridge from the core to Node (+ stateful `Session`) |
| `turbo-surf-mcp` | stdio JSON-RPC MCP server — native binary, full 80-tool surface over a stateful session |

`cargo test --workspace` runs the offline engine suite. Required gates also include
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`;
the root npm test command checks versions, the addon/shim and detection harness too.

The render tier consumes published `turbo-html2pdf-core 0.3.5` for native live-tree
box geometry, transforms, element scroll metrics and point-hit regions. It reads
private actual CSSOM state without guest getters or HTML rewriting. Supported
contracts and remaining inline/caret/scroll/layout limits are recorded in
[LIVE-GEOMETRY.md](../harness/detection/LIVE-GEOMETRY.md).

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
- **The DOM is a native `rtdom`↔V8 binding** — `browser_env`, vendored from
  [turbo-test](../../turbo-test) (its battle-tested binding that runs React +
  Testing Library). A JS DOM node is a V8 object holding a turbo-dom handle in an
  internal field; methods/accessors are native callbacks straight onto `Tree`. No
  JS-DOM-in-JS-VM indirection. See [`src/browser_env.rs`](crates/turbo-surf-render/src/browser_env.rs)
  for the vendor/sync story: the one-command script copies committed canonical
  Rust/JS/CSS grammar and generates the surf adapter from
  `scripts/browser-env-adapter.rs.in`. Seeded installation delegates to the
  canonical API; surf additions remain outside the upstream binding.
- The runtime in [`src/runtime.rs`](crates/turbo-surf-render/src/runtime.rs) grafts
  that binding onto deno_core's context, then layers the non-DOM `window` env a real
  page needs (`navigator`, `location`, virtual timers + real microtasks, `fetch`/XHR
  **over the tier-1 net stack**, `URL`, `crypto`(+`subtle`), `MessageChannel`,
  `ReadableStream`, `AbortController`, `BroadcastChannel`, `WebSocket`,
  `document.cookie` bridged to the shared `CookieJar`, observers, history) + the
  hydration pump (executes injected `<script>` chunks, drains to quiescence).
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
`PageSession::open_with_init_script` installs opt-in diagnostics before page scripts;
`TURBO_SURF_HTTP_TRACE` records native application HTTP requests, responses and decoded
bodies. These are raw local diagnostics, not a TLS packet capture. Current Google
observations and unresolved gaps are in
[`../harness/detection/FINDINGS.md`](../harness/detection/FINDINGS.md).
