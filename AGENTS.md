# AGENTS.md — working rules for turbo-surf

turbo-surf is a **browserless, native-speed crawler + MCP server for AI agents**,
a **Rust engine** built on the [turbo-dom](https://github.com/miaskiewicz/turbo-dom)
crate. It fetches + parses + acts on pages with **no Chromium**. For JS-gated pages
it runs the page's own scripts in a **true V8 isolate** (a `deno_core` runtime over
the native rtdom DOM), never a browser. The npm package is a **thin launcher**
(`cli.js`/`index.js`) that spawns the standalone Rust binary; Playwright/Chromium
appear **only as dev tooling** (benchmark oracle).

Read this before changing code. The current docs are [`README.md`](./README.md),
[`CHANGELOG.md`](./CHANGELOG.md), [`PUBLISHING.md`](./PUBLISHING.md), and the engine
docs [`rust/README.md`](./rust/README.md) + [`rust/HEADLESS-HYDRATION.md`](./rust/HEADLESS-HYDRATION.md).

## Stack & layout

- **Engine: Rust** (`rust/` — an 8-crate workspace on the `turbo-dom` crates, edition
  2021). Source is `.mjs`-free except a thin JS launcher + the dev harness.
- **Crates:** `turbo-surf-core` (net/cookies/robots/url/frontier/crawl/cache),
  `turbo-surf-page` (navigator), `turbo-surf-view` (extract/visible/aria/locator/
  markdown/query/xpath/hydration/actions/…), `turbo-surf-render` (deno_core V8 +
  the `turbo-dom-browser-env` rtdom↔V8 DOM binding; native live layout over
  `turbo-dom-layout`), `turbo-surf-transform` (swc), `turbo-surf-napi` (dev/harness
  in-process addon), `turbo-surf-py` (PyO3 binding → PyPI wheels), `turbo-surf-mcp` (the stdio MCP
  **binary** the launcher spawns). The synthetic screenshot tier (layout+paint,
  canvas raster, `measure_text`) is turbo-dom's `turbo-dom-raster` crate, used by
  mcp/napi/py; the V8 binding is turbo-dom's `turbo-dom-v8` fork (one V8 in the
  graph: `scripts/check-single-v8.sh`).
- **JS surface:** `cli.js` + `index.js` (the launcher), `harness/` (benchmarks),
  `rust/playwright-shim/` (a drop-in `@playwright/test` façade over the
  `turbo-surf-napi` addon — Page/Locator/expect/context/fixtures, no browser;
  see its [`LIMITATIONS.md`](./rust/playwright-shim/LIMITATIONS.md)).
- **DOM binding = the `turbo-dom-browser-env` crate (a dependency, never vendored):**
  the rtdom↔V8 browser binding (`window`/`document`/CSSOM/forms/events, its JS and
  its Chrome probes/controls) lives in
  [turbo-dom](https://github.com/miaskiewicz/turbo-dom) (`crates/turbo-dom-browser-env`);
  binding fixes land there, never as copies here. `turbo-surf-render` uses it with
  feature `host-internals` for surf's native services over its documents
  (`src/live_layout_dom.rs` layout capture, `src/compression_dictionary_dom.rs`
  link discovery); native live layout runs turbo-dom-layout's live-DOM input
  (`turbo_dom_layout::live`, no tree copy) and its `cssom_view` geometry modules, while
  surf keeps media/supports evaluation, scoped-sheet preparation, `zoom` and the
  neutral/resource admission policy; integration tests read probe/control data from
  `turbo_dom_browser_env::test_support` (dev feature `test-support`), and Node
  detection tests locate the resolved package via `cargo metadata`
  (`harness/detection/binding-files.mjs`). `native-sdk-frame-read` on
  render/mcp/napi/py forwards to the crate for the engine-154/155 native-SDK overlay.

## Embedded Deno extension sources

Nothing is vendored: `rust/crates/turbo-surf-render/src/deno_extension_sources.rs`
embeds the exact licensed published Deno JS from the `turbo-surf-deno-web` /
`-webidl` / `-crypto` distributions' `embedded_sources::SOURCES` tables (generated
by `rust/release-deps/deno/prepare.py` from checksum-pinned `.crate` archives), and
its tests re-check every source against the reviewed `SOURCE_PINS`. The crate-local
`DENO-EXTENSIONS.md` documents source identity and the portability boundary.

## The gates

The engine gate is **Rust** (`cd rust`): `cargo test --workspace`, `cargo clippy
--workspace --all-targets` (0 warnings), `cargo fmt --check`. The pre-commit hook
(`.githooks/pre-commit`, wired by `npm run prepare`) lints/formats staged JS
(oxlint + biome over the launcher/harness; source-hashed
Chrome probes are skipped). `npm run check` runs the JS lint/format + the Rust gate + the Playwright
shim tests (`test:playwright` = `build:addon` then `test:shim` + `test:e2e`) and
the offline detection harness tests (`test:detection`). Never
bypass with `--no-verify`.

