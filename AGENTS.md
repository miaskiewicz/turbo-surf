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

- **Engine: Rust** (`rust/` — a 9-crate workspace on the `turbo-dom` crate, edition
  2021). Source is `.mjs`-free except a thin JS launcher + the dev harness.
- **Crates:** `turbo-surf-core` (net/cookies/robots/url/frontier/crawl/cache),
  `turbo-surf-page` (navigator), `turbo-surf-view` (extract/visible/aria/locator/
  markdown/query/xpath/hydration/actions/…), `turbo-surf-render` (deno_core V8 +
  the vendored `browser_env` rtdom↔V8 DOM binding), `turbo-surf-raster` (synthetic
  screenshot tier — native layout+paint over `turbo-html2pdf-core`, + `measure_text`),
  `turbo-surf-transform` (swc), `turbo-surf-napi` (dev/harness in-process addon),
  `turbo-surf-py` (PyO3 binding → PyPI wheels), `turbo-surf-mcp` (the stdio MCP
  **binary** the launcher spawns).
- **JS surface:** `cli.js` + `index.js` (the launcher), `harness/` (benchmarks),
  `rust/playwright-shim/` (a drop-in `@playwright/test` façade over the
  `turbo-surf-napi` addon — Page/Locator/expect/context/fixtures, no browser;
  see its [`LIMITATIONS.md`](./rust/playwright-shim/LIMITATIONS.md)).
- **Vendored, never hand-edit:** `rust/crates/turbo-surf-render/src/browser_env{.rs,_upstream.rs,.js}`, `src/cssom_parser.rs`, its selector-lexeme probe/Chrome control, `src/html_documents.rs`, `src/document_basics.rs`, `src/document_basics.js`, `src/dom_implementation{.rs,.js}`, its tests/probe/control, `src/element_tokens.js`, `src/element_relations.js`,
  `src/element_relations.rs`, `src/html_text.rs`, `src/html_text.js`,
  `src/html_popover.js`, `src/html_popover.rs`, `src/collection_records.rs`, `src/css_policy.rs`, `src/event_handlers.rs`, `src/event_brands.rs`, `src/event_listeners.rs`, `src/event_listeners.js`,
  `src/dom_mutations.rs`, `src/dom_adoption_probe.js`, `src/dom_adoption_chrome155.json`, `src/dom_variadic_errors_probe.js`, `src/dom_variadic_errors_chrome155.json`, `src/dom_projection_tests.rs`, its element-interface probe/Chrome control, `src/range_contents{.rs,.js}`, its abstract/static Range interface and frame probes/Chrome controls, `src/range_probe.js`, `src/range_chrome155.json`, `src/css_adoption_tests.rs`, `src/css_adoption_probe.js`, `src/css_adoption_ownership_probe.js`, `src/css_adoption_chrome155.json`, `src/css_adoption_ownership_chrome155.json`, its stylesheet-lifecycle probe/Chrome control, `src/css_live_tests.rs`, `src/css_live_probe.js`, `src/css_live_chrome155.json`, `src/range_live.rs`, `src/range_live_probe.js`, `src/range_live_chrome155.json`, `src/document_xpath{.rs,.js}`, `src/xpath_provider/` and `src/browser_sanitizer_default.json`
  and `src/dom_ownership.rs`, `src/realm_ownership.rs`, `src/storage_ownership.rs`,
  `src/dom_isolation.rs`, `src/ownership_guard_tests.rs`, its computed-color probe/Chrome control,
  `src/html_meta_tests.rs`, its meta-reflection probe/Chrome control,
  `src/mutation_observers{.rs,.js}`, its tests/probe/Chrome control
  and `src/frame_tokens_tests.rs`, `src/frame_tokens_probe.js`, `src/frame_tokens_chrome155.json`
  and `src/script_state.rs`, its preparation probe/Chrome control,
  `src/shadow_slots{.rs,.js}`, its tests/probe/Chrome control, iframe
  credentialless tests/probe/Chrome control, and `src/utf16_contract_tests.rs`
  with its `src/utf16_*` probes/controls and `src/script_text*.js`/Chrome control
  and `src/template_contents{.rs,.js}` with its ownership/lifecycle probes/controls
  and `src/custom_elements{.rs,.js}` with its main, extended, reactions, IDL, realm
  move, callback and retired-definition probes/Chrome controls
  and `src/dom_parser_observer_lifecycle_{probe.js,chrome155.json}`
  and `src/trusted_types{.rs,.js}`, its native mapping, tests/probes/Chrome controls
  and `src/form_entrylist_{cases.json,probe.js,chrome155.json}`, `src/formdata_constructor_{cases.json,probe.js,chrome155.json}`, `src/form_controls.js` with its form-methods cases/probe/Chrome control,
  `src/element_internals{.rs,.js}` and its core/associated/state/definition/setlike controls,
  `src/computed_intrinsics_*`, `src/computed_button_ua_*`, `src/computed_border_priority_*`, `src/css_descriptor_*`, `src/checkedness_*`, `src/control_prototypes_*`, `src/form_reflection_*`
  and `src/formdata_{event,guards}_*` controls
  and `src/form_collections.js`, `src/form_checked_state.rs`,
  `src/{element_registry,submitter_reflection,form_collections,radio_dirty}_*`
  probes/cases/Chrome controls and `src/radio_{group_moves,owner_review}_*`
  probes/Chrome controls, `src/control_labels.rs` and its `src/control_labels_*`
  probes/Chrome controls
  — re-vendor from turbo-test's committed HEAD via
  `rust/crates/turbo-surf-render/scripts/vendor-browser-env.sh`.

## Embedded Deno extension sources

`rust/crates/turbo-surf-render/src/vendor/deno_extensions/` and the generated
`src/deno_extension_source_table.rs` preserve exact licensed published Deno JS.
Never hand-edit or format these files. Reproduce them with the render crate's
`scripts/vendor-deno-extension-sources.py --check` (or `--write` only for a
reviewed update), using the checksum-pinned `.crate` archives. The crate-local
`DENO-EXTENSIONS.md` documents source identity and the portability boundary.
This generator is independent of turbo-test's DOM-binding vendor script.

## The gates

The engine gate is **Rust** (`cd rust`): `cargo test --workspace`, `cargo clippy
--workspace --all-targets` (0 warnings), `cargo fmt --check`. The pre-commit hook
(`.githooks/pre-commit`, wired by `npm run prepare`) lints/formats staged JS
(oxlint + biome over the launcher/harness; the vendored `browser_env.js` is
skipped). `npm run check` runs the JS lint/format + the Rust gate + the Playwright
shim tests (`test:playwright` = `build:addon` then `test:shim` + `test:e2e`) and
the offline detection harness tests (`test:detection`). Never
bypass with `--no-verify`.

## Testing rules

- Rust: `cargo test` — `node:test`-style unit tests live in each crate; deno_core
  render tests are in `rust/crates/turbo-surf-render/tests/render.rs` (a separate
  binary, so they don't share a V8-platform init with the vendored binding's
  standalone-V8 unit test).
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