## Testing rules

- Rust: `cargo test` — `node:test`-style unit tests live in each crate; deno_core
  render tests are in `rust/crates/turbo-surf-render/tests/render.rs` (a separate
  binary, so deno_core's V8-platform init stays out of the lib unit-test process;
  the binding's own standalone-V8 tests run in turbo-dom-browser-env).
- **Deterministic + offline**: tests parse fixtures / hit a localhost server, never
  the live network. New code must be covered.
- Live-network checks (the competitive + crawler harnesses) live in `harness/` and
  auto-skip when their deps/browsers are absent.
- Playwright shim: `rust/playwright-shim/{test,surface}.test.mjs` (offline unit) +
  `e2e/*.spec.mjs` run through `register.mjs` (drop-in, localhost server, no
  Chromium). They skip cleanly if the napi addon isn't built. New shim surface must
  be covered there; run via `npm run test:playwright`.

## Code standards

- **Match the surrounding code**: comment density, naming, idiom. Comments explain
  *why*, not *what*.
- **Decompose** for readability — table-dispatch over long `match`/`if` ladders,
  small named helpers over nested branches.
- **turbo-dom is consumed via its public `rtdom` surface only** (`Tree`, `NodeRef`,
  `serialize`, `cascade`, the `DocumentExt` seam). Do not reach into internals. If a
  capability is missing, solve it here or request a small additive turbo-dom change.
- **Hot-path discipline** (extract/visible): a single index loop over
  `query_selector_all` results, no per-element allocation beyond the result record.
- **Honest behavior**: surface what can't be done (e.g. JS-only handlers are inert
  on the no-JS path; pixel/render-only APIs throw a clear "no-browser engine"
  error) rather than silently no-op.
- **The render isolate keeps the host heap unreachable from guest page JS** (a true
  V8 isolate, with a runaway-execution budget).

## Commits

- Conventional-commit style subject (`feat:`/`fix:`/`refactor:`/`docs:`/`chore:`).
- Work on a branch off `main` unless told otherwise; commit/push only when asked.
- Every commit message ends with:
  `Co-Authored-By: Codex Opus 4.8 (1M context) <noreply@anthropic.com>`

## Releasing / publishing

**Pushing a `v*` tag IS the publish** — see [PUBLISHING.md](./PUBLISHING.md) for the
full checklist. In short: bump every version string to the SAME `X.Y.Z`
(`package.json`, `rust/Cargo.toml` workspace + each crate's path-dep `version`,
napi `package.json`, Python `pyproject.toml`, lockfile root versions, README status line;
compiled core/MCP/NAPI/Python versions derive from `CARGO_PKG_VERSION`),
green the gate (`cd rust && cargo test/clippy/fmt`), update `CHANGELOG.md`, commit
`chore(release): vX.Y.Z`, tag, push. On the tag, `release.yml` builds the
`turbo-surf-mcp` binary per platform and publishes the launcher npm package, and
`rust-crates-publish.yml` publishes the crates. Irreversible — only tag when asked
to ship/publish/release.

## Adding a capability (checklist)

1. Implement in the right crate (`rust/crates/turbo-surf-*`), functions small.
2. Expose over napi (`turbo-surf-napi`) if Node/harness needs it, and/or as an MCP
   tool (`turbo-surf-mcp` `tools()` + dispatch) if agent-facing.
3. Tests in the crate (offline; localhost server for net).
4. `cd rust && cargo test --workspace && cargo clippy --workspace --all-targets &&
   cargo fmt` green.

## Reference docs index

- **Engine:** [`rust/README.md`](./rust/README.md) (crates + tiers),
  [`rust/HEADLESS-HYDRATION.md`](./rust/HEADLESS-HYDRATION.md) (the JS render tier).
- **Product + status:** [`README.md`](./README.md), [`CHANGELOG.md`](./CHANGELOG.md),
  [`PUBLISHING.md`](./PUBLISHING.md).
- **Harness:** [`harness/competitive/README.md`](./harness/competitive/README.md),
  [`harness/crawlers/README.md`](./harness/crawlers/README.md).
- **Chrome parity setup:** [`harness/detection/CHROME-PARITY-RUNBOOK.md`](./harness/detection/CHROME-PARITY-RUNBOOK.md) (SDK, all-feature builds, opt-in flags, controls and evidence).
- **Playwright drop-in:** [`rust/playwright-shim/LIMITATIONS.md`](./rust/playwright-shim/LIMITATIONS.md)
  (per-method coverage map).
