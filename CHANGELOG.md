# Changelog

## [Unreleased]

### Fixed (turbo-dom consolidation P5b)
- Live render images match Chrome 156 (#32): `complete`, `naturalWidth`,
  `naturalHeight` and `currentSrc` are `HTMLImageElement.prototype` accessors
  (turbo-dom-browser-env), fed by surf's loader through the document's private
  `imageHost` record; `src` and `decode()` are prototype members too, so an `<img>`
  has no own properties (`__tcImagePatched`, own `src`/`decode`/state getters and
  the own `document.createElement` override are gone). `decode()` rejects a bad
  receiver with Blink's TypeError and an undecodable image with an `EncodingError`
  `DOMException`. `render(html, base, script)` now starts the parser's image and
  stylesheet loads before its script, so parser images load and fire `load`/`error`
  (they never loaded there).
- `data:` images decode synchronously (#38): like Chrome, an image whose source is a
  `data:` URL is complete with its natural size the moment the source is set (parser
  images at the first script read, `new Image().src`, fragment-parsed and connected
  images); its `load`/`error` event still fires as a later task, and a broken script-set
  one stays pending until that `error`. Chrome 156 controls
  `html_image_render_chrome156.json` (now exact) and `html_image_sync_chrome156.json`.
- `HTMLImageElement` is entirely the binding's (turbo-dom#23): surf's plain-function
  `globalThis.Image`, its `src` setter and `decode()` overrides are gone, so `Image` is
  the native legacy factory (`new`-only, `length` 0, NewTarget-aware) and `decode()`
  settles with the image's load state. Loads start from the document's mutation
  consumer, so a detached image loads like Chrome — `new Image().src = …`,
  `setAttribute("src")` on a created image, detached `innerHTML` /
  `insertAdjacentHTML` / `createContextualFragment` images (they never loaded) — while
  images of inert documents (template contents, `DOMParser`, `createHTMLDocument`)
  no longer load, and removing `src` resets the natural size to 0x0. Chrome 156
  control: `tests/fixtures/html_image_loads_chrome156.json`; the binding's
  `HTML_IMAGE_*` (factory/own/decode) and `CUSTOM_ELEMENTS_DEFINED_*` controls also run
  through `PageSession`. Known gaps (pinned): no `srcset` candidate selection; the
  binding's `decode()` EncodingError is not `instanceof` surf's `DOMException`; parsed
  `is=` elements read `:defined` once a session has opened.
- Live `<img>` loads decode like Chrome 156 (#30): success and `naturalWidth` /
  `naturalHeight` come from turbo-dom-layout's shared probe (`probe_natural` and its
  raster probe) instead of a full `image` crate decode, and every `data:` form goes
  through `decode_data_url` (percent-encoded and raw `image/svg+xml`, whitespace /
  unpadded / percent-escaped base64; it was `;base64` only). Newly loading: BMP, ICO/CUR
  (the largest entry), AVIF, SVG served as `image/svg+xml` (Chrome's natural sizes, e.g.
  300x150 for a sizeless root), truncated or corrupt-IDAT bodies with a readable header,
  and image bodies of HTTP error responses; SVG under any other type and raster bytes
  labelled `image/svg+xml` now fail. `turbo_surf_core::net::fetch_typed_bytes` also
  returns the response's Content-Type. Chrome 156 control:
  `tests/fixtures/image_formats_chrome156.json` (49 rows differed before; 10 pinned
  probe gaps remain: header-only JPEG/GIF/WebP and a namespace-less SVG root load). The
  render crate no longer depends on the `image` crate.
- Screenshots paint AVIF (#30): the MCP binary, the napi addon and the Python wheel
  forward a new default-on `avif` feature to turbo-dom-raster (rav1d, pure Rust,
  BSD-2-Clause — notice added to `THIRD_PARTY_NOTICES.md`; +1.4 MB napi, ~4 s release
  compile). Live loads already size AVIF and image requests advertise `image/avif` as
  Chrome does, so CDNs serve it; without the feature it painted as a placeholder. BMP and
  ICO/CUR paint through the repinned raster. napi `fetchBytes` sends Chrome's image
  `Accept` again when built with `avif`. Chrome 156 control:
  `turbo-surf-mcp/tests/fixtures/screenshot_image_formats_chrome156.json`.
- `img.width`/`height`/`x`/`y` read surf's live layout (turbo-dom#26): surf answers the
  binding's `imageHost.setLayoutQuery` from a new native op (content box pixel-snapped
  and zoom-adjusted, border-box document position; null without a box), and live layout
  sizes a loaded `<img>` from its natural size (#39: it laid out every image without one,
  e.g. a `display:block` image as 1280x0). The binding's whole `HTML_IMAGE_*` control,
  including its `dimensions` section, holds through `PageSession`.
- Acceptance of the turbo-dom 5157913 fixes through `PageSession`: live geometry no
  longer throws for table rows/row groups or positioned inlines (#31,
  `live_geometry_throws.rs` has no known gaps), and getComputedStyle reports the UA
  `display`/`position`/`visibility` with no injected `<title>` (#36 items 1-2,
  `computed_defaults.rs`).
- `srcset` is honoured (#40): an `<img>` loads the candidate Chrome 156 picks — density
  and `w` descriptors (`sizes`, default 100vw), `src` as the 1x candidate, the first
  candidate whose density reaches `devicePixelRatio`, else the densest — and reports its
  natural size divided by that density; a `sizes` change reselects. `<picture>` is not
  selected yet. Chrome 156 control `html_image_srcset_chrome156.json` (uncached HTTP
  candidates at devicePixelRatio 2).
- The font-run geometry Chrome collector reads its fonts only from the pinned
  turbo-html2pdf commit's blobs, so it runs against turbo-html2pdf ≥ 0.6.0 (#33); its
  three controls are recaptured on Chrome for Testing 156 (`*-chrome156.json`,
  identical measurements).
- Flaky tests made deterministic: the smoke-release interrupt control waits for the
  fake executable instead of a fixed 5 s (#34); the credentialless partition control
  no longer times the host with a 5 s budget (#35); the reentrant CSP event test awaits
  both events instead of a 50 ms timer (#37, plus a slow-host variant). Audit of the
  remaining 5 s session budgets: the 40 that bound real localhost work, and the host-side
  caps on server effects, are generous caps now (60 s / 30 s), and fixed waits for
  real-time deliveries (storage events, BroadcastChannel, CSP task capacity) are
  condition waits.
- turbo-dom crates repinned to the P5b binding fixes (turbo-dom#20 `Option()` and
  HTMLOptionsCollection accessor definitions, #32 image accessors); the interceptor
  runtime control has no known gaps left.


## [0.6.0] — 2026-10-26 — turbo-dom consolidation

turbo-surf now builds on the published turbo-dom 0.6.0 crate family instead of
vendored or sibling-repo copies. For users of the npm launcher, the MCP binary,
the PyPI wheel and the five Rust crates nothing is renamed and no tool or API
changes; the visible differences are the Chrome-correct selector fixes, `t:*` in
static screenshots and the live-geometry re-capture listed below. Dependency map:

| was | now |
|---|---|
| vendored turbo-test binding (`scripts/vendor-browser-env.sh`) | `turbo-dom-browser-env` 0.6.0 |
| turbo-html2pdf-core 0.3.9 (layout) | `turbo-dom-layout` 0.6.0 |
| `turbo-surf-raster` (this workspace) | `turbo-dom-raster` 0.6.0 (not published from here any more) |
| `turbo-surf-v8` / vendored rusty_v8 | `turbo-dom-v8` 150.4.0 (one V8 in the graph) |
| vendored Deno extension JS | the Deno family 0.x.1 `embedded_sources` |
| turbo-dom 0.5.8 | turbo-dom 0.6.0 |

Rust consumers: require `turbo-dom = "0.6"`; `turbo-dom-raster` replaces
`turbo-surf-raster` for screenshots. The workspace publishes core → view → page →
render → mcp.

### Changed (0.6.0 consolidation, final step)
- Live geometry reads re-capture a stale capture: when the DOM changed between a
  read's capture and its layout, surf now captures again and lays out the current
  tree (as Chrome's layout reflects the current DOM) instead of failing with
  "native layout tree changed after its capture", and a stale capture is no longer
  answered from the layout cache of its old version. A tree that keeps changing
  under its own capture, or a node that left its document, still reports an error.
  No production path mutated there, so no corpus or test output changes.
- Live layout's installed-font registry is turbo-dom-layout's process-wide
  `FontRegistry::system(SystemFontPolicy::Html)` (one load per process, shared with
  any other embedder); the faces and their order are unchanged.
- turbo-dom crates pinned to the turbo-dom 0.6.0 consolidation head (S5a: one image
  layer, legacy owned-node layout input removed, workspace lints); no surf API
  change.
- Release: `rust-crates-publish.yml` gains a tag-only `release-guard` job
  (`rust/release-deps/release_guard.py`): no `[patch]` table, no Git/path sources,
  and the turbo-dom crates + Deno family locked from crates.io at published
  versions. PUBLISHING.md lists the exact pre-tag steps.

### Changed
- The DOM binding is no longer vendored: `turbo-surf-render` depends on the
  `turbo-dom-browser-env` 0.6.0 crate (the same binding, moved from turbo-test
  `cb5c64e` into turbo-dom) with feature `host-internals`. Deleted
  `scripts/vendor-browser-env.sh`, `scripts/browser-env-adapter.rs.in`, the generated
  adapter and the 398 files it copied; surf's native layout capture
  (`live_layout_dom.rs`) and compression-dictionary discovery
  (`compression_dictionary_dom.rs`) are ordinary modules over the crate's
  `host_internals`. Integration tests read probe/control data from
  `turbo_dom_browser_env::test_support`; Node detection tests locate the package via
  `cargo metadata`. New forwarding feature `native-sdk-frame-read` on
  render/mcp/napi/py (enabled by the engine-154/155 native-SDK overlay).
- turbo-dom 0.6.0 (turbo-dom#18 S3a: Chrome-correct selector
  escapes, non-ASCII idents, `*|p`, `:empty`, `:read-only`/`:read-write`,
  `:nth-child(An+B of S)`, `:link`/`:any-link`; S2/S3b: turbo-dom-layout styles a
  turbo-dom `Tree` and matches with turbo-dom's selector engine). The static
  screenshot tier treats `t:*` elements as unknown inline elements, as Chrome does
  (no surf fixture contains one).
- Native live layout no longer copies the DOM: the live-tree copier
  (`live_layout.rs`), the composed styler (`style_composed_tree`) and the separate
  `collect_styles` walk are gone. turbo-dom-layout's live-DOM input
  (`turbo_dom_layout::live`) captures the composed tree's provenance under the same
  budgets and errors (pinned by `live_layout_budget_tests.rs`) and styles the live
  tree from surf's pre-matched scoped rules; zoom scaling and style collection share
  one post-cascade walk. The pure CSSOM-View modules (`live_layout_{metrics,hit,scroll,
  visibility,inline,text,range}.rs` and the composition graph) moved to
  turbo-dom-layout's `cssom_view`. No output change.
- `@supports selector(…)` and `CSS.supports("selector(…)")` use turbo-dom's selector
  engine (one unforgiving complex selector), matching Chrome 156: `*|p`, `|p`,
  `[*|title]`, escapes, `:hover`, `::before`, `:link`, `:empty`, `:host`, `::slotted()`
  and `:nth-child(… of S)` are now supported, `ns|p` and `:is(:bogus)` are not
  (control `harness/detection/fixtures/css-supports-chrome156.json`, which replaces the
  Chrome 155 one).
- The interceptor ShouldThrowOnError fix is active: strict `[[Set]]`/`[[Delete]]` and
  `Object.defineProperty` on interceptor-backed collections (`document.all`,
  `<select>`, `select.options`) throw Chrome's TypeError, sloppy writes and the
  `Reflect` forms stay quiet (`tests/interceptor_mode_runtime.rs` against a Chrome 156
  control; raw V8 and accessor-vs-frame-read checks in
  `tests/interceptor_should_throw.rs` / `tests/interceptor_mode_agreement.rs`).
- The DOM binding is turbo-test `cb5c64e`'s (v0.4.8; 0.5.5 vendored `602b267`),
  first re-vendored with the old copy script, whose file list gained the
  selection, editing and EditContext modules, then moved into
  `turbo-dom-browser-env` unchanged. This brings the genuine Selection API
  (`Selection`, text-control selection, `createValueRange`/OpaqueRange), the editing
  engine (`execCommand`/`queryCommand*`, contenteditable/designMode, `EditContext`,
  `ariaNotify`) and the `scroll-behavior`/`scroll-snap-*` CSSOM keyword properties.
  The 0.5.5 entry listed editing and Selection, but the binding vendored in 0.5.5
  (`602b267`) did not contain them yet.
- `document.getSelection` is now the binding's `Document.prototype` member over
  per-document state, no longer a creation-context delegate: a retired (navigated)
  or parsed document returns `null`, as in Chrome, instead of the old document's
  selection object.
- Host typing replaces a text control's selection through the binding's trusted
  InsertText path: live value ranges follow the edit and no `select` event fires
  per typed character.

### Consolidation onto the turbo-dom crates (layout / raster / Deno)
- Layout: `turbo-surf-render` depends on `turbo-dom-layout` 0.6.0 (the layout half
  of turbo-html2pdf-core 0.3.9, relocated verbatim into turbo-dom; default features
  only) instead of turbo-html2pdf-core. turbo-surf no longer depends on
  turbo-html2pdf.
- Raster: the `turbo-surf-raster` crate moved to turbo-dom as `turbo-dom-raster`
  0.6.0 (verbatim, incl. WebGL; its tests run there). mcp/napi/py depend on it and
  forward `gpu-metal` / `coretext`; the workspace has eight crates and
  `rust-crates-publish.yml` publishes five.
- Deno: the render crate uses the 0.x.1 family (deno_core 0.412.1, deno_crypto
  0.273.1, deno_web 0.290.1, deno_webidl 0.259.1, facade 0.4.1) on turbo-dom's
  `turbo-dom-v8` 150.4.0 fork; `turbo-surf-v8` and `v8x` left the graph and
  `scripts/check-single-v8.sh` (in `npm test`) requires exactly one V8.
- No vendored sources: the Deno extension JS comes from the distributions'
  `embedded_sources` tables (the vendored `src/vendor/deno_extensions` copy, its
  generated table and `vendor-deno-extension-sources.py` are gone; the reviewed
  SHA-256/length pins moved into the render tests). `rust/vendor/` is removed: the
  rusty_v8 fork lives in turbo-dom, and deno_crypto is generated by
  `rust/release-deps/deno/prepare.py` from the pinned upstream archive plus
  `patches/deno_crypto.patch`.
- Pre-publish only: `rust/Cargo.toml` `[patch.crates-io]` resolves the Deno
  candidates and the turbo-dom crates from Git; it is removed before the v0.6.0 tag
  (PUBLISHING.md) and the release guard refuses a tag that still has it.

## [0.5.5] — 2026-10-25 — Chrome-156 parity batch + re-version

Lands the Chrome-156 parity batch tracked in [issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28):
re-vendors the turbo-test DOM binding with the closed missing-member work and
adds turbo-surf-side geometry/runtime support. Every closed surface has an
independently captured Chrome 156.0.8078.47 control and an offline regression;
no presence-only stubs. Depends on the released turbo-dom 0.5.8 and
turbo-html2pdf-core 0.3.9.

### Added / changed
- Re-vendored binding: meter/progress/output, select collection API, input/
  textarea reflection, invoker commands + CommandEvent, getHTML/setHTMLUnsafe and
  ShadowRoot state, declarative shadow DOM in parsing + Sanitizer API, document.all
  and legacy Document members, pointer capture/lock, Web Animations, CSS Typed OM,
  View Transitions, editing (execCommand/EditContext), Selection + Range.expand,
  reference targets / interest / focusgroup, FileList/DataTransfer, HTMLOptionsCollection/
  History/Location/VisualViewport/ScreenOrientation, strict CSSOM selector admission,
  interface key order, and the privacy-sandbox member stubs that reproduce Chrome's
  fresh-profile rejections.
- turbo-surf geometry/runtime: real scroll + smooth scrolling + scroll-snap,
  checkVisibility, CSS zoom / currentCSSZoom, root/viewport client sizes, observers
  (Intersection/Resize/Performance), the view-transition/animation frame loop,
  document response hooks (lastModified/domain/agent-cluster) and fullscreen/PiP
  permissions-policy accessors.
- Emulated identity re-versioned to Chrome 156.0.8078.47, with a reusable
  measured identity battery and a data-driven re-version helper
  (harness/detection/CHROME-REVERSION.md).

### Known gaps / deferred
- JS engine stays stock V8 15.0 vs Chrome 156's 15.6 (native 15.6 is an opt-in
  from-source profile; see harness/detection/engine/README.md).
- The cross-repo dependency consolidation (moving the DOM binding + render engine +
  V8 fork into turbo-dom) is deferred to follow-up PRs.
- PARITY-GAPS.md records the remaining open categories; issue #28 stays open.


## [0.5.4] — 2026-10-21 — browser parity and reusable detection differential

This release collects the work in PR26. The entries describe the implemented
subset, not complete Chrome conformance or successful native Google clearance.
[PARITY-GAPS.md](harness/detection/PARITY-GAPS.md) is the central remaining-work
checklist; [issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28) tracks
residual work beyond this PR, with audio separately deferred in
[issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27).

### Startup and scheduling follow-up — 2026-10-21

- Consume the public font-sharing producer fix from turbo-html2pdf PR25.
  It preserves collection-face indexes, shaping caches and PDF subset identity;
  863 workspace tests,15 xref controls and strict checks pass. One matched
  optimized font probe measures registry initialization1,653→165ms and peak
  RSS3.83→1.40GB, including later fingerprinting. All983 faces and930 selection,
  metric and glyph results match. OS disk-cache state is unspecified.
- Officially re-vendor the tested per-read ancestor snapshot and selector-first
  fixes from turbo-test PR27. Old code fails the ancestor control with100 scans
  against its28 bound and performs3,528 unmatched-declaration reads where the
  fix performs zero. Complete private selector-list matching also preserves
  quoted braces/commas, verified against genuine Chrome. Canonical checks pass
  33 CSS,304 library,63 Node and52 compatibility cases; one explicitly ignored
  selector-admission gap retains its Chrome expectation and fails when run
  explicitly. Caches expire before the next public read. That preexisting gap
  and default-inherited color/CSSOM synchronization remain documented in issue28.
- Add a genuine frozen-Chrome timer control, reusable collector/probe and native
  regressions. Chrome155 carries same-Window nesting through Promise/microtask
  callbacks and clamps after six immediate tasks; alternating Window targets
  reset nesting. Native task checkpoints, cancellation conversion and scoped
  nesting cleanup pass21 native integration tests plus an isolated native
  regression covering seven nesting variants, frame retirement, reentrant
  legacy drains and watchdog recovery. The combined current native gate also
  passes; broader interval cadence and task-source timing remain in issue28.
- Add a reusable private CPU-profile summary with source locations and sampled
  self-time aggregation. It sorts out-of-order Inspector timestamps together
  with their samples; the real profile contains one negative1us delta. Node20
  and22 each pass four focused summary/source-pin controls. This analysis is not
  a publication sanitizer or a count of function invocations.
- Reproduce homepage hydration exhaustion without interface wrappers using the
  preserved optimized all-feature executable and unchanged20-second budget.
  CPU profiling identifies repeated private CSS record/ancestor-style work;
  native layout time is measured separately. The homepage-only diagnostic has
  no consent or search actions and does not establish native SERP clearance.
  The first rebuilt optimized all-feature diagnostic completes HTTP200 hydration
  with one pump and no budget exhaustion:18.52s whole run,1.24GB peak RSS including
  profiling and snapshots. The selector-first follow-up also completes hydration:
  16.35s whole run,957MB peak RSS and14.70s guest profiling. These live homepage
  diagnostics do not establish SERP clearance, reliability or complete timing
  parity. At9304df9 the full gates pass1,509 native and1,464 stock Rust tests, strict
  Clippy, formatting, verified addons, shim96+1 intentional skip and E2E20 each.
  Detection passes204 native controls and195 stock controls+9 documented skips;
  both Python checks pass in each gate. The fresh75-interface inventory remains
  142 missing own members and374 difference paths.
- Run a fresh14-process Google comparison on the tested9304df9 source and
  optimized artifact, with independent cookies and all native bridges. Both
  native baselines complete initial hydration with one pump inside the unchanged
  budget, save consent204 and submit real forms. Search200 shells then redirect
  to429 with zero results: native acceptance remains0/2. Both interface-instrumented
  native captures still exhaust their initial hydration budget. Four Chrome
  flow captures fail after ineffective consent clicks, with no save request and
  an obscured query input. Two standalone Chrome probes fail because the blank
  fixture inherits the search readiness selector. This is an incomplete SERP
  comparison; the probe configuration is repaired separately without relabeling
  these failed captures. No rejection or ineffective-click cause is established.
  Raw traces remain private.
- Validate capture supervision and evidence publication with10 supervisor/manifest
  controls,21 historical privacy controls,25 current privacy/provenance controls,
  ten real-proof tamper rejections and eight matching historical diagnostic
  controls. Bind every publication to its actual capture attempt and build.
- Clear inherited page readiness only for the standalone blank probe fixture.
  Two actual-runner regressions pass on both Node20 and22; the final Node22
  detection suite passes206 tests without skips, plus Python2. A genuine local
  seven-process matrix completes all four probes in Chrome and native. Search
  readiness and all timing settings are preserved. The earlier Google probe
  failures remain failed evidence; native engine inputs are unchanged.
- Disable only dev/test debug symbols in CI to reduce accumulated build storage.
  The previous9304df9 Node22 job passes; Node20 exhausts runner disk during addon
  linking. Test selection, assertions, overflow checks and release settings stay
  unchanged. The repaired workflow passes on Node20 and22 at final tested
  head46f4340 ([CI run](https://github.com/miaskiewicz/turbo-surf/actions/runs/39533940495)).
- Remove consumed producer debug/release caches after source, ownership,
  ignored/untracked, retained-proof and closed-consumer checks. Actual free disk
  space increases6.53GB; independent font probes and all source/SDK evidence remain.
  After the successful combined gates, remove only their closed native/stock
  debug caches (33.47GB and30.65GB); retain verified release artifacts and proofs.

- Preserve all 28 embedded Deno modules, their three licenses and source pins as
  exact Git bytes on Windows as well as Unix. A real autocrlf checkout regression
  reproduces the former CRLF drift and verifies the complete 32-file inventory;
  the existing source-hash adapters remain unchanged and fail closed. Fresh
  Windows release artifacts remain an independent installed-runtime gate.

- Preserve the exact full MIT and third-party notice bytes in all 14 shipping
  source copies. The former Windows wheel contained complete text with CRLF
  drift; literal Git attributes and real LF/CRLF checkout controls keep payloads
  byte-identical without relaxing installed-artifact or publication audits.
  Include the workspace `rust/LICENSE` copy: controlled Windows-style checkouts
  now run the actual notices checker and reproduce its missing-rule failure.

- Keep the authored Python package module, type stub and installed smoke-test
  source at canonical LF on every platform. Actual Windows wheel bytes exposed
  CRLF drift; three literal checkout rules and LF/CRLF Git controls preserve the
  existing exact source and receipt hashes without normalizing artifact audits.

### Release readiness — 2026-10-21

- Embed the exact 28 licensed Deno Web, WebIDL and Crypto extension JavaScript
  sources in the render crate before its existing hash-checked private-array
  adapters. Deno 0.412 extension declarations record build-machine filesystem
  paths; a Linux wheel installed outside that build container exposed permission
  failures when hydration loaded those modules. The reproducible source table
  replaces all four extension source lists without a filesystem fallback, while
  preserving upstream bytes, specifiers, ops, state and adapter checks. Source
  inventory changes fail closed. Relocation and actual installed artifacts remain
  explicit validation gates; embedding alone is not a wheel success claim.
- Align npm, Rust, NAPI and Python at version 0.5.4. The release uses matching
  annotated `v0.5.4` and `pyv0.5.4` tags on the same final merge commit for
  npm/crates.io and PyPI respectively.
- Publish and integrate thirteen reproducible, licensed transport/Deno/V8
  distributions, preserving the hardening and benchmark/core type edges. Seven
  explicit Cargo aliases replace the root patches. The normal resolver replaces
  twelve runtime packages at the same versions; all unrelated lock records stay
  unchanged. Public DOM0.5.7 and layout0.3.8 are also registry dependencies after
  all open producer PRs merged. The earlier core package preflight failed before
  compilation because original registry wreq0.16.1 lacked `chromium-roots`; that
  failure predates the named dependency integration. The exact reviewed
  dependency publication passes thirteen normal package verifications and eleven
  fresh registry consumer tests with no skips. Independent archive audits verify
  runtime/license bytes, complete normalized manifests and real checksums. Actual
  Surf package, wheel/sdist and final registry consumers remain separate gates in
  [PUBLISHING.md](./PUBLISHING.md).
- Merge all open producer PRs before their single patch releases: turbo-dom
  0.5.7, turbo-html2pdf0.3.8 and turbo-test0.4.7. Verify their actual registry
  payloads and immutable merge tags, including HTML2PDF PyPI wheels and sdist.
  Officially re-vendor from the released turbo-test merge; the source bytes are
  unchanged from the tested revision, with only the adapter revision updated.
- Test the actual thirteen dependency archives with coherent Deno/V8/serde
  runtime types, hardened CryptoKey snapshots, ordinary/snapshot weak-handle
  retirement, and default/prefixed transport API and local TLS controls. Preserve
  archive checksums, source guards and failed attempts separately. The upstream
  V8 test harness lacks its published ICU fixture; independent teardown tests
  retain the reviewed assertions without claiming that harness passed.
- Support explicit Deno package names in the experimental SDK build helper using
  private manifest aliases. Keep verified native155 sources, manifests and
  archive intact; preserve the native backend and reject foreign alias files.
  Add offline regression checks and document the generated metadata bridge in
  the Chrome parity runbook. Full integration remains a separate release gate.
- Give the four extension/benchmark distributions explicit default V8 forwarding
  while preserving caller opt-out. Standalone package verification exposed their
  reliance on an enclosing runtime to select a backend. Twelve isolated feature
  resolution controls cover old defaults, repaired defaults and opt-out for all
  four; Rust/JavaScript sources and dependency feature lists remain unchanged.

- Select the existing Tokio runtime by default in the standalone transport
  utility distribution. Its enclosing Surf graph already selects Tokio, but
  isolated package verification exposed the missing default. Preserve emulation,
  dependency opt-outs and explicit Compio selection. Three isolated resolution
  controls and the failing predecessor regression cover the repaired policy.

- Add a standalone six-library package consumer with exact release requirements,
  shared DOM/runtime type checks and actual MCP-to-raster canvas execution. The
  version checker now binds its own version and all six dependency requirements
  to the release version. Document normal multi-package archive verification,
  independent registry checks and actual wheel/sdist consumers.
- Compare the Python binding version with installed distribution metadata in the
  existing smoke test, replacing its stale0.2.5 expectation. Actual wheel and
  source-tarball consumers remain required release checks.

- Require exact release tag/version agreement and real installed-wheel checks
  in publishing CI. Each platform runs all ten existing offline Python smoke
  tests; a separate source-tarball job rebuilds and tests its wheel against the
  registry before uploading the sdist. Preserve artifact hashes, extension paths
  and JUnit results. Missing PyPI credentials fail a requested tagged release.

- Exercise each staged npm platform binary over MCP stdio before upload. A shared
  bounded supervisor requires initialization, genuine loopback navigation and
  evaluated DOM/native random-byte/stream behavior with Deno globals hidden.
  Thirteen offline supervisor controls cover protocol failures, cancellation,
  timeout cleanup and lingering children. Pack the assembled launcher, verify all
  four binary payloads and licenses, then repeat actual JavaScript execution through
  the extracted launcher before publishing that exact tarball. Initialization alone
  did not expose missing build-machine extension sources.
  Manual npm/Python dispatches validate artifacts without publishing; Rust
  publication requires the exact release tag and keeps the verified lockfile.
  Linux binary/crate release jobs explicitly install libclang for the transport
  binding generator alongside the BoringSSL build tools.
- Package the full root MIT text through synchronized local `LICENSE` files in
  all nine Rust crates, retaining SPDX `MIT` metadata. Cargo archive, maturin
  source-tarball and wheel-metadata controls verify exact license bytes without
  manifest warnings. Local discovery also keeps license paths inside the Rust
  workspace. Actual product archive/wheel audits remain separate gates.
- Bound the clock test's `Date()` read with its own before/after timestamps,
  retaining native string round-trip checks and failure snapshots. Separate
  clock reads may cross a second boundary; six frozen-runtime controls accept
  that crossing and reject invalid, stale, non-string and ISO-only substitutes.
  The earlier CI failure lacked those snapshots, so its exact cause remains
  unconfirmed. Production clocks, timer budgets and coherence assertions stay
  unchanged.
- Keep native `std::bit_cast` where supported and supply checked, byte-preserving
  runtime conversions for older manylinux C++ libraries. Actual ARM wheel
  compilation exposed the missing library interface. Reusable native/legacy
  compiler controls cover float32/64 finite values, signed zeros, subnormals,
  infinities, NaN payloads and array/SIMD bits, plus three invalid-type failures.
  The old header fails the legacy control; both repaired paths pass on Node20/22.
  Native constexpr behavior and upstream audio algorithms remain unchanged.
- Keep the forced legacy bit-cast fixture independent of standard `<array>`,
  which needs internal `<bit>` helpers on GCC13. The fixture deliberately
  shadows `<bit>` with an empty legacy header; a plain trivially copyable
  lane array preserves the bit-pattern and SIMD controls without breaking the
  compiler library itself. Production audio headers and algorithms are unchanged.
- Declare Python's MIT expression and `LICENSE` file explicitly under PEP 639
  with maturin 1.9.3 or later. Actual metadata controls reproduce implicit-license
  loss after sdist relocation and preserve the full MIT with the explicit policy.
  Both direct and rebuilt wheel workflows compare actual MIT/notices payloads
  with the tagged source; Cargo SPDX metadata and local license files are unchanged.
- Select explicit `manylinux_2_28` for both native Linux Python wheel builds,
  setting the supported glibc floor to 2.28 while retaining CPython 3.8+ abi3.
  Both earlier manylinux2014 links failed when GCC10 nonshared exception
  definitions collided with V8's bundled libc++ definitions. The newer baseline
  may avoid that supplement through its base shared C++ runtime. Actual linking,
  ELF compliance and installed tests gate publication; the exact symbol-resolution
  mechanism remains unconfirmed. The pinned V8 build source targets glibc 2.26 or
  later, which is not an audit of the actual archive. Compiler, exception and ABI
  flags remain unchanged; no duplicate-symbol suppression is introduced.
- Include `<cmath>` directly in the compiled ARM NEON delay source for
  `std::isnan`. The Linux ARM release build exposed reliance on a transitive
  platform header. Preserve the audio algorithm and original upstream hashes;
  document this portable-header adaptation in its provenance.

- Validate the prepared thirteen-archive Surf graph in both profiles: stock
  passes 1,464 workspace tests with 11 documented ignores; native 155 passes 1,509
  with 8 ignores. Both pass strict Clippy and formatting with unchanged source
  guards. The stock addon passes shim 96+1 and E2E 20 on Node 20/22; focused
  optimized-runner checks close six artifact-dependent skips on Node22. The
  rebuilt native155 addon passes the same shim/E2E gates on both Node versions;
  full native detection passes206 on Node22 and197 with nine documented
  WebSocket-capability skips on Node20. Loaded engine versions and exact artifact
  paths are verified. These staged integration results remain separate from
  final registry consumers.
  Standalone PyO3 feature inspection also confirms reqwest/rustls resources;
  whole-workspace feature unification must not be reported as wheel defaults.

### Runner timing follow-up — 2026-10-20

- Publish the fresh546d511 API and Google measurements. All75 interfaces are
  measured:64 original-four missing own members,142 expanded missing members and
  374 difference paths. One fresh signed-out Chrome baseline reaches200/search
  with nine markers; the other fails effective consent dismissal before input.
  Both native baselines save consent, perform all six actions and use real forms,
  then receive429 with zero markers. Early homepage budget exhaustion persists.
  All14 process records and unchanged-source/artifact guards are retained; four
  Chrome captures exit1, so the matrix is not a complete successful comparison.
  Zero completed hydration pumps does not establish that no pump began.
  The sanitizer passes21 privacy/schema controls and eight historical positive
  diagnostic controls. Native clearance and broader residuals stay in issue28;
  audio stays in issue27. Raw traces remain private.
- Remove closed stock/native debug caches and five duplicate test executables.
  Archive and hash-verify eight failed-test temporary directories before clearing
  their originals, plus27 empty directories. Preserve operative binaries,
  addons, SDK archives/source and all diagnostic evidence.
- Fix a real Chrome post-submit collector failure when the current document has
  no root. Both Chrome backends read root presence, markup and result counts in
  one synchronous evaluation; preserve each backend's doctype serialization and
  current-loader HTTP attribution. A rootless document produces empty markup,
  zero markers and an incomplete observation, with no extra wait or stale-page
  fallback. Focused Node20/22 checks pass29 each; two genuine rootless controls
  and all16 paired controls pass. Full detection passes200 on Node22 with zero
  skips and191 on Node20 with nine documented browser skips; Python passes twice
  on each. The genuine resource-order recapture changes only two source hashes;
  its focused Rust consumer passes. Rust engine, optimized binary and budgets
  are unchanged. Preserve the original incomplete Google attempt; fresh full
  API/Google captures follow the repair.
- Keep the public TLS echo diagnostic out of default offline gates. Explicit
  runs now fail on a request error or30-second deadline rather than returning
  before their wire assertions. The unchanged localhost ClientHello control and
  one explicit live run both pass; JA4, HTTP2, UA and header checks are retained.
  Strict workspace Clippy and formatting pass. Preserve the earlier interrupted
  workspace attempt and its roughly145-second external request; historical
  nominal passes do not prove the old live assertions executed. This is a test
  procedure repair, not a production transport change or Google clearance.
- Correct the compression-dictionary lifecycle regression after Linux CI observed
  an independent resource-error task after an already-due Window timer. Preserve
  every event count, readyState and other ordering constraint; permit only the
  adjacent `link:error` / `future-timer` reversal. Positive and mutation-negative
  controls retain parser/load barriers and reject missing or duplicate events.
  Runtime, fixture, genuine Chrome golden and deadlines are unchanged. Nine
  focused native dictionary tests, full-workspace strict Clippy and fmt pass.
  The original Linux failure remains recorded; exact scheduling parity is not
  claimed. A current full workspace rerun and new exact-head CI follow.
- Complete the native gate at4a734d6:1,496 Rust tests, strict Clippy, fmt,
  optimized CLI/addon and loader proof, shim96+1 intentional skip, E2E20,
  Node22 detection195 with zero skips and Python2 pass. Six generated Rust
  doc cases remain ignored. Fresh warm-artifact queries and source/fixture/full
  Chrome guards pass. These retain their actual4a identity; the subsequent
  test-only correction changes no production or harness input. Current default,
  optimized API inventory and fresh Google validation remain pending.

- Separate fixed observation semantics from timer delivery speed. Preserve the
  original600/20ms delays and100/1000ms windows, carry created/fired clocks on
  the existing retry navigation, and verify actual document responses and server
  events. A shell stays a rejected search; unresolved navigation stays incomplete.
  A timer-free302 control independently requires the real429 destination. Keep
  provisional-loader behavior under deterministic tracker tests rather than require
  a snapshot to sample a transient pending state. Original timing failures remain
  preserved; this does not establish scheduling parity. The four paired suites
  pass14 tests with zero skips, covering10 variants /19 actual captures; source,
  native debug binary and complete frozen Chrome bundle remain unchanged.
- Fix shared completion reporting for the actual native trace schema: native
  top-level `errors` contains collector failures, while Chrome uses
  `collectorErrors` and records guest exceptions separately. Missing, malformed,
  nonempty or conflicting channels fail closed. Matching public-summary and
  navigation regressions pass40 focused checks on both Node20 and22. Earlier
  native Google captures retain their actual429/zero-result outcomes; this fix
  does not turn them into successful searches. Full current-source native/default,
  optimized API and fresh Google checks remain pending.

- Prime the existing shared system-font cache on a blocking worker before
  PageSession creates V8. Cold discovery now leaves the current-thread network
  driver responsive, retaining eager geometry startup and the existing guest/input
  budget boundaries. A fresh-subprocess regression holds the real cache initializer
  until a real localhost round trip progresses through PageSession startup.
  The native focused gate passes67 tests, strict affected-crate Clippy and formatting.
  The unchanged same-URL POST regression passes; its observed POST precedes idle
  expiry, while the deterministic startup test separately verifies driver progress.
  Earlier repeated failures attempting POST4.29 seconds after server socket closure
  remain preserved. Full current-source validation follows this integration;
  cold-font latency/memory and bootstrap performance remain in issue #28.
- Keep aborted CDP commands as rejected Promises, so background-target finalizers
  catch them and finish owned browser/profile cleanup. Actual lifecycle/connection
  tests pass7/7 on Node20 and22, and real CDP/Patchright interruption controls
  pass2/2 at the original45-second limits. The old synchronous contract fails the
  regression. Re-capture the source-pinned resource-order Chrome oracle genuinely;
  its observable results remain unchanged and both Node versions pass source pins.
  Retain the earlier188-pass/3-failure Node22 run, including the truthful200 shell
  when native page bootstrap consumed the fixed retry observation window.

- Synchronize the idle-receiver cancellation regression with explicit child
  readiness and server-confirmed stalled I/O, without another receiver RPC.
  Separate bounded30-second process startup from the original five-second
  cancellation operation; the original five-second-from-spawn failures remain
  preserved. A traced cold open spent18.07 seconds loading system fonts with
  a2.6GB footprint before cancellation was reached. All190 sampled worker
  stacks are font-loading work; source shows complete TTC bytes copied per face.
  The startup cost remains in issue #28. Await child `close` and socket teardown
  before asserting output; passing cancellation does not certify cold startup.
  Both complete shim suites pass96 tests with one intentional skip each on
  actual stock and native addons; compiled Rust/artifact bytes are unchanged.

- At the earlier581d checkpoint, complete default `npm run check`:1,450 Rust tests, strict Clippy,
  formatting, rebuilt addon/shim96, E2E20, detection184 and Python2 pass.
  Explicit skips remain nine Rust, one framework and seven Node20 browser
  controls requiring Node22. Actual addon loading/code identity is verified;
  these precede the subsequent startup repair and do not validate current Rust.
  Full current-source native/default and fresh optimized API/Google checks remain pending.

- Re-vendor public canonical `a49544f`: computed styles preserve parsed
  declaration importance through shorthand expansion, respect private document
  adopted sheets in source order, and bypass poisoned public priority/root
  getters and numeric Array prototype setters. Canonical checks pass 299 library,
  52 compatibility and63 Node tests; all four exact-head platform CI jobs pass.
  The unrelated computed custom-property omission
  remains explicitly tracked in issue #28.

- Expand physical border shorthands before native cascade winner selection, so
  author declarations retain their origin, importance and source-order priority
  over UA side widths. All nine measured border controls pass in the rebuilt
  native engine, alongside genuine same-embedded-font Range controls. The Times
  glyph-width mismatch remains tracked in issue #28.
- Reuse supervised children in the API inventory runner, retaining its60-second
  deadline, full private logs and owned-process cleanup on timeout/abort. Six
  focused tests pass on both Node20 and22; historical API inventory stays intact.

- Reject malformed collector-error metadata when deciding modern capture,
  observation and SERP completion. Empty arrays pass; absent legacy evidence stays
  unmeasured. Focused comparison/summary regressions pass31 each on Node20/22.
- Preserve a genuine frozen-Chrome timeout/interval conversion control, including
  huge finite values, Infinity and signed32 boundaries. Matching native coercion
  and defensive scheduler overflow guards pass the fresh native integration run.

- Integrate the native inline/block layout repair from producer PR24: split real
  block descendants out of inline flow, preserve continuation and Range ownership,
  propagate collapsed margins and quantize native intrinsic widths. Preserve
  zero-size client rectangles without inflating integer offsets. The producer's
  final local gate at public `2ea52368` passes858 workspace tests and15 xref
  controls, strict Clippy, formatting and complexity checks. Exact-head CI passes
  all three jobs, including100% coverage. The rebuilt native integration passes
  66 tests, affected-crate strict Clippy and formatting with unchanged source and
  compiled fixtures. The unchanged four paired suites pass13 tests with zero
  skips, including9 observation variants / 17 captures. Full native-feature
  validation and optimized live comparison remain pending.
- Use measured Chrome button defaults at native UA priority, retaining author
  overrides. Officially re-vendor canonical turbo-test `030c776`: matching computed
  button styles, repaired Node/EventTarget constructor identity, and real Text and
  DocumentFragment constructors with native creation, conversion and subclass
  behavior. Canonical library295 and nine compatibility fixtures52 pass, as do
  all four platform CI jobs at that exact head. Its
  supplemental strict attempt still reports28 unchanged-base diagnostics; that
  result does not replace turbo-surf's required strict gate.
- Save the genuine Chrome constructor/ShadowRoot probe and control as reusable
  harness assets. Combined turbo-surf and native155 checks remain pending after
  these integrations; component passes alone do not establish Google acceptance.

- Add a central [Chrome155 parity runbook](harness/detection/CHROME-PARITY-RUNBOOK.md) with exact native SDK/build commands, feature and tracing flags, timing options, artifact checks, test order and evidence limits; link it from the agent documentation index, SDK, engine and main harness docs. Document inherited environment/`.env` effects, addon shadowing, browser version drift and incomplete control-path selection.
- Supervise test and matrix children with abort signals, bounded cleanup and full
  private output logs. Retire owned groups even when their leader exits first;
  test teardown waits for retirement before starting the next case. Nine offline
  controls pass under actual Node20 and Node22, including intentional outer-test
  timeout and resistant-descendant cases. Preserve timeout failures and original
  success deadlines.
- Let both Chrome backends and all four collector suites select the explicit
  Chrome executable. On interruption, checkpoint incomplete captures and retire
  only the owned browser. Verify actual typed insertion, retained control identity
  and focus before Enter, accounting for existing text and UTF-16 selections;
  equivalent native host guards and startup changes pass47 focused native tests
  and four example tests, strict affected-crate Clippy and formatting.
- Initialize diagnostic render hooks without loading solver configuration or
  `.env`; production session initialization retains its existing behavior.
- Add a reusable complete-app Chrome freezer with signature verification, source
  before/after and copy content checks, internal-link validation and failure
  receipts. Nine Node22 offline controls pass. Preserve installed Chrome and user
  profiles; omit Finder metadata only from the owned copy. Fixed-version actual
  Chrome controls and a genuine oracle refresh pass8/8. Actual Chrome is
  155.0.8059.85 / V815.5.35.25; native remains .80 / .23, an open profile/engine gap.
- Verify both Chrome interruption paths against real owned processes: preserve
  incomplete traces, retire observed helpers and remove owned profiles. Both
  controls pass without fallback kills. Public summaries require all measured
  process-completion fields;14 controls pass on Node20 and Node22. Legacy captures
  missing these fields remain unmeasured.
- Preserve a newly exposed native block-in-inline layout failure in the combined
  run (12 passing tests, one failure). Fifteen paired captures complete; the
  existing-results fixture fails geometry before consent or search. Keep this
  layout defect separate from Google acceptance and from the earlier input and
  timeout failures.

- Run detection test files serially to bound competing browser/V8 workloads,
  preserving all deadlines, assertions and skips. The prior concurrent failure
  remains recorded; its cause is unproved. Unchanged isolated controls pass3/3
  and the full serial diagnostic passes 134 with four existing skips.
- Measure a real comparison gap with reusable localhost scenarios: a600ms retry
  produces a Chrome200 shell versus native429 at the current observation point,
  and native guest typing clocks advance about twice the real typing duration.
  The slow-response and20ms retry controls agree; the failed timing control is
  preserved rather than relabeled as a passing comparison.
- Add opt-in real-time Window scheduling before hydration and through frame/form
  navigation, preserving default virtual crawler scheduling and original budgets.
  A one-shot hydration cap leaves later operation budgets unchanged. Ten focused
  default-engine controls pass; genuine-native155 passes40 integration controls
  and two example tests, strict affected-crate Clippy and formatting.
- Observe both engines against one deadline starting before final Enter. Preserve
  overruns, unfinished/failed main navigation, hop limits and timeouts. Guard
  snapshots against loader changes and distinguish main-document requests from
  iframe/XHR traffic. Public summaries require complete observation and real-time
  native mode; absent legacy evidence remains unmeasured. Thirty focused
  comparison/navigation tests and twelve public-summary tests pass; actual paired
  timing controls, final integrated gates and fresh Google verification remain pending.

### Chrome control and diagnostic follow-up — 2026-10-19

A signed-out native Chrome Incognito UI control reaches real results for the
same `rust lang` query, with Google's sign-in link visible before and after
submission. This is one successful UI control, not a reliability measurement or
an empty-profile control: existing Incognito cookie state was not measured and
extensions were present. It does not establish native search acceptance or the
cause of the traced collector's challenges. The earlier logged-in control is a
separate observation. Historical captured Google matrices remain unchanged.

- Separate guest script errors from collector completion. Preserve both kinds of
  errors and label legacy completion inference explicitly.
- Exclude hidden query fields from input diagnostics and record the selected
  control type/counts. This reports type-based selection, not computed visibility.
- Record the Chrome document's actual HTTP status, ready state and response
  identity. Accept a Chrome SERP only with a completed capture, a same-origin
  `/search` document, HTTP200, complete DOM and organic result headings.
  Receiving an HTTP200 shell alone does not qualify.
- Align harness readiness, real pre/post dwell, per-step snapshots and minimum
  host-wall typing cadence. Recompute trusted native click coordinates from the
  private current-layout operation, and replace live input/textarea selections
  while respecting canceled editing events. Initialize the existing shared system-font
  registry during geometry setup so a first click does not incur cold font parsing
  inside its input budget; this shifts that real cost to engine startup. Add paired localhost action evidence
  and an explicitly selected Incognito comparison mode. Rebuilt native155
  diagnostics/input pass19/19, native duration2/2, paired/native-process controls6/6
  with zero skips, and separate real-Chrome collector controls6/6.
- Pin the public turbo-dom attribute-selector repair `d01d6a5` in all six
  consumers and the lockfile. Preserve the shared hidden-field exclusion selector
  and its failing fixture; the repair belongs in the matcher. Its full producer
  Rust/JS/native/WASM gate and exact-head CI pass; Surf native155 integration
  passes the genuine paired action control.
- Reject malformed custom-step operands in the common driver before artifacts or
  backend launch. Keep Chrome string/timer coercion from producing a different
  plan than native; accept empty typing and zero waits explicitly.
- Verify actual offline action alignment in baseline and interface modes: all six
  actions, real consent coordinates/response/cookie ordering, focused textarea
  query, trusted events, genuine native form submission and final HTTP200/results.
  Missing readiness fails before input on both engines. Full default checks and
  fresh optimized Google validation remain pending; this does not certify live
  Google acceptance or identical event timing.

### Previous full validation checkpoint — implementation a005140

Implementation source `a005140` passes the complete stock-V8-150 workspace
(1,411 tests, nine ignores and six zero-test native-only targets) and genuine
native-V8-155 workspace (1,456 tests across 134 suites, six generated-doctest
ignores), with zero failures. Strict all-target Clippy and formatting pass;
all 1,457 selected implementation source hashes remain unchanged. Both Linux
Node20/Node22 CI jobs pass. Rebuilt native-addon checks pass: shim96 with one
intentional skip, E2E20/20, detection123 with two Node20 CDP skips, Python2/2;
separate Node22 real-Chrome CDP controls pass3/3 with zero skips.

Fresh paired API measurement covers75 interfaces with no unmeasured entries:
64 missing own members across the original four interfaces,142 expanded missing
members and374 descriptor difference paths. Complete member/difference arrays
are unchanged from the preceding inventory; this is measured shape/ownership
coverage, not full behavioral conformance. All remaining API and browser parity
work stays in issue #28; advanced audio stays in issue #27.

The fresh `a005140` Google matrix completes all14 processes without process
timeouts. Both Chrome baselines and both native baselines reach `/sorry/index`
with HTTP429 and zero result markers: native0/2 and Chrome0/2 accepted SERPs.
Both instrumented Chrome passes also receive429. All eight consent responses
are204 before search. Native uses genuine form navigation, with no fallback,
imported Chrome cookies or sidecar. All four native homepage passes exhaust the
20-second hydration budget with zero completed pumps; dismiss retains that
failure, and search subsequently reaches the real native429 response. The budget
residual remains in issue #28. No jc/lc requests appear in any of the eight
baseline/instrumented captures; their absence does not establish a rejection cause.
These Node22 captures are separate from the historical Node24/source59f7835 run
(Chrome7/8 result markers, native0/2). No native SERP success is claimed.

### Reusable diagnostics and browser identity

- Add reusable normal headed Chrome controls using the producer's exact bundled
  IBM Plex Mono and Fira Code bytes. Pin font, probe, collector and socket-helper
  hashes; record selected fonts and preserve separate headless metrics. Capture
  whole/split runs, decorated boxes, partial Ranges and eleven transform contexts,
  retaining the explicit unsupported fixed-containing-block boundary. Bound
  WebSocket startup and cover timeout, early-close and error cleanup offline.
  Add a nineteen-scene whitespace control with individual collapsed-space,
  wrapped Range endpoint and caret-affinity observations, plus an exact end-to-end
  DOM/CSS regression. Genuinely recapture
  all three modes after collector changes; preserve previous geometry exactly.
- Pin public layoutdcbfd20, including native source-run advance admission before
  wrapping and placement, preserving raw glyph intervals for partial Ranges.
  Apply Chrome's float transform stages to native text selections and actual
  block/replaced border quads; retain ordinary inline and PDF mapping. Keep
  same-source merges within compatible projection contexts and collect containing
  blocks in one pass. Preserve leading spaces inside their real inline boundary,
  place collapsed successor offsets after the rendered space, and carry collapsed
  source prefixes with wrapped spans or bare text. Retain actual internal-space
  UTF-16 cells in bare text, preserve wrapped Range endpoints and canonicalize
  caret offsets only through connected zero-space cells at the winning edge.
  Keep positive leading spaces and pre/nowrap glyph provenance; generic/PDF paths
  stay unchanged. Document the new public Rust source-cluster provenance fields.
  Producer767 full tests and41 focused controls pass. Current stock/native155
  inline targets each pass5/5 with1457 unchanged source hashes and all nineteen
  Chrome scenes. Final a005 full stock/native gates pass1,411/1,456 tests,
  strict checks and formatting; rebuilt native addon/Node/CDP gates also pass.
  The subsequent test-only producer follow-up `4999290` passes768 tests and coverage
  9,806/9,806 without changing pinned dcb production bytes.

- Preserve per-element custom-element reaction order across reentrant adoption,
  including definition-realm callbacks and error routing. Parse Range fragments in
  their actual post-conversion context with native namespace, form-ancestor,
  quirks-mode and UTF-16 handling; preserve scoped registries and inert-template
  ownership without premature upgrades or spurious observer events. Clear retained
  shadow-root adopted stylesheet lists before cross-document adoption callbacks,
  preserving same-document moves and disconnected roots. Stop automatic construction
  and queued reactions after a definition frame retires, retaining registry lookup
  and existing instance identity. Re-vendor
  canonical turbo-test 3ed91fb and pin the additive public turbo-dom aaeb290 API
  consistently across all six consumers.

- Drop collected weak context registrations at host boundaries after actual V8 GC,
  preserving live retained documents and avoiding native Tree traversal inside GC
  callbacks. Add independent nested-frame/retained-constructor ownership controls,
  exact registry reclamation and reentrant Window transfer retirement regressions.
  Vector backing capacity retains its high-water mark; this is not full OS-memory reclamation.
  Capture isolate-shared WebCrypto descriptors and native DOMException builders in
  the main realm before any initial markup iframe can bootstrap. Preserve child
  crypto/clone exception identity while allowing the first child realm to collect;
  actual heap traces identify both former strong-root paths. The unchanged
  reentrant navigation assertion and initial-markup retirement/realm controls pass
  in the76-test stock focused cohort and20-test genuine-native155 lifetime/crypto
  cohort; final complete gates remain separate.

- Roll back native ownership when iframe initialization fails before publication,
  without calling guest helpers in partially installed or terminating realms.
  Resolve child viewport layout when metrics are read so unsupported parent CSS
  does not prevent frame scripts from starting. Add host-only one-shot fault
  controls for exact cleanup and same-host recovery; preserve the existing watchdog.

- Preserve the requested fresh source59f7835 retry separately from the earlier
  same-day capture: Chrome baselines reach7/8 SERP markers, native baselines remain
  0/2 accepted, and both instrumented Chrome passes are challenged. All14 processes
  finish without process timeout; no native cookie import or form fallback is used.
  Preserve3/4 native homepage watchdog failures, including one clean baseline;
  later navigation completion does not erase initial-open diagnostics.

- Preserve the earlier source88528ae October17 consent/search rerun: all14 processes finish;
  native and Chrome baselines both accept0/2 SERPs. Native consent204 and genuine
  form navigation work without imported cookies or fallback. Clean native flows
  stay within20s, while one instrumented homepage open exhausts it; its repeated
  dismiss diagnostic is the same event. Keep full-trace boundedness and reliable
  acceptance open in issue28, without inferring a rejection cause.

- Pin authored probe/control line endings to LF so source-hash controls remain
  reproducible on Windows. Preserve the licensed XPath provider's original bytes.
- Retain Chrome response protocol, TLS/security, timing, connection and cache
  metadata, including redirects and raw response events. Public summaries use
  fixed protocol buckets and availability counts; native application-layer
  observations remain separate from Chrome CDP and packet-level evidence.
- Trace configurable methods on returned native WebGL/2D contexts and WebGL
  extensions, including own methods. Report skipped/unsupported surfaces and
  bounded inspection coverage; clean controls retain their original behavior.
  Intrusive wrappers remain observable and do not capture every JavaScript access.
- Traverse ordinary native light-tree children directly during layout capture,
  retaining private shadow-host/slot distribution and termination checks. Preserve
  stylesheet scope order with indexed membership rather than repeated linear scans.
- Reduce repeated resource-order walks during parser hydration while preserving
  synchronous mutation fallback and native collection order. A narrow HTML-script
  guard prevents unsupported SVG/uppercase script nodes from discarding queued
  ordinary HTML scripts; full SVG script execution remains tracked in issue28.
- Reuse private CSS rule records during computed-style import flattening, keeping
  nested import/media order and retained declarations live.
- Warm process-wide font discovery in a separately bounded geometry-test session
  before the existing five-second neutral-rule conformance controls. Preserve all
  original assertions and production budgets; retain the failed cold-start evidence.
- Validate native layout selectors once per invocation before matching current
  node state, instead of repeating the same validation for every candidate node.
  Preserve dynamic matching, scope boundaries and first-error behavior.
- Avoid exponential parent computed-style refresh through inherited color and
  percentage-size chains. Fresh parent values are shared only within the current
  calculation; external retained declarations continue refreshing on each read.
- Refresh the private stylesheet list once per computed-style calculation, keeping
  retained declarations live. Independent Chrome controls cover stylesheet edits,
  replacement, disabled state and owner-document behavior.
- Add native hydration phase/pump diagnostics and opt-in Inspector CPU profiles
  that survive watchdog termination. The execution budget remains enforced;
  collected profiles and script sources stay in private local artifacts. Optional
  host-stage aggregates separate layout input capture, rule/key work, cache hits/
  misses and actual layout, with explicit inclusive timing scope.
- Separate source-less native script-pump reads from page queries in interface
  traces. A real-stack regression preserves page callees, return identity and
  timing; source/ancestor attribution remains a diagnostic heuristic. Refresh
  product and engine documentation against the final code and linked evidence.
- Add fresh-cookie paired Chrome/native tracing with baseline and instrumented
  controls, JS interface arguments/results/stacks, request payloads, response
  bodies, debugger source inventories, failure checkpoints and build hashes.
  Raw CDP captures include out-of-process frames/workers and session-qualified
  network events. Native runs use compiled wreq, Metal WebGL and CoreText features;
  Chrome oracle cookies are never imported into the native runtime.
- Add a reusable sanitized run summarizer with executable-hash verification,
  consent ordering, missing/failed capture status and explicit native acceptance
  conditions. Preserve cookie names while excluding values, bodies, query strings
  and raw diagnostics; reject malformed evidence instead of silently losing rows.
- Save consent dismissal in the shared harness and both collectors. Await bounded
  asynchronous probe results privately instead of reading a guest-controlled
  global slot. Optional capability, crypto, integrity and GPU probes run after
  the recorded verdict. Offline harness tests are part of the standard CI gate.
- Expand the API audit to75 interfaces with exact missing/extra members, callable
  shapes, descriptors, inheritance, raw key order and collector/executable hashes.
  The fresh Chrome155.80/native155 pair measures all interfaces:64 missing own
  members across the original four,142 in the expanded scope and374 difference
  paths. On the same68 previously measured interfaces, missing members fall134→119
  with15 resolved and none introduced. Both formerly absent collection interfaces
  are now present; the five added interfaces contribute23 additional missing
  members. Historical counts and smaller scopes remain explicitly dated.
- Refresh JS/HTTP identity to independently measured Chrome155.0.8059.80.
  Preserve historical controls and SDK source provenance155.0.8059.79. The tested
  native engine is15.5.35.23-rusty; the stock production SDK remains V8150.4.0
  (engine15.0.245.2). Profile metadata alone does not upgrade the engine.
- Isolate native SDK manifests/locks from the production workspace. Reproduce
  native WindowProxy/context, module-source, code-generation and teardown patches
  from licensed owned source and hash-verified archives. Keep historical154
  profiles reproducible; final production-engine/distribution alignment is open.
- Align npm/Rust/Python metadata at0.5.4, derive runtime versions from Cargo and
  check versions/notices in local/release gates. No Surf publication tag is created.

### Networking, Fetch and storage

- Fix the functional native XMLHttpRequest constructor name, standard tag, global
  enumerability, six network-method descriptors/arity and readonly ready-state
  constants. Capture private WeakMap operations and reject forged/proxy receivers
  before coercion or state access, while allowing genuine subclasses and detached
  prototypes. An independent Chrome155 control covers descriptors, hostile hooks
  and real HTTP delivery; four existing request/frame controls also pass. Broader
  XHR state, hierarchy/upload/progress and transport lifecycle remain in issue28.

- Use the Chrome154 BoringSSL transport recipe with measured155 navigation
  identity, real trust-anchor IDs, ML-DSA offers and signature GREASE. Pin the
  additive connector-metadata API to controlled wreq fork4ed3cf0c (upstream
  basec6fd8b1; upstream PR1327), wreq-util5715529 and btlsf5a8a48. Retain raw
  ClientHello bytes and measured H2 fields. Matching bounded localhost controls
  does not establish complete wire parity. Registry APIs/distribution remain open.
- Negotiate session Accept-CH and construct navigation/script/fetch-specific
  headers, Origin/referrer/credentials and redirect metadata. Native response
  effects retain cache/storage notices across overflow/retry without rooting
  retired realms; late frames do not receive historical clear notifications.
- Hide HttpOnly cookies from JS while retaining real HTTP cookie behavior and
  preventing script overwrites. Credentialless frames use an actual ephemeral
  top-document network/storage partition, shared by relevant siblings and
  reclaimed on teardown. Native storage persists across same-session navigation;
  peer, clear, profile and partition tasks use private host-owned state.
- Implement genuine bounded response streams/cancellation, initiating-policy
  snapshots and cache/referrer effects. Normalize BodyInit with authentic private
  brands, constructor-time byte/URLSearchParams snapshots, Request stream ownership
  and independent clone tees. Reject invalid duplex/keepalive/no-cors/chunk cases.
- Stream HTTPS/HTTP2 uploads without prebuffering through a one-shot producer,
  two64KiB chunks and a64MiB host budget. Preserve preflights, reject HTTP/1 before
  I/O, rewrite303 toGET and reject307 replay. Immediate guest abort and bounded
  transport cleanup preserve measured cancellation semantics. Resolve fixture
  script paths before launching Node from linked SDK workspaces.
- Add a genuine session-owned compression-dictionary store with origin/partition
  selection, validated hash/ID negotiation, bounded dictionary-dependent Brotli
  and Zstandard decoding, and decoded-cache/304 reuse. Share one codec engine
  between buffered callers and incremental Fetch bodies; expose real headers and
  partial output through bounded workers/channels. Require explicit input/output
  success and actual transport EOF before cache publication; cancellation, panic
  and caps cannot publish partial data. Official C-encoder fixtures and real HTTPS
  controls cover both transports. Chrome publication timing and encoded Range
  semantics remain open in the parity checklist.
- Discover dictionary hints in document headers and actual HTML/shadow links
  through native policy-aware background Fetch, including shared ownership and
  cancellation. Independent Chrome controls cover resource policies and lifecycle;
  use a separate bounded resource task queue with per-frame fairness, microtask
  checkpoints and retirement cleanup. This preserves due-timer/resource/future-timer
  order without holding document readiness. Final full gates and live acceptance
  remain unverified.
  See [implementation and bounds](harness/detection/COMPRESSION-DICTIONARIES.md).
- Apply all eight standard referrer policies to document/request/redirect paths.
  Process active meta referrers at parser and mutation boundaries; ignore inert
  template/shadow/foreign nodes and poisoned getters. Native jobs retain immutable
  initiating CSP/referrer/embedder snapshots before asynchronous body conversion.

### Native DOM, scripts and ownership

- Initially consume public Git-pinned turbo-dom0.5.7 candidateaaeb290 when the
  latest registry release was0.5.6, and re-vendor committed turbo-test3ed91fb.
  The current dependency is released registry0.5.7. Native per-isolate node state preserves
  original UTF-16 payloads, names, parser positions and mutation-time ancestry.
  GC callbacks schedule bounded storage scans outside collection; genuine weak
  wrapper/guest edges and native job pins preserve retained identities without
  permanent host roots. Thread exit, interruption, disposal and peer sessions
  have deterministic lifecycle regressions.
- Allocate actual iframe V8 contexts with independent intrinsics, guarded stable
  WindowProxy access and authentic Location/document authority. Experimental155
  contexts include retained/retired-frame ownership controls; stock150 coverage
  remains partial. Scoped activation restores native world and V8 before yielding.
- Execute classic scripts directly from the host pump with original sources,
  lexical scope/currentScript and trusted errors; preserve real load/error/CORS
  taint behavior. Private preparation replaces guest execution markers and DOM
  rescans. Modules and srcdoc retain original UTF-16 units. Dynamic ordered
  downloads, inert/retired documents and host budgets have regression coverage.
- Use actual V8 microtasks for queueMicrotask and observer delivery, and individual
  Window tasks on shared clocks across frames. Flush microtasks between tasks;
  bootstrap pending scripts before advancing delayed timers. Claim cross-document
  mutation batches before reentrant resource queries to preserve reaction order.
- Implement native mutation observers, transient removed-subtree records, shadow
  slots/manual assignment, template ownership and scoped custom-element
  construction/upgrades/reactions. Genuine brands, private callbacks and
  cross-realm/adoption controls replace public helper authority. Broader parsing,
  observer/custom-element and form-associated behavior remains on the checklist.
- Preserve namespace-aware interface projection, Attr/NamedNodeMap and document
  collections, HTML/XML document factories, native script text, DOMTokenList/part,
  scoped ARIA relations, supported moveBefore and innerText/outerText mutations.
  XPath uses the licensed bounded provider. Standard legacy Document no-effect
  methods retain genuine receiver/descriptor behavior. Popovers have native
  lifecycle/events; focus/light-dismiss/top-layer behavior is still bounded.
- Track live Range boundaries through insertion/removal, split/normalize,
  replacement, moves and adoption without rooting collected ranges. Add genuine
  AbstractRange/NodeRange/StaticRange hierarchy, immutable static boundaries,
  WebIDL conversion and cross-realm receiver checks. Independent native tests
  cover retained nodes/frames and actual GC rather than prototype presence alone.
- Add real form validation/ValidityState/SubmitEvent behavior and pre-submit
  sandbox admission. Queue native top-document text GET/POST submissions through
  the actual HTTP profile/jar/hints/storage, then install response HTML/status/
  headers/referrer after leaving the old isolate activation. Preserve redirects,
  consent cookies, fragments and immutable metadata. Independent UTF-8 and
  Windows-1252 controls cover encoding and numeric entities. Multipart/file-input/
  other-target and broader successful-controls behavior stay explicit.

- Add genuine live HTMLFormControlsCollection/RadioNodeList interfaces, native
  indexed/named properties, SameObject form elements, external association and
  private receiver/realm ownership. Reflect input/button submitter action, method,
  encoding and target through native attributes. Preserve radio checked-state
  grouping, canceled-click restoration, cross-document adoption, nested/fragment
  moves, shadow boundaries and external ownership. A native scalar candidate index
  avoids rescanning ordinary moved subtrees and is swept with native storage.
- Correct the independent radio adoption-GC readiness probe so its Promise does
  not retain the iframe load Event. Record a real-task/three-GC policy and three
  repeat Chrome captures; preserve the original contaminated and immediate
  observations. Check the same exact lifetime states through the production
  PageSession without adding a native ownership root.

- Add genuine live control `.labels` on the seven existing labelable interfaces,
  with native light-tree association, first-ID rules and shared ElementInternals
  ownership. Preserve receiver-realm NodeList identity, hidden-input transitions,
  adoption and GC. Capture the genuine collection prototype before guest code;
  use null-namespace UTF-16 attributes so foreign id/for/type cannot change
  association. Label activation and broader form behavior remain separate.
- Preserve real native I/O/system-timer ordering while selecting Window tasks.
  Keep one cancelable monotonic deadline while native work is pending; do not
  jump a later virtual Window timer ahead of earlier body completion or timeout.
  Independent Chrome controls cover stalled-body cancellation, retained and
  no-public-listener timeout delivery. Quiescent virtual fast-forward remains
  explicit; this is not universal browser timer-policy conformance.

- Harden private abort conversion against guest Array.from, iterator, numeric
  setter and descriptor hooks using private null-prototype records and a
  source-pinned official WebIDL dense-array adapter. Preserve measured iterator
  failure behavior and reason identity; retain the explicit Chrome array
  fast-path iterator-count difference. Use fixed member comparisons so a poisoned
  Array.includes cannot expose an official stream controller to guest callbacks.
  Fresh Chrome controls and actual native attack regressions cover the boundary.
- Return and await the actual final human-driver sequence Promise, including
  rejection propagation and native form commits despite forged DOM completion
  markers. Preserve generic script draining and production execution budgets.
- Give the600-mutation native reclamation conformance fixture an explicit60-second
  budget on shared debug CI runners. Preserve all operations,1200-node reclamation
  and retained-owner assertions; this does not change product limits.
  Apply the same explicit conformance budget to peer-layout isolation after a
  native sample proves cold system-font discovery exhausts its old5-second limit;
  preserve all geometry and ownership assertions. Scope saved macOS shadow Range
  text metrics to that platform while checking exact font-independent boundaries,
  positions, line boxes and retirement everywhere; retain the full macOS comparison.
  Cancellation regressions await actual transport closure separately from worker
  permit recovery under the existing timeout, preserving both lifecycle checks.

### Layout, CSS and resources

- Expose the actual linked renderer V8 version through the host-only NAPI
  `engineVersion()` diagnostic. Keep it distinct from package versions, the
  advertised browser profile and Node's own V8; assert the expected engine during
  native addon validation without adding a guest global.
- Resolve measured initial/inherited computed font longhands and shorthand using
  private owner-context preferences, case-insensitive grammar tokens, typed
  arithmetic and scalar-versus-length line-height inheritance. Follow actual assigned-slot inheritance and flat-tree availability;
  refresh retained declarations after mutation, adoption, disconnect and frame
  retirement. Preserve conversion order and captured numeric intrinsics. Broader
  UA element defaults and unresolved font expressions remain tracked in issue28.
- Preserve native inline source boundaries through shaping and wrapping: genuine
  decorated/empty/mixed-font border fragments, own-font struts, nested baseline
  shifts, relative offsets and source-owned whitespace/BR Range geometry.
  Distinguish ordinary inline client metrics from replaced controls; preserve
  empty fragments in offset bounds without duplicating ancestor paint.
- Derive child/nested frame viewports from actual content boxes and refresh them
  on resize without consulting guest dimension getters. Preserve measured
  Replaceable accessor writes, borrowed media-query receivers, definition-realm
  errors and cross-origin guards. Retired frames return empty retained geometry
  and zero viewport dimensions while their retained DOM remains usable.
- Use genuine iframe default object dimensions, UA borders and content boxes;
  preserve finite zero-sized child viewports, definite-zero percentage bases and
  normal-flow relative positioning. Resolve zero-axis aspect-ratio media queries
  using the native parser and rational comparison. Suppress native iframe fallback
  layout while retaining DOM/UTF-16 ownership and empty fallback Range rectangles.
  Preserve the separate generic/PDF behavior. BackCompat body-strut geometry
  remains explicitly tracked in issue28.
- Preserve the complete exact macOS UTF-16/font geometry oracle and run exact
  font-independent code-unit, selection, private access and caret lifetime
  contracts on every platform. Linux fallback fonts differ from the captured
  macOS system-font environment; Linux glyph parity remains unmeasured.
- Connect native live-tree snapshots/private CSSOM to branded per-realm rectangle
  interfaces, transforms, client/offset metrics, scrolling/clamping, point hits,
  shaped original-unit text, Range/caret geometry and outer SVG boxes. Preserve
  real source identities without HTML rewriting or page-owned getters.
- Integrate composed closed-shadow/slot layout, reassignment, retained/unassigned
  Range behavior, scoped native matching and real CSP-applied declarations.
  Host-only realm cache epochs prevent independent sessions with equal document
  IDs from sharing snapshots. Share actual platform font loading across threads.
- Initially consume public Git-pinned turbo-html2pdf-core0.3.8 candidate2eac7f6
  from PR23 when the latest registry release was0.3.7. The current dependency is
  released registry0.3.8. The producer adds display:contents,
  matched cascade, fractional anonymous flex/grid text and platform-only fonts.
  Publication was verified separately from development integration.
- Parse live CSSOM rules and preserve disabled/media/adopted sheet identity and
  constructor ownership. ObservableArray adoption has native backing. Public
  CSS.supports uses private grammar/capability checks; selector matching retains
  quoted commas/braces and genuine :is/:where specificity and relative :has.
- Admit parsed font-face/keyframe definitions only when actual matched styles
  prove them unused. Referenced/unresolved definitions still reject. Complete
  unsupported inline alignment/writing/positioning and block splitting, used
  fonts/animations, stacking/scrolling,
  inner SVG and broader CSS/resource behavior remain tracked in issue28.

### Security, messaging and native APIs

- Add genuine private Trusted Types values/factories/default policies, allowlists
  and typed HTML/script/attribute/Range/DOMParser conversion. Native155 codegen
  admission preserves direct eval lexical scope, real EvalErrors, string timers,
  original report samples and retained-policy GC behavior. Stock SDK enforcement
  remains conservative; broader sinks/default-policy reentrancy are tracked.
- Enforce parser/mutation CSP and base-uri with original UTF-16 positions/samples,
  native branded violation events and bounded trusted report queues. Deliver
  legacy/modern endpoint reports using immutable profile-owned jobs, cookies,
  request policy and redirect semantics; frame retirement permits realm collection.
- Enforce tested CSP sandbox/frame-ancestors/XFO, mixed content, SRI and CORP,
  including redirect/CORS eligibility, actual ancestor trees and inherited frame
  policy. Implement tested COOP/COEP isolation, agent-cluster shared-memory
  restrictions and Permissions Policy capability/delegation. PermissionStatus
  brands/query conversion/events and clipboard denial have authored controls.
  Full popup/navigation/directive/deep-policy/Reporting API coverage remains open.
- Use private native capture/bubble event/listener/handler state with once/passive/
  signal behavior and cross-realm brands. Blob downloads read authentic bytes.
  Host input captures private drivers and discards timed-out queued work; guest
  setup cannot expose host ops even when iframe bootstrap is interrupted.
- Clone with V8 and genuine native CryptoKey/DOMException/DOMRect records,
  preserving aliases, nonextractable permissions and receiver realms. Transfers
  validate serialization/admission before ownership changes; queue limits are
  host-only4096 snapshots/64MiB. Native weak endpoint graphs, message/task pins
  and retired-frame cleanup have actual ownership/GC regressions.
- Transfer ReadableStream/WritableStream/TransformStream through genuine private
  ports and official raw stream algorithms. Preserve Chrome transfer phase order,
  overlap partial commits, reentrant generation checks, receiver errors and
  hidden endpoints. Public port/stream replacements cannot grant authority;
  abandoned plans/completed transfers collect source realms.
- Use official Deno WebCrypto with genuine key operations and browser admission,
  including actual MLKEM768-X25519 X-Wing composition, private formats/wrapping
  and Chrome interoperability. Reproduce the licensed provider inherited-setter
  fix; native material/permissions never pass through page setters.
- Retain native offline audio DSP, genuine PCM output and licensed source pins.
  Realtime playback, feedback, parameter connections, multichannel/custom waves,
  oversampling and lifecycle expansion remain deferred in issue27.

- Preserve authoritative installed navigation metadata in the native trace driver,
  including same-URL POSTs; prohibit synthesized form fallback on request while
  still following genuine later script redirects. Native host Enter submission
  uses captured canonical form admission and validation instead of manually
  constructing a GET URL. Private branded File/FormData values now retain genuine
  entry lists, conversion/iterator behavior and source-pinned Chrome controls.

- Implement native-backed ElementInternals and CustomStateSet, including genuine
  admission/ARIA/form association/validation/reactions, UTF-16 custom states and live
  state iterators. Preserve native checked/defaultChecked/reset/clone behavior and
  typed :checked matching. The later canonical integration adds real radio grouping,
  canceled-click restoration, adoption and native owner-index lifecycle controls;
  broader form-associated behavior remains on the central checklist.
- Deliver genuine trusted FormData events with shared mutable entry records,
  reentrancy guards, pre-event metadata and post-event wire serialization/CSP
  admission. Disconnected forms and forms removed during submit listeners do not
  submit. Independent controls cover actual HTTP, constructor/brand and GC edges.
- Fix child form-value callback ownership and return fallible native bootstrap
  errors under watchdog interruption instead of aborting the process. Existing
  Fetch/form/host-input lifecycle regressions expose both failures before repair.
- Compile ARM NEON delay code only for aarch64 targets. Actual x86_64/arm64
  compilation, linking and delay PCM checks cover the Linux CI build failure;
  upstream audio kernels/source hashes remain unchanged.
- Select the portable `std::exp2(float)` overload in the authored audio adapter.
  The unqualified call selected double on Linux GCC and float on macOS Clang.
  All 176 cells of the independent Chrome frequency-response control now match
  exactly on Linux x86, macOS ARM and Rosetta x86; no tolerance or licensed-kernel
  change is introduced. Advanced audio remains deferred in issue27.
- Derive navigation activation from private trusted input before pointerdown,
  mousedown and key handlers run; preserve genuine form/Location admission through
  HTTP redirects and clear transient activation for subsequent document-script
  retries. Public UserActivation uses native context state and genuine brands;
  same-origin navigation retains sticky activation while cross-origin navigation
  clears it. The reusable 14-case localhost Chrome oracle covers trusted/untrusted
  input, expiry, Location handlers, redirects, cross-origin state and retries.

- Read genuine macOS accessibility preferences and system appearance for live CSS
  and matching media queries; derive pointer/hover from the installed input driver.
  Include captured host media in the layout cache key. Source-pinned independent
  Chrome controls cover31 queries and real CSS rectangles. Dynamic preference
  events and other platform backends remain tracked in issue28.
- Reflect real form attributes with native conversion and receiver behavior; repair
  input/button prototype placement and text-range editing, storage enumerable flags
  and ValidityState's readonly constructor prototype. Existing Blob members now use
  the measured enumerable flags and key order; absent textStream remains tracked.

- Preserve genuine native form navigation in MCP using host-owned URL, response
  headers/status/referrer and commit count. Keep same-URL POST responses, cookies
  and history without replaying a GET; destination CSP remains effective on later
  renders. Preserve committed state before returning a later guest exception. Remove
  synthetic-form fallback so canceled, invalid and CSP-blocked submissions produce
  no request, while explicit JavaScript navigation remains supported. The full MCP71
  suite covers existing GET and new POST, error, admission and lifecycle controls.
- Regenerate the complete durable API backlog from frozen paired evidence with
  `api-inventory.mjs`, retaining all observed paths as well as member summaries.

- Retain valid typed `@property` and `@view-transition` descriptor blocks in CSSOM
  and private native stylesheet exports. Descriptor rules omit child `cssRules`;
  fresh Chrome shape controls and poisoned-public-getter tests cover both seams.
  Full property registration and transition lifecycle remain tracked in issue28.

- Admit parsed modern at-rules only after a bounded geometry proof: named container
  queries with no eligible authored container declarations, color registrations
  whose transitive variable uses affect only background/color, and navigation-only
  static transition metadata. Unknown rules and geometry-changing dependencies still
  error. Fourteen Chrome cases, actual inline/sheet cache mutations and an offline
  replay of the saved Google declaration graph cover admission and counterexamples.

- Record synchronous interface call durations, cumulative/max time and timed samples
  using captured clocks, including throws. Preserve guest return/Promise/error
  identity and exclude trace serialization/stack work from the measured call span.
- Capture private computed-style selector intrinsics so page overrides do not run
  as internal matching hooks or appear as page-originated interface queries.
- Reuse the resolved layout view for geometry and scroll translation. Batch ordinary
  document selectors through the native query cache; dynamic pseudos and shadow
  selectors retain their existing matching path and scope behavior.
- Use real elapsed waits in the comparison harness while pumping pending native
  callbacks. A delayed-XHR regression verifies that a genuine HttpOnly consent
  cookie reaches the subsequent search request before document replacement.
- Harden private dense arrays in two hash-pinned official Deno event modules
  without changing public Array or the official abort/timer/dependent lifecycle.
  Preserve translated dependent-signal reason identity before event delivery.
  Chrome state/timeout controls and the pre-adapter native phase order are retained;
  the measured stream/listener microtask-order difference remains open in issue28.
- Retain pending timeout signals in their document heap until delivery, releasing
  them before dispatch; cover forced GC and retired-document ownership without
  introducing a native realm root. Listener-count release and detached-window
  active-time suspension remain open.
- Pin the experimental155 SDK async-operation admission transaction and generated
  stubs, with reusable exact-preimage failure, recovery, rejection and all-arity
  controls. Verify source/patch provenance before compilation and retain genuine
  rejected-operation errors. Production stock150 and arbitrary interruption
  reservations remain outside this bounded repair.
- Separate conformance-fixture budgets from queue-capacity checks. Preserve exact
  4096-task admission/recovery assertions and reject watchdog termination as a
  capacity result; give the host-media oracle a conformance budget without
  changing product execution limits or Chrome expectations.

- Add genuine Input/Button submitter action, method, encoding and target reflection,
  preserving measured missing-versus-invalid defaults and Input DOMString versus
  Button USVString action writes. Canonical independent Chrome controls cover
  brands, owner-document/base resolution and hostile public getters.
- Expose Element.customElementRegistry through private native associations and
  preserve unread parser ownership across moves. Rebind global/inert-null registry
  state during actual adoption, including ShadowRoots, without retaining an old
  global registry. Forty-four independent Chrome controls pass in the canonical
  binding; final Surf integration checks remain separate.

### Historical validation checkpoint — source a057a3a

These results describe the earlier source-bound checkpoint; the current follow-up
requires its own complete gates and fresh measurements.

That default workspace passed1,376 tests with three explicit
stock-native callback skips and six generated doctest ignores. The final actual155
six-feature workspace passes1,415 tests with zero failures and six generated
doctest ignores. Both complete commands cover129 component suites. Both engines pass final XHR5, shadow9,
all-target strict Clippy and formatting. Both rebuilt addons passed95 shim tests
(one intentional test-framework skip),20 E2E tests,89 detection controls without
skips and2 Python SDK-helper tests.

The optimized native CLI/addon includes wreq, trust anchors, Metal and CoreText.
That75-interface comparison records64 original/142 expanded missing members
and374 paths. On the same68 previously measured interfaces, missing members fall
134→119 without additions; both formerly absent collection interfaces are present.
Both actual detector captures have no strong findings. The fresh two-repeat
final-source Google matrix completes with all four native features and genuine
consent/native-cookie forms, without sidecar or fallback. Native baselines reach
/search200 then/sorry429:0/2 accepted SERPs. Chrome baselines load9 markers each;
instrumented Chrome is rejected in both repetitions. Native trace.errors is empty
in all four passes, but stdout records reCAPTCHA fetch failures and homepage
hydration still reaches its20s budget. Fresh paired capabilities and shared WebGL
probes match within their measured scope. All four native passes still emit
Google jc/lc stylesheet telemetry before search; Chrome emits neither. The
constant-code diagnostic snapshot records this residual without asserting a
rejection cause.

Source-pinned receipts retain their own measurement dates and commits; newer
VALIDATION-LAST-RUN.json and GOOGLE-LAST-RUN.json captures supersede this historical
checkpoint without changing its recorded results. Broader residual behavior and reliable
native SERP acceptance stay in issue28; advanced audio stays in issue27. See
FINDINGS.md and PARITY-GAPS.md for current evidence and bounds. No turbo-surf
publication tag is created here.

## [0.5.3] — 2026-09-29 — hide internal globals from window fingerprint

### Changed — render fingerprint fidelity
- **turbo-surf's `__*` internal globals no longer sit on `window`.** `Object.getOwnPropertyNames
  (window)` (a bot-enumeration check) previously exposed ~31 turbo-surf helpers (`__runTimers`,
  `__hydrate`, `__makeFrameRealm`, `__importMeta`, …) — real Chrome has none. **30 of 31** are now
  moved onto a **`globalThis[Symbol.for('ts')]` namespace**: a Symbol key, absent from
  `getOwnPropertyNames`, yet reachable from every context (the bootstrap via `__G`, the Rust host +
  injected scripts + child iframe realms via `globalThis[Symbol.for('ts')]`). Covers the
  ENV_BOOTSTRAP internals (render machinery, testctl, the module/frame loader, misc), the
  `import.meta` rewrite target (`__importMeta`), and — via **turbo-test v0.4.4** (the vendored DOM
  binding, re-vendored here) — `__makeFrameRealm` / `__ttEvent` / `__winListeners`.
  - **Not moved (not tells):** `__RESULT` (the public `run_playwright` return API) and `__hi`
    (`human_interact`) are transient — present only during those flows, never during an anti-bot
    page load; `__addGetElems` self-deletes in the binding after use.
  - **Sole residual:** `__name` (esbuild's `keepNames` helper) — transpiled page code references it
    as a free identifier expecting esbuild's module-local `var __name`; the global is a no-op
    fallback, and `__name` on window is ambiguous (a real esbuild helper name), so it's left as-is.
  - Regression test `internal_globals_are_off_window` verifies the moved names are gone from
    `getOwnPropertyNames`/`in window` yet still callable via the namespace.

### Dependencies
- Vendored DOM binding bumped to **turbo-test 0.4.4** (`browser_env.{js,rs}`).

## [0.5.2] — 2026-09-29 — fix crates.io publish (trust-anchors default)

Hotfix for the v0.5.1 crates.io publish, which failed on `turbo-surf-mcp` with
`no field 'trust_anchors' on type '&mut TlsOptions'`.

### Fixed
- **`trust-anchors` removed from `turbo-surf-mcp`'s `default` features.** It relies on
  `TlsOptions::trust_anchors`, a field that exists only in turbo-surf's **vendored** `wreq`
  (`rust/vendor/wreq`), not the crates.io release. `cargo publish` verifies the crate against the
  **registry** `wreq` (path/patch overrides don't apply to a published crate's deps), so a default
  that enabled trust-anchors failed to compile at publish time — even though local + CI builds
  (which use the vendored wreq) passed. The **shipped npm binary still gets trust-anchors**:
  `release.yml` now builds it with `--features trust-anchors` (vendored wreq present in CI). JA4
  fidelity (Chrome 152+ `0xCA34`) is unchanged for the product; only the crate default changed.

### Added
- **Regression guard** (`turbo-surf-mcp/tests/publishability.rs`): asserts no crate `default`
  feature enables a vendored-wreq-only capability (`trust-anchors`), so this publish break can't
  recur. Offline, part of `cargo test`.



## [0.5.1] — 2026-09-29 — Chrome 154 + versioned browser-config registry

Default reported identity bumped **Chrome 153 → 154**, and — the substantive change — all
version-specific fingerprint data moved into a **declarative per-version registry** so the
reported browser is a coherent, swappable set (and adding Chrome 155 later is a one-line
drop-in, no code-digging). Driven by a live header/request diff vs real Chrome 154: header
**set, values, casing, h2 pseudo-order and TLS all matched already**; the one real tell was
the navigation **header ORDER**, which Chrome reordered in 154.

### Added
- **`turbo-surf-core::browser` — versioned Chrome release registry.** One `ChromeRelease`
  literal per version (`major`, `full_version`, `header_order`) in a `RELEASES` table, with
  `DEFAULT_MAJOR` = current stable. `crate::fingerprint` (both `default_profile` and the seed
  pool) and `crate::net` read every version-specific value from here; the render tier receives
  it over `op_fingerprint`. Adding a new Chrome is a single literal + (if its header order
  changed) one `HEADER_ORDER_*` const — no scattered code/doc edits. `release(major)` resolves
  exact/nearest configs (unlisted-newer → newest listed; older-than-listed → oldest/classic).

### Changed — Chrome 154 identity (wire + render, coherent)
- **Navigation header ORDER is now per-version.** Chrome 154 hoists `accept-language` up right
  after `user-agent` (before `accept`); 153 kept `accept` first with `accept-language` late.
  `net::emulate` sets wreq's `OrigHeaderMap` from `profile.header_order` (registry-driven)
  instead of a hardcoded 153 list — closing the one confirmed header tell vs live Chrome 154.
  The 153 order is retained + correct, so switching identities stays coherent.
- **Default identity → Chrome 154** across `default_profile` (UA + `sec-ch-ua`), the render-tier
  navigator, and the seed pool's leading major. UA string stays reduced to `<major>.0.0.0`.
- **High-entropy UA-CH now reports the real full build.** `getHighEntropyValues`
  `fullVersionList` / `uaFullVersion` emit the registry's `full_version` (e.g. `154.0.7258.66`)
  instead of the reduced `<major>.0.0.0` (a prior tell — real Chrome gives the full build here).
- **UA ↔ `userAgentData` coherence.** The render tier now derives `navigator.userAgentData`'s
  major from the actual UA string, so brands can never disagree with `navigator.userAgent`; and
  the google path pushes the wire profile's `chromeMajor`/`fullVersion` into the render navigator
  so both layers report the same version for any pooled profile.

### Notes
- The BoringSSL TLS/HTTP-2 template stayed Chrome 149 (wreq-util), while the
  *reported* version (UA/`sec-ch-ua`/header order) tracked 154. **Corrected 2026-10-10:**
  the former assertion that this template was byte-stable across Chrome releases was
  unsupported; newer upstream profiles change TLS options and signature algorithms.
  The browserless Google `__Secure-ENID` was still rejected in the recorded test.

## [0.5.0] — 2026-09-29 — client-side fingerprint fidelity + browserless anti-bot recon

Closes every **client-side** divergence from real Chrome that a BotGuard-class collector
reads, verified **same-machine/same-IP** against a real headed-Chrome capture. Honest scope:
a **trusted** google `__Secure-ENID` remains out of reach browserless — BotGuard is
**interaction-gated**, VM-interpreted + introspection-heavy (Error-stack + descriptor probes),
and **server-scored**, with real-input / real-GPU / real-audio / live-DOM blockers a headless
V8 isolate cannot satisfy (measured: even real headed Chrome on this exit IP minted no ENID
this run — google appears to have changed the gate). The trusted-token path stays the
real-browser sidecar, now interaction-driven. These land the achievable client parity; they
harden the engine broadly against presence/string/timing/interaction-gated walls.

### Added
- **`probe_mint {url}` MCP tool** + **`probe_page_async`** (render): execution-complete
  in-isolate anti-bot recon — runs a page's own integrity JS to completion (dynamic
  `<script>` injection + `op_fetch` + timers) under instrumented fingerprint globals and
  reports `{shim_needed, accesses, earned_cookies, minted_enid}` (what env the VM demands +
  whether an in-isolate run mints anything, no browser).
- **`TURBO_SURF_TRACE`** — env-gated `serp:` diagnostics across the native-google path
  (cache-vs-mint decision + ENID age, native status/final_url/body_len, the enablejs-shell
  verdict, the one-shot re-mint retry, and `/sorry` detection).
- **`human_interact` MCP tool** — a **generic** page-interaction driver: string together
  `move` / `click` / `focus` / `type` / `blur` / `wait` steps (targeting CSS selectors or
  coords) and play them as realistic, **trusted** input on the current page, then keep the
  hydrated result. Takes inline `steps` or a saved **`routine`** (with `{param}` substitution).
  Ships the **`google-serp`** routine (move→click search box→focus→type query→click Search) — a
  real-user flow to *attempt* the interaction-gated SERP; the driver is general, google is one
  caller.
  - **Follows the resulting navigation.** A fill-a-form-then-submit interaction now records the
    triggered navigation — the page's JS `location` change and, as a fallback, the enclosing
    form's serialized GET target (action + every named field's value, so tokens the page baked
    into the form ride along) — and, when it's a real navigation away from the page, LOADS it
    natively through the session (jar/cookies carried, a `Referer` of the originating page + a
    coherent `sec-fetch-site`), then rehydrates. So "type a query, click Search" actually loads
    the result page instead of stalling on the form. Returns `{navigated, navigated_to}`. General.
  - **Typing now mutates the field value.** The synthesizer sets the focused element's `.value`
    as each character's `input` event fires (real-browser order), so a submitted form carries the
    typed text (previously typing dispatched events only — the field stayed empty).
  - **Full, realistic key-event sequence.** Each typed character now fires
    keydown→keypress→**beforeinput**→input→keyup, and every keyboard event carries the real
    identifiers (`key`/`code`/`keyCode`/`which`, e.g. `KeyR`/`72`) instead of `{key}` only — a
    `{key}`-only event is a tell BotGuard-class collectors read. A new **`press`** step fires a
    named key (e.g. `Enter`) with the correct identifiers (Enter → keyCode 13, code `Enter`); the
    `google-serp` routine now presses Enter to search (how users actually do it) rather than
    clicking the button, so the page's own Enter handler runs and the nav-follow loads the result.
- **Human-input synthesizer** (`turbo_surf_render::HUMAN_INPUT_JS`) backing the tool:
  `__hi.{path,move,type,typePlan,delay,moveAndClick,play,sequence,ev}`. Generates a pointer
  **path A→B sampled the way a browser samples a gesture** — a coordinate every ~12ms (±stddev),
  NOT per-pixel/ms, so speed shows as distance-per-sample. Modes `"straight"` and `"human"`
  (cubic-Bézier curvature + Gaussian coordinate noise + slow→fast→slow velocity + overshoot).
  `sequence()` chains gestures over the virtual event loop, firing the full real-event families:
  pointer/mouse **move** stream, **mouseenter/leave + mouseover/out** on hover changes, a real
  **mousedown → dwell → mouseup → click**, **focus/blur (+focusin/out)** on focus changes, and
  **human-paced keyboard** (keydown→keypress→input→keyup with per-key rhythm, extra gaps after
  space/punctuation, occasional hesitation). A jittered **start delay** (events begin after N±rand
  ms — humans don't fire at t=0). `Event.isTrusted` is now a Chrome-shaped **prototype accessor**;
  synthesized events read `isTrusted:true` (ordinary script events stay `false`) and carry
  `timeStamp` on the hi-res clock. Addresses the interaction-gate + input-entropy signal
  in-isolate (not GPU/audio/server scoring).
- **Raster-backed canvas `toDataURL`** — `turbo-surf-raster::canvas_ops_png` replays the 2D
  display list (`ctx._ops`) into a real PNG (was a ~94-byte synthetic stub — an impossible
  size for rendered content); wired as a host hook (`set_raster_fn`) from mcp + napi.
- **Canvas-2D surface fidelity** (diffed vs real Chrome):
  - **`getImageData` reads the real rendered pixels** (a new raw-RGBA rasterizer path,
    `canvas_ops_rgba` + `op_raster_rgba`), not the vendored synthetic bytes — `fillRect(red);
    getImageData` returned non-red before, a broken/fake-canvas tell. Raw pixels have no PNG
    encoder in the loop, so solids/shapes read back **byte-identical to a real browser**.
  - **`measureText` width** routes through the host system-font measurer (`op_measure_text`, the
    same CoreText face Chrome uses) — matches Chrome **exactly** (`82.3984375` for `16px Arial`,
    was a synthetic ~66; a font-detection tell).
  - Context **class tag** → `[object CanvasRenderingContext2D]` (was `[object DOMImplementation]`),
    and **`getContextAttributes`** added (`{alpha, colorSpace, colorType, desynchronized,
    toneMapping, willReadFrequently}`).
  - Residual: `toDataURL` hash still differs (needs Chrome's Skia+CoreText rasterization AND its
    exact libpng encoder — no shared final layer, unlike WebGL's raw GPU pixels), and glyph
    rasterization pixels differ. `getImageData` (the raw-pixel path fingerprinters use to verify a
    fill) is the one that now matches.

### Changed — page-load lifecycle
- **Real document/window load sequence.** The render tier now drives `document.readyState`
  through `loading → interactive → complete` and fires the matching events **in order** on the
  main document + window: `readystatechange`, `DOMContentLoaded` (at interactive, reaching window
  listeners), then window `load` + `pageshow` (at complete, after DCL). Previously `readyState`
  was a frozen `'complete'` and `DOMContentLoaded`/`load` fired only in child (iframe) realms — so
  a page (or a collector) that gates init on them never ran. Events are trusted + hi-res stamped.
- **Realistic `performance.timing` spread.** The navigation phases are now laid out with plausible
  ordered gaps (`navigationStart < dns < connect < request < response < domInteractive < DCL <
  domComplete < loadEventEnd`) instead of all sharing one timestamp (an all-equal timing was a
  synthetic tell).

### Changed — render fingerprint fidelity (ENV_BOOTSTRAP)
- **WebGL identity → real Chrome/ANGLE-Metal (Apple).** `UNMASKED_VENDOR/RENDERER`, numeric
  limits (`MAX_TEXTURE_SIZE` 16384, …) and the exact `getSupportedExtensions` lists (39 webgl
  / 36 webgl2) — was SwiftShader ("ANGLE (Google, … SwiftShader)"), a headless/VM signal.
- **Native-code masking** for canvas/WebGL context methods — `toDataURL`/`getParameter`/… now
  report `[native code]` (were JS source, an instant anti-tamper flag).
- **Timezone pinning (opt-in)** — set **`TURBO_SURF_TZ`** (e.g. `America/New_York`) for a coherent
  zone; the isolate then reports it via `Intl`/`Date` (ICU `TZ` + per-isolate
  `date_time_configuration_change_notification`). Not set by default: mutating the process `TZ` env
  would race other threads + silently change the host process's timezone when embedded in the napi
  addon / PyO3 wheel. Without the opt-in the isolate uses the host zone.
- **High-resolution clock** — `performance.now()` is origin-relative + fractional + monotonic
  on Chrome's 100µs grid, `timeOrigin` is a fractional epoch anchor, and `requestAnimationFrame`
  is **batched** like Chrome — all callbacks scheduled for one frame fire with the SAME fractional
  origin-relative `DOMHighResTimeStamp` (~16.6ms cadence + jitter, ≤ `now()`), a re-scheduled
  callback runs on the next frame, and it returns an integer id with a correct `cancelAnimationFrame`
  (was: per-callback incrementing timestamp, no arg, `clearTimeout` alias — a tell).
- **Coherent clock — observable time is REAL, timer delays stay virtual.** `performance.now()` and
  `Date.now()` now derive from `page_age + real_monotonic_hrtime + virtual_timer_advance` via a new
  `op_now_perf` op (a real monotonic `Instant`, sub-ms). So wall-clock reads advance like a real
  machine's — `setTimeout(fn, 100)` observes ~100ms elapsed (was **0ms**, a chronometric-trap tell),
  and a busy loop shows real µs drift (the maxIdenticalRun over repeated `performance.now()` fell
  13989 → 1110). `Date.now` is overridden so `timeOrigin + performance.now() == Date.now()` (coherent
  residual 0), passing BotGuard's `perf.now()`-vs-`Date.now()` chronometric cross-check. Timer *delays*
  stay on the virtual clock so the crawler never blocks on a real `setTimeout`. The `Date`
  **constructor** is wrapped too, so `new Date()` / `+new Date()` / `Date()` read the same coherent
  clock (they otherwise diverged from `Date.now()` by the accumulated virtual time — a coherence
  tell); the wrapper preserves statics, `instanceof`, `Date.length===7`, `prototype.constructor`,
  subclassing (`class X extends Date {}` via `Reflect.construct`), and native fn shape (`Date` /
  `Date.now` report `[native code]`, not the JS override source). Covered by
  `clock_reflects_virtual_time_and_is_coherent`.
- **`Object.prototype.toString` brand pollution fixed.** A node-branding pass was tagging
  `document.implementation`'s prototype — which *is* `Object.prototype` — so `Symbol.toStringTag`
  landed on `Object.prototype` and `Object.prototype.toString.call({})` / `[]` / `new Date()` all
  read **`"[object DOMImplementation]"`**, a trivial, high-severity bot tell on *every* un-branded
  object (and it had clobbered `Object.prototype.constructor` too). Branding now targets the instance,
  never a shared root proto: `{}` → `[object Object]`, `[]` → `[object Array]`, `new Date()` →
  `[object Date]` again, with `document.implementation` still correctly branded.
- **UA-CH greased brand** → `"Not_A Brand";v="8"` in the middle slot, matching the on-wire
  `sec-ch-ua`; coherent `getHighEntropyValues.fullVersionList` (was a stale `Not)A;Brand;v=24`).
- **Coherence surfaces** real Chrome exposes that deno_core omits: `navigator.pdfViewerEnabled`,
  a populated `mimeTypes` (PDF types linked to the plugins), `Notification.permission`,
  `performance.memory`, `document.scrollingElement`, `window.sessionStorage`, legacy
  `performance.timing`/`navigation`, and `window.scheduler`.
- **Four structural tells closed, found by a Chrome-vs-turbo-surf snapshot diff** (a shared
  collector run in real headed Chrome on a live google page vs. the render isolate; after the
  fixes the diff is 0 structural fields — the remaining diffs are host display/RAM/CPU config,
  the canvas render-engine hash, and Chrome version drift, none of them tells). A repeatable
  turbo-surf-side runner ships as `turbo-surf-mcp`'s `fp_snapshot` example. The four:
  - **`navigator.permissions` was absent** (`permissions.query` threw `TypeError`). Now a
    spec-shaped Permissions API — `query()` resolves a `PermissionStatus` with realistic default
    states (`notifications` coupled to `Notification.permission`), the method native-masked.
  - **`document.characterSet`** (+ `charset`/`inputEncoding`) returned `undefined` (a vendored
    own-property); forced to `"UTF-8"`.
  - **`window.location` class-tagged as `"[object DOMImplementation]"`** instead of
    `"[object Location]"` — a trivial `Object.prototype.toString.call(location)` bot tell.
  - **A WebGL context was not `instanceof WebGLRenderingContext`** and the constructor prototype
    was a bare stub (`getParameter` threw off the prototype). Each context now gets a dedicated,
    native-marked prototype wired to its named global ctor — *without* reusing the shared vendored
    fallback proto (which would make `location instanceof WebGLRenderingContext` true and collapse
    the webgl/webgl2 prototypes onto one, both worse tells). Regression-tested against Chrome's
    captured values (`fingerprint_parity_permissions_location_charset_webgl`).
- **Physically coherent window/screen geometry.** The viewport is now shorter than the window
  (`outerHeight = innerHeight + browser chrome`, ~88px) — equal inner/outer height was impossible
  for a real window and a tell; widths stay equal (no side chrome); `availHeight < screen height`
  (menubar). Covered by `window_and_screen_geometry_is_coherent`.
- **macOS-coherent color depth.** `screen.colorDepth`/`pixelDepth` default to **30** on a Mac
  profile (wide-gamut/HDR displays) and 24 elsewhere — was a flat 24 while claiming an Apple GPU.
- **All viewport/display fields are param-driven** (`__pick`) with platform-aware sensible
  defaults: `innerWidth`/`innerHeight`/`outerWidth`/`outerHeight`, `screen` dims,
  `availWidth`/`availHeight`, `colorDepth`/`pixelDepth`, `deviceMemory`, `hardwareConcurrency`,
  `languages` — a caller passes a machine profile; nothing is hardcoded to one host.
- **Real-Chrome UA-CH GREASE brand.** `navigator.userAgentData.brands` and the on-wire
  `sec-ch-ua` now both emit `"Chromium";v="M", "Google Chrome";v="M", "Not A(Brand";v="99"`
  (greased brand LAST, token `Not A(Brand` v99), validated against a live capture and unified
  across the two layers. Previously the wire (`Not_A Brand`/`8`) and render tiers disagreed and
  neither matched real Chrome — a cross-layer mismatch is a hard tell. (The UA *string* itself was
  already a correct reduced Chrome UA — verified character-identical to real Chrome bar the major.)

### Changed — BotGuard/SearchGuard anti-tamper hardening
Driven by a committed **integrity-trap probe** (`scripts/browser-sidecar/probes/botguard-probe.js`,
run in real Chrome via `run-probe.mjs` and in the isolate via the `fp_snapshot` example, then
diffed) against the checks the research surfaced (native-fn shape, toString anti-hook, chronometric
trap, env surface). Closes the tractable traps (and, with the `gpu-metal` WebGL bridge, the WebGL
render-hash — now pixel-identical to Chrome, see the GPU section); the residual (the Canvas-2D
render hash + the BotGuard VM's live execution-integrity checks) needs a real browser and is out of
reach for a synthetic DOM — documented.
- **Native-fn SHAPE.** A real native function reports `[native code]` AND has **no own
  `prototype`** (a stealth-detection tell: `'prototype' in HTMLCanvasElement.prototype.toDataURL`
  is `false` in Chrome). Our `function`-expression shims carried an own, non-deletable prototype.
  The toString trap is now a concise method (no prototype); a `nativize` forwarder (prototype-less,
  name/length pinned) backs the canvas/WebGL/event shims; `addEventListener`/`removeEventListener`/
  `dispatchEvent` are native-shaped on their own object. A final sweep native-marks every remaining
  window data-property function, so no window global leaks JS source via toString.
- **`navigator` is a real `Navigator` instance.** Was a plain object literal (26 own props + a thin
  15-member prototype); now `window.Navigator` + an **84-member `Navigator.prototype`** (the exact
  member list captured from live Chrome — 56 getters + 27 methods + constructor) with
  `navigator = Object.create(navProto)` → **zero own properties**, `instanceof Navigator`,
  `[object Navigator]`, and native-marked prototype getters (incl. a `webdriver` getter, no own
  prop). Real values where we have them; plausible object stubs otherwise (e.g. `scheduling.
  isInputPending` for React). Removed the old `hostInterface` re-parenting that produced a 13-getter
  stub prototype.
- **Full window surface breadth.** A window own-prop diff vs real Chrome found ~740 missing globals;
  added them — 554 interface constructors (stub fns), 115 `on*` handler slots (null, like Chrome),
  and ~70 misc methods/props/bar objects — taking the own-prop count from **527 → ~1263** (Chrome
  1235), so `X in window` / `typeof window.X` / `window.onX === null` presence checks pass.
- **Coverage:** `masked_native_functions_have_no_prototype`, `navigator_is_a_prototype_backed_instance`,
  `window_surface_matches_chrome_breadth` (render-tier regression tests).

### Added — GPU (opt-in)
- **`gpu-metal` cargo feature** (off by default; macOS/Metal) — real Apple-GPU rendering via
  `wgpu`→Metal for the render tier's fingerprint surface, fixing the coherence tell of claiming
  an Apple-Metal renderer while producing device-invariant/software pixels. Two paths:
  - **Canvas 2D** — `toDataURL` rasterizes on the GPU instead of tiny-skia (falls back on any
    GPU error, so it never regresses).
  - **WebGL** — a **live WebGL→wgpu bridge**: the render tier overrides the isolate's WebGL
    methods to record a page's `gl.*` calls *at the live call site* (real buffer bytes + shader
    source — nothing lossy), then executes the batch on the real GPU (`naga` translates the
    page's GLSL-ES → WGSL) and returns genuine framebuffer pixels for `readPixels`. Synthetic
    fallback when no GPU backend is installed.
  A broad fidelity win for **any** wall that hashes canvas/WebGL (DataDome, Kasada, reCAPTCHA,
  Akamai, Cloudflare, Incapsula), not just google. Zero cost to the default build / PyPI wheels
  (0 wgpu/naga crates unless the feature is on); enabled via `turbo-surf-mcp`/`napi`/`py`'s own
  `gpu-metal` pass-through. **Live-measured: the WebGL fingerprint now matches real Chrome
  pixel-for-pixel** — a standard gradient-triangle draw + `readPixels` produces a byte-identical
  buffer and the SAME pixel hash as Chrome on the same Apple GPU (wgpu→Metal lands on the same
  result as ANGLE→Metal for these scenes). Two bugs fixed to get there: WebGL contexts were missing
  the ~298 enum constants (so `gl.VERTEX_SHADER` etc. were undefined → real draws got `undefined`
  args → clear-only), and the GLSL rewrite was line-based so a minified single-line shader (all
  declarations + `main` on one line, as real fingerprint shaders are) was corrupted → translation
  failed → synthetic fallback. Now: full constant set on the context prototype + a brace-aware
  statement splitter. So this beats device-invariance/SwiftShader **and** exact-hash comparison for
  the common WebGL fingerprint scenes. (Live-measured: even the full kitchen-sink — GPU + interaction
  + parity — does not earn a trusted google `__Secure-ENID`; that residual is server-side attestation.)
- **`coretext` cargo feature** (off by default; macOS only) — canvas **text** glyphs raster through
  CoreText/CoreGraphics (the same system glyph stack Chrome's Skia uses on macOS) instead of the
  ttf-parser + tiny-skia vector glyph trace, so `getImageData` over rendered text reads back like a
  real Mac Chrome. OS-guarded twice: `#[cfg(all(target_os = "macos", feature = "coretext"))]` on the
  code path AND macOS-only optional `core-graphics`/`core-text`/`core-foundation` Cargo deps — the
  frameworks don't exist elsewhere, so nothing to build off-macOS. Only text is swapped (shapes stay
  on tiny-skia, no regression); handles an identity/translate CTM and falls back to the vector trace
  for a scaled/rotated text matrix. **Live-measured vs real Chrome**: `getImageData` over "Hello"
  20px Arial → bbox `[3,13,46,28]` + avgAlpha 163, matching Chrome's bbox `[3,13,46,28]` + avgAlpha
  163 (pixel count within 2%). Enabled via `turbo-surf-mcp`/`napi`/`py`'s `coretext` pass-through;
  zero cost to the default build. Attribute ranges are counted in UTF-16 code units (not scalars),
  so an astral char (emoji, CJK-ext) styles its whole glyph run; `set_should_smooth_fonts(true)` is
  the measured Chrome-parity choice (avgAlpha 163.3 vs Chrome 163; grayscale/`false` drifts to
  147.3). (Canvas-2D `toDataURL` byte-parity remains intentionally out of scope — PNG-container/
  encoder differences, not a glyph-shape tell.) Covered by `coretext_rasterizes_text_into_pixmap`
  + `coretext_declines_non_translate_ctm`.

### Added — per-strategy config (data-driven, not google-hardcoded)
- The `structural` result extractor's engine-internal host filter is now a per-strategy
  **`internal_hosts`** list (`.`-prefixed = substring, else host suffix; empty = no filtering)
  instead of a google-hardcoded blocklist baked into the shared extractor — so `structural` is a
  general format, not secretly google-only. The google list moved to `search-strategies.json`.
- Search strategies (`search-strategies.json`) gain an optional **`headers`** map, applied to the
  SERP fetch and merged over the impersonate/rustls defaults — a general, config-driven knob (not
  google-hardcoded). The google strategy uses it to send `Referer: https://www.google.com/` +
  `sec-fetch-site: same-origin` (overriding the emulation's cold-nav `sec-fetch-site: none` / no
  referer) so the `/search` fetch looks like a real in-site navigation, and its `query_url` carries
  `&source=hp`. Measured vs a real incognito Chrome `/search` (which returns the real SERP on this
  same IP — the IP is not the wall), a missing `Referer` + `sec-fetch-site: none` + a bare `?q=`
  were the top "direct/scripted fetch" tells. Honest note: these close real tells but aren't a
  standalone SERP unlock. The residual is a single upstream gate — **trusted homepage delivery**.
  An untrusted request is served the `enablejs` shell; a *trusted* one (valid `__Secure-ENID`,
  real-browser mint) gets the full homepage with the session params `ei`/`iflsig`/`sxsrf` already
  baked into the HTML. Those params are **server-minted, not client-computed** — not a second,
  independent blocker but a symptom of the same gate: solve trusted delivery and they come along
  for free (just parse them out of the trusted HTML).
- **Browserless SERP diagnostic** (`browserless_google_serp`) + a reusable real-Chrome **network
  tracer** (`scripts/browser-sidecar/net-trace.mjs`) narrowed the wall. A net-trace of real Chrome
  shows the cookie set is built by the **consent Accept-all handshake** (`consent.google.com/save`
  → `NID`/`SOCS`/`__Secure-BUCKET`) + the homepage response (`AEC`/`__Secure-ENID`) — never by a
  bare GET. `browserless_google_serp` now does the REAL consent handshake browserlessly and earns
  the **same cookie set as real Chrome** (`AEC`, `NID`, `__Secure-BUCKET`, `__Secure-ENID`,
  `SEARCH_SAMESITE`) — yet `/search` **still returns the `enablejs` shell**. So the trust is **not
  in the cookies** (identical set, still untrusted): it's the `__Secure-ENID` *value*, which a real
  browser's homepage load earns and a browserless mint does not. NB: the `/gen_204` beacons are
  **client telemetry, not attestation** — their params are all CSI/latency/interaction metrics
  (`atyp=csi`, `rt=…`, `mem=…`, `net=…`, `fid=…`), no token blob (verified with the tracer's
  POST-body/query dump). The exact server-side mechanism that blesses a real browser's ENID is
  still open — the headed-sidecar mint remains the only path to a trusted token.
- **Param-driven `SOCS` consent seeding.** The synthetic `SOCS` cookie that dismisses google's
  "before you continue" interstitial was a single hardcoded value; it is now caller-driven via
  `FetchOptions.consent_socs` (core) + the `set_consent_socs` MCP tool / `Session.consent_socs`.
  `None` keeps the built-in default (`consent::DEFAULT_SOCS`); a value seeds a caller-chosen consent
  state; **`""` seeds NO `SOCS`**, so even with bypass on the raw consent-gated response is served
  and the *real* `consent.google.com/save` Accept-all handshake runs instead of the shortcut. Used
  to test whether skipping the consent guard was the trust blocker: measured live both ways
  (default synthetic SOCS vs. `""` → real handshake) — **both return the `enablejs` shell**, so the
  consent shortcut is NOT the gate; the block stays the server-scored `__Secure-ENID` value.

### Changed — general iframes (render)
- **Any** iframe (created, parsed from HTML, or `srcdoc`) now instantiates a real bridged child
  realm that runs its own inline + `<script src>` scripts, with a **distinct** `contentWindow`/
  `contentDocument` (was `=== window`/`=== document`, a hard bot-tell), a wired frame tree
  (`window.frames`/`window.length`/`frames[i]`, child `top` = real top, `parent` = immediate
  parent), and a **depth cap (12)** against frame-bombs. Was: only reCAPTCHA bframe/anchor URLs
  got a realm; every other iframe was an inert stub whose scripts never ran. The reCAPTCHA
  handshake still works (now via the same generic loader). Honest limit: still one V8 isolate, so
  frames share prototypes (no true origin isolation).

### Changed — messaging APIs (render)
- `MessagePort`/`MessageChannel` are now real classes (ports are `MessagePort` instances with a
  working `addEventListener('message')` + `start()`, not object-literal stubs), `MessageEvent` is
  a real constructor (`new MessageEvent('message',{data}).data` was `undefined`), and
  `window.postMessage`/port/`BroadcastChannel` dispatch real `MessageEvent` instances. All four
  message constructors are native-marked (`.toString()` was leaking JS source). The React/google
  `new MessageChannel; port1.onmessage=…; port2.postMessage(0)` scheduler idiom still fires.

### Changed — observers (render)
- `IntersectionObserver` / `ResizeObserver` / `MutationObserver` are now three **distinct**
  constructors (were one shared object — `IntersectionObserver === ResizeObserver` was a trivial
  `===` / `.name` fingerprint tell), `PerformanceObserver.supportedEntryTypes` returns a populated
  Chrome list (was `[]`), and `IntersectionObserver` fires one initial async entry per observed
  element (`isIntersecting:false`, like Chrome for an off-screen node) so visibility-gated init runs.

### Changed — sidecar + deps
- **Browser sidecar mint** (`fetch-serp.mjs`): BotGuard is interaction-gated, so `mintEnid`
  now performs a genuine human-paced search-box interaction to trigger the VM before harvesting
  cookies (the old idle homepage dwell earned no trusted `__Secure-ENID`).
- **deps:** turbo-dom `0.4.0` → `0.5.1` across all six consumer crates.

### Fixed — hardening (3 rounds of adversarial review)
Three review passes over the new render/mcp/raster surface, all findings fixed + covered:
- **`toDataURL` honours a non-PNG MIME** (the `image/webp`/`image/jpeg` support probe) instead of
  relabelling a raster PNG — returning PNG bytes for a webp request was both wrong and a tell.
- **`getContext` is idempotent** — a repeat call no longer re-wraps the WebGL
  `getParameter`/`readPixels` overrides (double-recording / recursion).
- **`IntersectionObserver.disconnect()` before the async initial entry cancels it** (a stale
  callback after disconnect was wrong + a behavioural tell).
- **Canvas `rgba()` accepts percentage alpha + the CSS4 `rgb(r g b / a)` slash form**; the `font`
  shorthand parser tolerates whitespace runs (a double space no longer desyncs the family list).
- **Interaction routines error on an unresolved `{param}` placeholder** instead of typing a literal
  `"{query}"` into a search box.

## [0.4.4] — client-hint parity + in-isolate reCAPTCHA execution

Fingerprint-parity + browserless reCAPTCHA/BotGuard execution work. Note: this does
**not** unblock google search — google `/search` is served an enablejs shell at
request #1 and `/sorry` is IP-reputation-gated (hits real Chrome too), and a
reCAPTCHA token is Google-server-verified. These land the client-side capability +
fingerprint fidelity; the residual walls are IP reputation and server-side scoring.

### Added
- **Full Chrome-153 client-hint parity on the impersonate wire** — the complete
  `sec-ch-ua-*` family (`arch`/`bitness`/`wow64`/`model`/`form-factors`/
  `platform-version`/`full-version`/`full-version-list`), Apple-Silicon-coherent, plus
  the corrected greased brand token (`"Not_A Brand";v="8"`). wreq's Chrome149
  emulation only sent the low-entropy trio.
- **In-isolate reCAPTCHA/BotGuard execution** in the V8 render tier: the reCAPTCHA
  main-frame VM now runs to completion (`window.postMessage` + host-protocol shims:
  `___grecaptcha_cfg`, on* slots, `contentType`, `trustedTypes`); a **bridged
  bframe cross-origin iframe** (second window realm, `contentWindow`/`contentDocument`)
  with two-way cross-realm `postMessage`; a **network handshake** that fetches
  `api2/anchor`+`bframe` over `op_fetch` and runs the bframe VM in the child realm
  (`grecaptcha.execute()` resolves with a client token); a coherent **SwiftShader
  WebGL** context (identity strings + limits + extensions + deterministic readback,
  was null); content-dependent **canvas2d** readback; and a much-extended `probe`
  recon surface (window/WebGL/performance/Date/Intl/MessageChannel/`toString`).
- **reCAPTCHA solver** in the challenge framework: `Vendor::Recaptcha` + `detect()`
  (google `/sorry`, "unusual traffic", or a `g-recaptcha`/`grecaptcha.render` widget →
  lifts sitekey + v3 action), a `RecaptchaSolver` that drives the in-isolate flow to
  mint a `g-recaptcha-response` token, `TURBO_SURF_SOLVER=recaptcha`, and a
  `solve_recaptcha {url, sitekey?, action?} -> {token}` MCP tool. Clears v3/invisible/
  score flows and google `/sorry` **only with a clean `TURBO_SURF_PROXY` IP** (token is
  server-scored); v2 visual image-grid is refused (`SolveError::VisualChallenge`) to
  route to an external solver.
- **Google `/sorry` → SERP clearance loop**: parses the `/sorry` reCAPTCHA form,
  submits a solved token (`g-recaptcha-response`) to `/sorry/index`, captures the
  `GOOGLE_ABUSE_EXEMPTION` cookie, and re-fetches the original SERP with it — wired
  into the `web_search` native path. Token-source-agnostic (in-isolate v3/invisible,
  or an external solver for v2 image-grid). Loop is offline-tested; live `/sorry` is
  usually a v2 image challenge needing an external solver for the token.
- **Browser sidecar: `headless` is a per-call choice** (stdin `headless` >
  `TURBO_SURF_SIDECAR_HEADLESS` env > **headed default**) and a `headless?` arg on
  `web_search`. Measured: headless Chrome trips google's `/sorry` (an automation tell,
  not the IP — headed on the same IP returns the real SERP), so headed is the default.
- **Real EU consent-save handshake → earns google `NID`** (the native `/search`
  unlock). The "Before you continue" interstitial's **Accept all** POSTs to
  `consent.google.com/save?…escs=<token>` and *that* 302 `Set-Cookie`s `NID` (+ SOCS/
  STRP) — the trusted-session cookie `/search` requires. `consent::handshake_if_consent`
  now parses the interstitial, replays the accept form, and ingests NID (previously we
  only hard-coded a synthetic `SOCS` that *visually* dismissed the wall but never earned
  the session — which is why the homepage rendered but `/search` stayed on enablejs).
  Proven offline: enablejs shell → real SERP once NID is in the jar.
- **Fix: `Set-Cookie` dropped on redirect hops.** `fetch_html` auto-follow ingested
  cookies only on the final response, silently dropping the `NID` set on the consent
  302; the consent handshake + render-tier `op_fetch` now follow redirects manually so
  session cookies earned mid-redirect land in the jar (browser-equivalent).
- **Native google search via a reused, sidecar-minted `__Secure-ENID`.** google's
  `/search` serves the real SERP to a plain **native (no-browser)** wreq request that
  carries a *trusted* `__Secure-ENID` — that cookie alone is sufficient, it's long-lived
  (~2027) and client-agnostic. A trusted token is minted only by a real browser's homepage
  load, so the sidecar mints it **rarely** (new `{"mint":true}` stdin mode → `{"cookies":…}`)
  and the engine reuses it across many native searches, persisted to a gitignored
  `.enid-cache.json`. google's strategy is now `mode:"enid"` (was `"browser"`): fetch
  `/search` natively with the cached token; on the `enablejs` shell (a stale/rotated token —
  no `#rso`/`<h3>`) auto re-mint **exactly once** + retry. `web_search { remint:true }` (or
  `TURBO_SURF_ENID_REMINT=1`) forces a fresh mint; `browser:true` still routes the whole SERP
  through the browser. Reuse holds only while the ENID stays trusted **and** on a non-flagged
  exit IP (a `/sorry`'d IP won't serve the SERP to any token — orthogonal, handled by the
  `/sorry` clearance path); it does **not** defeat IP-reputation blocking.
- Requires turbo-test browser_env ≥ 0.4.2 (vendored) for the iframe + WebGL + canvas.

## [0.4.3] — Chrome 153 fingerprint

Freshened the emulated browser identity from Chrome 149 to **Chrome 153** (current
stable) across every layer an anti-bot wall reads, and fixed a latent cross-layer
version mismatch on the default impersonate path.

### Added
- **`trust-anchors` cargo feature** (off by default; implies `impersonate`) — emits
  Chrome 152+'s `trust_anchors` TLS extension (codepoint `0xCA34`), taking the
  ClientHello JA4 from `t13d1516h2` to `t13d1517h2` to match current Chrome. Built on
  a small **vendored fork of `wreq`** under `rust/vendor/wreq` (adds a `trust_anchors`
  field to `TlsOptions`, wired to BoringSSL's `SSL_CTX_set1_requested_trust_anchors`),
  applied via `[patch.crates-io]`. An empty list still emits the extension. See the
  caveat in `PUBLISHING.md` — the `[patch]` is workspace-local, so the feature works
  in the shipped binary but not for crates.io library consumers who enable it.
- **Render tier: real `navigator.sendBeacon` delivery + a complete `XMLHttpRequest`
  surface** (responseType, response headers, readyState transitions/constants, abort/
  timeout/withCredentials) over the isolate's `op_fetch` + shared cookie jar, in the
  non-vendored `ENV_BOOTSTRAP` injection layer (native-branded via the `toString`
  WeakSet). Any page whose anti-bot/analytics flow ships a token via `sendBeacon` or a
  fuller XHR now completes the round-trip. (Note: this does **not** unblock google
  search — its SERP is BotGuard/JS-gated and stays on the real-Chrome sidecar.)

### Changed
- **Request-#1 network-fingerprint parity with real Chrome** (the layer served
  before any JS): the impersonate HEADERS frame is now byte-for-byte Chrome —
  H2 priority weight 256 (was 220), `upgrade-insecure-requests` + `sec-fetch-user`
  added, exact Chrome navigation header ORDER (via `orig_headers`), `accept-encoding`
  = `gzip, deflate, br, zstd`, and the high-entropy `sec-ch-ua-*` hints are **no longer
  sent on a cold request** (real Chrome sends only the low-entropy trio until the
  server sends `Accept-CH`). The impersonate e2e asserts these against a live echo.
- **Emulated Chrome 149 → 153**: `fingerprint::default_profile` (UA + `sec-ch-ua`),
  the profile major pool, and the render-tier `navigator.userAgent` / `chromeMajor`
  now report Chrome 153.

### Fixed
- **Impersonate wire UA was silently stuck at Chrome 149.** Under `--features
  impersonate` (the default), wreq's bundled emulation owns the request headers and
  kept advertising Chrome 149 on the wire even after the rest of the stack moved to
  153 — a cross-layer version mismatch that is itself a bot tell. `emulate()` now
  pins the on-wire UA + `sec-ch-ua` to the same Chrome 153 identity, while wreq still
  owns the header order and the Chrome 149 TLS/HTTP-2 template. **Corrected 2026-10-10:**
  the live echo checked specific header, priority and JA4 properties; it did not establish
  byte-for-byte Chrome 153 parity. The newer upstream profiles have different TLS settings.

## [0.4.2] — screenshot render fidelity

Raster-tier fixes so synthetic screenshots of real, JS-heavy sites match a browser
(validated against a Chromium oracle on google.com / nike.com / en.wikipedia.org).
All via the no-Chromium hydrate + paint path.

### Fixed
- **`<noscript>` CSS was being applied** — Google ships `<noscript><style>…div{display:none}…`,
  which a JS-enabled browser ignores. `lay_out()` now strips `<noscript>` before the
  cascade, so a hydrated page's real content isn't hidden (Google's search UI was
  dropped to a near-blank body).
- **`>` inside a quoted attribute truncated `<img>` extraction** — a naive `.find('>')`
  matched the `>` inside `alt="PLAY MIND<br>GAMES"`, so responsive heroes whose real
  URL lives in later attributes (`data-landscape-url`, `srcset`) were never fetched or
  painted (Nike hero = gray box). Image-tag scanning is now quote-aware.
- **Wikipedia/`:is()` infoboxes restored** via **turbo-html2pdf-core 0.3.2**: the layout
  engine dropped every `<table>` because a selector like `:is(p,table,thead + tbody)`
  in a collapse sheet was comma-split into a bare `table{display:none}`. Bumped the
  raster dep to consume the fix.

### Added
- Responsive images pick the **largest** `srcset`/`<picture>` candidate for a sharper
  paint.
- **`delazyImages`** exposed over napi — de-lazy `data-src`/lazy `<img>`s before
  screenshotting.

## [0.4.1] — render-isolate stealth surface

Make the browserless V8+rtdom render isolate present a native-fidelity, passive
browser surface so a deep fingerprinter (Botguard / reCAPTCHA-class) reads it as
real Chrome. Battle-tested against the botto detector: **21 tells → 0**
(`fake=false`, suspicion 0.296 → 0.000). Additive — no change to existing tools or
the no-JS default tier.

### Added
- **`harness/botto-detect/`** — an oracle harness that runs botto's real in-page
  collector (probe + feature-probe + Botguard integrity VM) inside the live isolate
  and reports its fake/suspicion/tell verdict. Skips cleanly without `../botto` or
  the napi addon.
- **`op_measure_text` + `set_measure_fn`** (render) — a host-injected text measurer
  (napi/mcp back it with `turbo-surf-raster`'s real font engine, system fonts on),
  so the isolate's new `offsetWidth`/`offsetHeight` report per-font-family advance
  widths a no-layout DOM lacks.
- **`turbo_surf_raster::measure_text`** — advance width + line height for a CSS
  `font-family` list at a size, over real font metrics.

### Fixed — anti-detect surface
- Host-interface prototypes (Navigator/Screen/HTMLDocument) with native accessor
  getters; WebIDL brands via `Symbol.toStringTag` (incl. a computed brand on the
  shared rtdom node prototype); per-tag element brands + native-marked methods;
  missing host constructors; a non-silent `OfflineAudioContext`; `CSS.supports`
  rejecting vendor-prefixed Gecko/WebKit props; Chrome-desktop feature markers; and
  a real-global property-count pad.
- **`getComputedStyle`** now resolves percentage `width`/`height` to used px against
  the containing block (via turbo-test `browser_env` @ 52de5c4, re-vendored), so a
  layout-sensitive read returns real px instead of `"50%"`.

### Notes
- Passive/consistency surface only — the active render tier (real canvas/WebGL
  pixels) stays honest. botto is FP-safe-by-design, so this defeats it and the
  commodity tier; a hardened detector may still catch individual shims.

## [0.4.0]

Programmatic fetch + web-search tooling for agent consumers (the `fetch` knowledge
terminal), a hardened-browser search sidecar that clears google's BotGuard, render-tier
fidelity fixes, and the turbo-html2pdf-core **0.3.1** layout engine. Additive — no change
to existing tools' behaviour or the no-JS default tier.

### Added — fetch/markdown
- **`fetch_markdown`** — one-shot `{ url, mode?, links? }` → `{ url, title, status,
  markdown, links? }`. Composes `goto`+`markdown` so a caller needn't drive a stateful
  session for the common "URL → clean markdown" case.
- **`fetch_markdown_batch`** — concurrent, order-preserving multi-URL markdown;
  per-url failures captured, never abort the batch. Self-contained no-JS loop; does not
  touch `batch`/`crawl::Nav`.

### Added — web search
- **`web_search`** `{ query, engine?, base?, limit?(10), browser? }` → `[{ title, url,
  snippet? }]`. Stateless (never mutates the session page/jar). Engine precedence:
  explicit `engine` → session default (`web_search_set_engine`) → `duckduckgo`.
- **`web_search_set_engine`** — session-default engine switcher.
- **Data-driven, versioned search strategies** — engine parse rules are JSON data (not
  compiled selectors), a layered registry: `web_search_load_strategy` override → user dir
  (`TURBO_SURF_SEARCH_STRATEGIES`) → bundled `search-strategies.json`. Plus
  `web_search_strategies` (list) / `web_search_reset_strategy` (drop). Bundled: duckduckgo
  (reliable default), bing, google, searxng, baidu.
- **Real-browser SERP sidecar** (`web_search { browser:true }`; google is `mode:"browser"`)
  — engines behind a browser-integrity wall (google BotGuard/`enablejs`) serve no results
  to any headless fetch. The sidecar shells to a real browser (chromium stays OUT of the
  engine, over `TURBO_SURF_BROWSER_FETCH_CMD`, JSON contract) and parses its rendered
  results. Informed by our own `botto` detector: the tell is the CDP `Runtime.enable`
  leak, so the committed sidecar uses **patchright** (CDP-hardened) + a persistent context
  on real Chrome (works headless), and does zero surface tampering. A `/sorry` captcha
  surfaces as a clear error, not empty results.
- **`web_search_setup_browser`** MCP action + committed `scripts/browser-sidecar/`
  (`fetch-serp.mjs`, `setup.sh`, README) — one-command setup; runtime (node_modules /
  Chrome profile) gitignored.
- **Classname-free structural google parser** (`format:"structural"`) — google's result
  classes are obfuscated + rotate, so results are extracted by the stable STRUCTURE
  (`<a href="http…">` containing an `<h3>`, off-site, deduped). Verified: real organic
  results end-to-end.

### Added / Fixed — render tier
- **Per-script isolation** — the classic render path ran the whole page-script bundle as
  one `execute_script`, so one throw aborted every later script (google's `_._DumpException`
  killed hydration). Now each `<script>` runs as its own top-level program (errors isolate;
  cross-script `var`/`let`/`function` still shared) — a browser-faithful hydration fix for
  all JS sites.
- **`navigator.sendBeacon` shim** — a real-Chrome API whose absence is a headless tell.
- **Raster delazy fix** — a lazy-load placeholder/spacer `<img src>` (1×1/`data:`) now
  yields to the real `data-*` URL, so lazy images (nike) are fetched + painted instead of
  a grey placeholder.

### Changed
- **`impersonate` (real Chrome TLS/JA3/JA4 + HTTP-2 fingerprint) is now ON by default.**
  The shipped MCP binary + npm prebuilds present a genuine Chrome network fingerprint out
  of the box (anti-bot for Cloudflare/Akamai/TLS-gated sites). It's the BoringSSL (`wreq`)
  backend — building from source now needs a C toolchain (cmake/nasm); the CI/release
  workflows install it, and prebuilt binaries need nothing. Disable with
  `--no-default-features`. (Note: impersonate is *not* the google answer — BotGuard needs
  the browser sidecar above; impersonate covers TLS/HTTP-fingerprint gates.)
- Consumes **turbo-html2pdf-core 0.3.1** — the layout conformance engine (68→73
  Chromium-verified fixtures, border-collapse, `<br>` inline forced-break, sub/sup),
  replacing the 0.3.6 line's 0.2.14.

## [0.3.6]

Consumes turbo-html2pdf-core 0.2.14 — real-site rendering round 2 (nike.com
editorial cards / google.com search bar): `calc(% ± px)` lengths, `aspect-ratio`,
flow-relative margins/padding (`*-inline-start`), absolute overlay `%`-height/`bottom`
deferral against an auto-height card, height-based vertical insets, and static-start
placement of auto-inset absolute flex children (Google's "AI Mode" icon). See
turbo-html2pdf's CHANGELOG for the full list.

## [0.3.5]

Consumes turbo-html2pdf-core 0.2.13 — a batch of real-site rendering fixes
(google.com / nike.com home pages): CSS Grid `justify-items` + `grid-column:span`
placement, `box-sizing:inherit` reaching the page, border-box height, `.ttc` face
index, `align-self`/min-width, lazy `<img>` data-urls, and text max-content
rounding. See turbo-html2pdf's CHANGELOG for the full list.

### Fixed
- **Lazy images render.** The delazy pass (which promotes `data-*-url`/`srcset` to
  `src`) now also injects `opacity:1;visibility:visible` so an assume-loaded image
  isn't dropped as `opacity:0` — nike's hero `<img>`s start `opacity:0` and had
  rendered as a blank band.
- **Glyphs trace from the right `.ttc` sub-font.** The raster parses the font face at
  its collection index (from `FontFace::index()`), so text in a system Arial/Helvetica
  collection no longer renders shifted.

## [0.3.4]

Consumes turbo-html2pdf-core 0.2.7 (mask-image icons, `white-space`, data-URI
values, pseudo-element/self-ref-var/flex/table fixes, flex perf) so complex pages
render like Chromium.

### Added
- **CSS `mask-image` icon painting.** The raster stencils a box's tint colour
  through a mask SVG's alpha (`tint_pixmap`), so monochrome UI glyphs (Wikipedia's
  language/menu/edit icons, Codex icon fonts) render as their shape instead of a
  solid square. A missing mask paints nothing.
- **`border-radius` rounded rects/borders** (rounded radio/checkbox circles, cards).

### Fixed
- Wikipedia renders faithfully end-to-end: title bar no longer overlaps the tabs,
  taxobox montage contained + classification aligned, full-width header, page-action
  tabs on one line, blue links, round radios.

## [0.3.3]

Hydration reach + screenshot fidelity. Consumes turbo-html2pdf 0.2.6 (real-page
layout/cascade: `var()`, sibling/structural selectors, inline-block-in-line,
grid-template shorthand, sr-only hide, static-position absolutes, float text-wrap,
`@media(...)` no-space). (0.3.2 was staged but never released; 0.3.3 supersedes it
and carries everything below.)

### Added
- **Broader JS hydration** in the render tier so real page scripts reach a
  browser-faithful DOM: `document.implementation.createHTMLDocument` (jQuery init),
  `Node.prototype.replaceChild`, `PerformanceObserver`, and DOM interface
  constructors (`HTMLElement`/`Node`/`Element`/… with duck-typed `instanceof`) so
  React/emotion `instanceof` checks resolve instead of aborting the bundle.
  Combined, MediaWiki's startup now runs (sets `client-js` + Vector layout
  classes), so Wikipedia hydrates with its taxobox + images.
- **Lazy / responsive image recovery** (`delazy_images` + `image_urls`): pulls the
  URL from `data-src`/`srcset`/`data-srcset`/`data-original`/`data-*-url` when
  `src` is absent, and fills a missing `src` before layout so the box + pixels
  render. Nike-style `data-landscape-url` images now paint.
- **Opt-in system-font screenshots** (`system_fonts` on the raster/napi assets
  entries): resolve a page's fonts against the machine's installed fonts to match
  a browser on the same host.

### Fixed
- **Resilient hydration**: a mid-hydration JS error now returns the partially
  hydrated DOM instead of discarding everything — partial hydration beats none.
- **HTML-entity-decode** extracted stylesheet/image URLs (`&amp;` in `load.php`
  query strings), so Wikipedia's real skin CSS loads instead of a stub.

## [0.3.2]
Screenshot fidelity: CSS positioning + z-index stacking, image rendering, full-page.

### Added
- **`position` + `z-index` in synthetic screenshots** — screenshots now honor
  `position: relative | absolute | fixed | sticky` (out-of-flow boxes are removed
  from flow and placed against their containing block; `relative` boxes shift
  while keeping their space) and paint children back-to-front in CSS stacking
  order (CSS 2.2 §9.9) instead of raw DOM order, so overlapping menus/modals layer
  correctly. Powered by the layout work in turbo-html2pdf 0.2.5, consumed via the
  new `Fragment::paint_order` in `turbo-surf-raster`'s PNG + SVG paint walks.
- **Image rendering** — `<img>` and `background-image` boxes now paint real
  pixels: **PNG, JPEG, GIF, WebP, and SVG** (SVG rasterized via resvg). The raster
  normalizes every decoded image to RGBA and hands turbo-html2pdf a re-encoded PNG
  purely for sizing, so format support lives entirely in the raster. The
  `screenshot` MCP tool and the Playwright shim fetch the page's image bytes over
  the session client (impersonation + cookies apply), `{ images: false }` opts
  out; the raster scales them into the layout box (PNG output) or embeds a base64
  PNG `data:` URI (SVG output). New `image_urls` + `screenshot*_with_assets`
  across raster/napi/Python for callers that fetch the bytes themselves; new
  `net::fetch_bytes` (+ napi `fetchBytes`) returns raw bytes without charset
  decode. Unknown formats fall back to a placeholder.
- **Full-page screenshots** — `full_page` (MCP `full_page`, shim `fullPage`,
  napi/Python `full_page`) grows the image height to the full laid-out content
  height (viewport width still drives layout), clamped to 24k px. The `screenshot`
  reply reports the true image `width`/`height`.

### Changed
- `turbo-surf-raster` now depends on `turbo-html2pdf-core` **0.2.5** (positioning,
  `paint_order`, `layout_html_with_images`), the `image` crate (PNG/JPEG/GIF/WebP
  decode), and `resvg` (SVG rasterization).

## [0.3.1]
Screenshot fidelity: external stylesheets + root-background propagation.

### Added
- **External `<link>` stylesheet fetching** — the `screenshot` MCP tool and the
  Playwright shim now fetch the page's external stylesheets (via the session /
  engine client, so impersonation + cookies apply) and cascade them, so a page
  that keeps its CSS in `<link>` sheets renders styled instead of unstyled.
  `{ external_css: false }` opts out. New `stylesheet_hrefs` +
  `screenshot_with_css` / `screenshot_svg_with_css` across napi + Python for
  callers that fetch the CSS themselves.

### Fixed
- **Root/body background propagation** — a page whose `<body>`/root carries a
  background colour now fills the whole image with it (matching how browsers
  paint the viewport canvas), instead of a white backdrop under short content.

### Notes
- Still no `position: absolute/fixed` or `z-index` / stacking-context model:
  out-of-flow and layered elements paint in document order, so overlapping
  menus / modals on complex pages can stack incorrectly.

## [0.3.0]
Synthetic screenshots — turn any HTML snapshot into an image with no browser.

### Added
- **`turbo-surf-raster` crate** — a screenshot tier that drives
  [turbo-html2pdf](https://crates.io/crates/turbo-html2pdf-core)'s native
  HTML/CSS layout + font engine and paints the resulting `Fragment` display list
  two ways: a **PNG** raster (tiny-skia; boxes, borders, glyph outlines) and an
  **SVG** vector (self-contained glyph `<path>`s). A *reasonably representative*
  render, not pixel-faithful: fragments paint in DOM order (no z-index/stacking
  model) and `<img>` draws as a placeholder. Runs only when asked.
- **`screenshot` MCP tool** — `{ format: png|svg, snapshot?, width?, height? }`;
  renders the current page or any **hydration-trail snapshot** (`dom_history`
  index). PNG returns base64, SVG the document string.
- **napi `screenshot` / `screenshotSvg`** and **`page.screenshot()`** in the
  Playwright shim (returns a real Buffer; `{type:"svg"}` for vector). Element-
  level `locator.screenshot()` still throws (no per-element geometry to crop to).
- **Configurable viewport** — a default `1280×800` layout viewport, overridable
  per call (napi/shim/MCP args) or per session (`set_viewport` MCP tool). Ships
  in every build, including the `impersonate` mega crawl build.
- Python parity: `turbo_surf.screenshot()` / `screenshot_svg()` on the PyPI
  wheel, matching the napi + MCP surfaces.

### Notes
- `<script>`/`<style>`/`<noscript>`/`<template>` source is stripped before
  layout so it never paints as visible text (page `<style>` CSS still cascades).
- No-JS render: visuals painted only by JavaScript (e.g. a `<canvas>` gradient
  background) are absent, so a page whose text is styled for such a background
  can look low-contrast. External `<link>` stylesheets are not fetched.

## [0.2.7]
In-house solver maturity: a proper Cloudflare solve (run the challenge's own JS),
Akamai experimental recon/rebuild tooling, and versioned encoding registries.

### Fixed
- **Hyper Solutions adapter matched to the real API** (`hyper-sdk-go`) — was built
  on wrong assumptions. Correct now: `POST akm.hypersolutions.co/v2/sensor` with the
  `x-api-key` header and `{abck,bmsz,version,pageUrl,userAgent,script,acceptLanguage,
  ip}` body; the response `{payload}` is the `sensor_data` string, which turbo-surf
  POSTs to the target to harvest `_abck`. Akamai lane verified end-to-end (mock);
  other vendors return `Unsupported` to fall back. Scrapfly adapter validated
  against its docs (`/scrape?asp&render_js`, `result.cookies[].name/value`) — already
  correct.

### Added
- **AWS WAF Bot Control solver** (`turbo-surf-core::aws_waf`,
  `TURBO_SURF_SOLVER=awswaf`) — the bot layer behind CloudFront / ALB. Classifies
  the tier (common / targeted `challenge.js` / captcha), runs `challenge.js` in the
  V8 tier (same `PowEngine` as Cloudflare) to mint the `aws-waf-token`, and replays
  it; the `captcha` tier routes to the browser sidecar. New `Vendor::AwsWaf` +
  detection (`x-amzn-waf-action`, `aws-waf-token`, `*.awswaf.com`).
- **Universal browser fallback for in-house solvers** — `cloudflare`, `awswaf`, and
  `akamai` now all try their self-solve first and fall back to the browser sidecar
  when `TURBO_SURF_BROWSER_CMD` is set (via `FallbackSolver`), so a failed in-house
  solve still clears the wall.
- **Proper Cloudflare solver (run the challenge's own JS)** — a `PowEngine` trait
  (core) implemented by the render tier (`turbo_surf_render::V8PowEngine`) lets
  `CloudflareSolver` execute the interstitial's challenge script in the V8 isolate
  and use the answer it computes, instead of the structural placeholder — the
  proper self-solve for CF's JS-compute challenge, no browser. Wired in the MCP
  session via `solver_from_env_pow`.
- **Akamai experimental routing + `analyze_akamai` MCP tool** — the in-house Akamai
  solver is now flagged experimental and, when `TURBO_SURF_BROWSER_CMD` is set,
  routes through a `FallbackSolver` (try in-house → fall back to the browser
  sidecar). New `analyze_akamai` tool: probe the live Akamai script on the current
  page, hash it, build candidate `sensor_data` per stored version, and with
  `{retry:true}` POST each candidate, test live acceptance, and **save a working
  sensor locally** (`TURBO_SURF_SENSOR_DIR`).
- **Versioned solver encodings** — both in-house solvers now store *multiple*
  generations of their challenge encoding behind a registry, since Akamai/CF shift
  format across versions. Akamai `SensorVersion` {V1 plaintext, V2 PRNG-shuffled,
  V3 encrypted-blob} via `generate_sensor_versioned`; Cloudflare `ChallengeVersion`
  {Iuam, Managed, Turnstile} via `detect_version` + `solve_pow_versioned`
  (Turnstile flagged non-self-solvable → routes to the browser sidecar). A harness
  test per vendor sweeps every stored version (deterministic + distinct + correctly
  tagged), so filling one version's real encoding keeps the rest green. Default
  tracks the latest generation.

## [0.2.6]
**Look like a real Chrome on the wire.** The stock client sent a bare
`turbo-surf/0.1` UA + a thin `Accept` and a generic rustls TLS/HTTP-2
fingerprint — an instant tell for WAFs.

### Added
- **Chrome default headers** (default, rustls path, no new build deps) — every
  fetch now sends a current Chrome 149 (macOS) UA plus the full navigation header
  set (`accept`, `accept-language`, `sec-ch-ua`/`-mobile`/`-platform`,
  `sec-fetch-*`, `upgrade-insecure-requests`), values matched against a live
  real-Chrome capture. `accept-encoding` stays client-managed so auto-decompress
  still works; caller/crawl headers still override.
- **`impersonate` feature** (opt-in, BoringSSL) — swaps the reqwest+rustls client
  for `wreq`/`wreq-util`, presenting a real Chrome TLS/JA3/JA4 + HTTP-2 (Akamai)
  fingerprint. Off by default (needs a C toolchain — cmake/nasm — to build);
  forwarded by `turbo-surf-{page,napi,mcp}`. A single `http_backend` alias in
  `turbo-surf-core` swaps the backend in one place. New live e2e
  (`tests/impersonate.rs`) asserts a Chrome JA4 + HTTP-2 fingerprint against a
  public echo (auto-skips offline); a localhost e2e asserts the Chrome headers
  reach the wire on the default path.
- **Real Chrome `navigator` in the JS render tier** — `ENV_BOOTSTRAP` now installs
  a coherent Chrome 149 (macOS) `navigator` (UA, `platform`, `vendor`,
  `webdriver: false`, `hardwareConcurrency`/`deviceMemory`, a Chrome PDF plugin
  set) plus a `window.chrome`, and masks the JS polyfills' `Function.prototype.
  toString` so built-ins (`fetch`, `setTimeout`, …) report `[native code]`.
  Replaces the old `turbo-surf`/`turbo-test` tell page JS used to see. No-Chromium
  env emulation: satisfies passive/consistency anti-bot probes, not active
  canvas/WebGL/audio fingerprinting or PoW challenges.
- **Fingerprint seed pool** (`turbo-surf-core::fingerprint`) — ~4000 internally
  coherent real-Chrome identities, selected deterministically by a client key
  (stable per client, spread across the fleet). Opt-in via `FetchOptions.profile`;
  the default reproduces the prior fixed Chrome-149/macOS wire behaviour. Raises
  the passive/consistency anti-bot bar; does not defeat active fingerprinting/PoW.
- **Challenge-solver integration** (`turbo-surf-core::challenge`) — detect a JS-
  challenge/PoW wall (Akamai/DataDome/Kasada/Cloudflare) and hand it to a server-
  side solver (`ScrapflySolver`/`HyperSolver`) that returns tokens/cookies to
  replay. Configured via env / `.env` (`HYPER_API_KEY`, `SCRAPFLY_API_KEY`,
  `TURBO_SURF_SOLVER`, `TURBO_SURF_PROXY`); inert until a real key is set. See
  `.env.example`. Wired into the MCP session (per-host profile + auto solve/replay
  on a detected wall) and `TurboNavigator` (the crawl seam). Also a self-owned
  `BrowserSolver` (opt-in `TURBO_SURF_SOLVER=browser` + `TURBO_SURF_BROWSER_CMD`)
  that shells to a hardened-headless sidecar over a JSON contract — Chromium stays
  out of the engine; reference sidecar in `harness/browser-solver/`. MCP
  `stealth_status` tool reports the active profile + wired solver.
- **In-house Akamai solver** (`turbo-surf-core::akamai`, `TURBO_SURF_SOLVER=akamai`,
  no key) — the first hand-written `ChallengeSolver`: `generate_sensor` builds a
  deterministic Akamai-shaped `sensor_data` payload, `AkamaiSolver` POSTs it to the
  sensor endpoint and parses the cleared `_abck`. Structure + POST/parse flow are
  tested + green; the dynamic field encoding a live edge validates still needs
  keying off a real `_abck` script (use the `probe` mode).
- **In-house Cloudflare solver** (`turbo-surf-core::cloudflare`,
  `TURBO_SURF_SOLVER=cloudflare`, no key) — parse the managed-challenge
  interstitial (`window._cf_chl_opt`), solve its (JS-compute) PoW, POST to the
  challenge-platform endpoint, harvest `cf_clearance`. Structure + flow green; real
  per-version PoW math keyed off a live challenge. Turnstile-interactive stays on
  the browser sidecar.
- **`probe-script` example** (`cargo run -p turbo-surf-render --example probe-script
  -- script.js`) — run the `probe` instrumentation over a real captured anti-bot
  script and print what it touched + the shim gaps. Run against a real Akamai
  sensor it surfaced two missing `navigator` props (`connection`,
  `userAgentData`), now added to the render-tier navigator (coherent with the UA).
- **Runtime-controllable render fingerprint** — every render-tier `navigator` field
  (UA, platform, vendor, languages, hardwareConcurrency, deviceMemory, chromeMajor,
  connection, userAgentData, screen, devicePixelRatio) now has a Chrome 149 default
  and is overridable via `turbo_surf_render::set_fingerprint(json)` / the MCP
  `set_fingerprint` tool. `stealth_status` reports the active overrides.
- **Fingerprint debug/probe mode** (`turbo-surf-render::probe_globals`, MCP `probe`
  tool) — run a page's JS with `navigator`/`screen`/`window.chrome`/canvas wrapped
  in logging proxies and report every property it touched + which reads returned
  `undefined` (the shim to-do list). Recon for what an anti-bot check probes and
  what's left to emulate.

## [0.2.5]
A **pooled-render latency** fix on the JS-crawl fast path.

### Fixed
- **Watchdog join latency in `render_page_pooled`** — the per-page execution-budget
  watchdog polled completion on a 2 ms sleep loop, so `watchdog.join()` after a healthy
  render blocked until the watchdog woke from its current sleep, adding up to 2 ms of
  latency to *every* pooled render. The watchdog now `park_timeout`s on the budget
  deadline and is `unpark`ed the instant the render completes, so a healthy render's join
  returns in µs (an elapsed-guard loop survives spurious wakeups; the deadline still
  terminates a runaway script). Measured on `quotes.toscrape.com/js` (warm pool):
  **renderPooled 2.6 ms → 1.3 ms (−50%)**, output byte-identical. The politeness/network-
  bound crawl wall is unchanged; CPU/parallel render throughput ~doubles.

## [0.2.4]
A **Linux SIGBUS** fix in the Playwright-shim test harness, plus a new **Python
(PyPI) binding**.

### Fixed
- **SIGBUS on Linux running the shim suite** (#6) — root cause was a bug in the shim's
  fake `@playwright/test` harness: `test.describe(...)` was registered as a node:test
  *test* instead of a *suite*, so the nested `test(...)` calls in a describe body fired
  on the global runner while the parent test was still running. node:test cancelled them
  ("test did not finish before its parent and was cancelled"), and the dangling async
  test — still holding a live-session V8 isolate — was torn down at process exit, which
  faulted with SIGBUS on Linux (macOS tolerated it). Latent until v0.2.3 wired a real
  `npm test`, so the multi-file shim run never executed on CI before. `makeDescribe` now
  registers a real node:test suite, so nested tests are awaited and torn down cleanly.
- **V8 platform init hardening** (defense-in-depth) — `ensure_platform()` initializes the
  V8 platform once on a dedicated, parked keeper thread (deno_core otherwise inits it
  lazily on whichever thread builds the first runtime; a transient one that then exits
  orphans the platform). Called before any worker isolate is created from `evaluate`,
  `render`, `render_pooled`, `hydrate`, and `live_open`.

### Added
- **Python binding (`turbo-surf` on PyPI)** — a PyO3 abi3 wheel (CPython 3.8+) exposing
  the stateless parse → view/extract → JS-render surface (`markdown`, `text`, `links`,
  `query`, `extract`, `evaluate`, `render`, `transform`, …), mirroring the Node N-API
  functions. New crate `rust/crates/turbo-surf-py`; `release-py.yml` builds + publishes
  wheels on a `pyv*` tag (gated on a `PYPI_TOKEN` secret). A real `test` npm script + the
  stale shim-assertion fixes from v0.2.3's CI work are included.

## [0.2.3]
A **JS-render speed** pass on the crawl path: the render tier built a fresh V8 isolate
per page (boot + the ~90 KB env bootstrap + parse dominate), so a JS-mode crawl paid
the full isolate boot on every page. A pooled fast path reuses one isolate across pages
— boot is paid once per worker thread — with a cross-page global scrub so a reused
isolate still renders like a fresh navigation. **11.5 ms → 3.2 ms per page** on
`quotes.toscrape.com/js` (3.6×), output byte-identical to the fresh render.

### Added
- **Pooled render fast path** — `render_page_pooled` (Rust) / `renderPooled` (napi)
  reuses a thread-local V8 isolate across pages, on a persistent render worker (one
  long-lived thread + one reused tokio runtime). Per-page session repoint
  (base/cookies/UA) + a global scrub (`SCRUB_GLOBALS`) restore fresh-navigation
  semantics; a budget-terminated/errored runtime is dropped instead of repooled. The
  competitive JS adapter drives it; `render` (fully isolated, fresh-per-page) is
  unchanged for correctness-sensitive callers.
- **`harness/hotpath/render-bench.mjs`** — reusable offline profiler for `native.render`
  / `renderPooled` (faithful script extraction, cached sample, A/B + parity check).

### Notes
- Cross-page isolation is intentionally relaxed for crawl speed (matching the existing
  `EVAL_RT` stance): the scrub reverts page-ADDED globals, not builtins mutated in place.
- A V8 code cache for the bootstrap + page bundle was tried and reverted — with a fresh
  isolate per page, `ConsumeCodeCache` costs more than a re-parse. Isolate reuse is the
  real lever.

## [0.2.2]
The headless **Playwright-shim parity** push: the payroll-app Playwright e2e suite
now runs through the browserless shim (over the napi addon, **no Chromium**), driving
a real authenticated Next.js App Router SPA. Side-by-siding every failure against real
Chromium (reseeded per suite) drove the engine to parity — the suite's remaining reds
all reproduce in Chromium too (app/backend/test data, not the engine). See
`HEADLESS-HYDRATION.md` for the full record.

### Added
- **ES module support in the render tier** — `<script type=module>` + `import` graphs
  fetched/linked over the host net (shared cookie jar, same-origin), wired into the
  hydration pump. Turbopack-dev entry execution (`document.currentScript` in the module
  pump, `__name` helper); classic `<script src>` chunks with ESM bodies route to the
  module pump.
- **Live-isolate interaction drive** — `getByRole/getByText/getByLabel` resolve and
  dispatch IN the running isolate (live `querySelectorAll('*')` index), so fills/clicks
  reach the real app, not just the static snapshot. Web-first assertion retry
  (re-pumps the live app between tries); `page.on('response')` / `waitForResponse`
  backed by a real network log; fetch-aware drain (`__pendingFetches`) keeps pumping
  until a mutation's success re-render lands.
- **Nth-aware scope chain for nested locators** — `parent.nth(i).getBy*()` walks the
  chain via `__tcResolveScoped` (a CSS-concat selector can't express "the i-th match's
  subtree"); `getByRole/Text/Label`/`getByTestId` scope to the parent's subtree.
- **CSS `:hover` simulation** — hover-revealed menus (incl. emotion's nested `&:hover`)
  become visible by flattening the matched rules inline.
- **Download capture + `ElementHandle` + polling `waitForFunction`**; keyboard events +
  `navigator.clipboard`; `structuredClone`; `addInitScript`; `setInputFiles`;
  per-test fixture sharing; `test.extend` custom fixtures.

### Fixed
- **App Router RSC hydration unblocked** — defined `document.location` (a browser
  invariant the dev RSC flight client reads via `findSourceMapURL`); its absence threw
  inside the flight-stream parse and the React root suspended forever. 0 → 488 fibers on
  the live payroll route.
- **RSC soft-nav follow + query preservation** — Next client navigation
  (`router.push/replace`) fetches the target's RSC flight and never advances
  `location` headlessly; the target is recorded on `__rscNav` and re-loaded hop-by-hop
  (login redirect chain completes). It now records `pathname + search + hash` (was
  `pathname` only — dropped `?employeeIds=` etc.) and strips Next's `_rsc` cache-buster.
  Guard: `rsc_soft_nav_preserves_query_and_strips_rsc_param`.
- **`reload({waitUntil:'networkidle'})` re-hydrates the live SPA** — `reload` ignored its
  options (unlike `goto`), leaving the reloaded doc as the raw un-hydrated shell (a
  settings select read `""`). Guard: surface "reload re-hydrates the live SPA".
- **`Locator.filter()` scopes child locators** — `cards.filter({hasNotText: x}).first()
  .getByTestId('y')` resolved against the UNFILTERED set; a serializable
  `hasText`/`hasNotText` spec now rides the scope chain. Guard:
  `scoped_resolve_applies_filter_before_indexing`.
- **Browser-accurate click** — pointerdown→mousedown→focus(only if mousedown not
  preventDefault'd + focusable)→pointerup→mouseup→click, pointer events first (MUI
  v7/Radix gate on them); honors `preventDefault` so an `<a href="#">` whose onClick
  toggles state doesn't also navigate.
- **Playwright `isVisible` semantics** — `is_visible` ignores `aria-hidden` (pure
  CSS/layout, like Playwright) and treats effective `opacity:0` / `display:none` /
  `visibility:hidden` ancestors as hidden (closing MUI modals resolve
  `waitFor(state:'hidden')`).
- **Drain/timer correctness** — runtime-injected `<script>`s run during interaction
  drains (`next/dynamic` lazy modals); the virtual-timer budget is RELATIVE per drain so
  a closing MUI Fade's short exit timer isn't killed.
- **Shim parity** — `waitFor` polls the requested state (visible/hidden/attached);
  `page.evaluate` awaits a returned Promise; `waitForResponse` won't match a response
  from an earlier step; `about:blank` is a no-fetch blank doc; `waitForFunction` returns
  the function's value; RegExp locator names; `boundingBox` → null. Locale: the shim no
  longer force-seeds `NEXT_LOCALE=en-US` (matches Playwright's es-MX default).

## [0.2.1]

### Fixed

- **Authenticated SPA pages now render headlessly.** Heavy authed routes (e.g. a
  payroll people/grid page) previously committed an empty body in the render tier
  while rendering fine in a real browser. Two render-tier bugs let third-party
  scripts spin to the render budget so React never committed:
  - **Virtual-clock timers.** `__runTimers` used `delay` only as a sort key, so a
    self-rescheduling `setTimeout` poll (analytics SDKs do this) fired until the
    raw count cap, starving the budget. A virtual clock now gates delayed timers
    (`due = now + delay`, advance on fire, stop past a 15s virtual ceiling) so
    polls fire a browser-like number of times and the page quiesces.
  - **`<iframe>` `contentWindow`.** Analytics SDKs read a builtin's native
    prototype off a throwaway iframe's `contentWindow`; with none present they
    recreated an iframe on every lookup (hundreds of churned iframes). Iframes now
    get a lightweight stub whose `contentWindow` is the current realm (the lookup
    caches, the loop stops) and that never enters the rtdom tree.

  New tests: `virtual_clock_bounds_self_rescheduling_timers`,
  `iframe_content_window_exposes_builtins`.

### Added

- **Authenticated SPA journeys hydrate + drive headlessly.** Next.js App Router
  pages render through the render tier and the Playwright shim can log in and drive
  them end to end (no Chromium). Against clean staging: payroll-wizard 5/5, invites
  26/26, auth-guards 32/38. Key enabler: define `document.location` (mirrors
  `window.location`) so Next's DEV RSC-flight parse doesn't abort (0 → 488 fibers).
- **ES module support** in the render tier: `<script type=module>` + `import` graphs
  fetched over the net, classic `<script src>` chunks with ESM bodies routed to the
  module pump.
- **Shim parity surface:** `page.waitForEvent('download')` + a `Download`
  (`path()`/`saveAs()`/`suggestedFilename()`) backed by a real `URL.createObjectURL`
  registry + `<a download>` capture; `Locator.elementHandle()` + a polling
  `page.waitForFunction(fn, handle)`; CSS `:hover`-revealed menus become visible
  (`__tcApplyHover` flattens nested emotion `&:hover` rules and applies the reveal
  inline); locator subtree scoping for `getByTestId`/`getByRole`/`getByText`/
  `getByLabel`, including an nth-aware scope chain for `steps.nth(i).getBy*`.

### Fixed

- **`waitForResponse` no longer matches a response from an earlier step** — it tags
  each drained response with the interaction that produced it and only accepts one
  from the current action or later (Playwright "after the call" semantics), so a
  loose URL predicate can't grab a prior step's response.
- **Visibility matches Playwright `isVisible`:** effective `opacity:0` (self/ancestor)
  reads hidden so a faded-out MUI modal resolves `waitFor(state:'hidden')`; and
  `aria-hidden` is NOT treated as hidden (a decorative aria-hidden icon carrying a
  test-id is still visible — aria-hidden stays an accessibility-query concern).
- **Modals open/close reliably:** the virtual-timer budget is RELATIVE per drain (a
  closing Fade's late exit timer fires), and `drain_to_quiescence` runs runtime-
  injected `<script>`s so `next/dynamic` lazy modals load on click.
- **Interactions are browser-accurate:** click fires the full pointer→mouse→click
  sequence (focus only when mousedown isn't preventDefault'd), and
  getByRole/getByText/getByLabel resolve in the LIVE isolate so portal'd MUI options
  dispatch their onClick.
- **Locale parity:** the shim no longer force-seeds `NEXT_LOCALE=en-US`; with no
  cookie the app resolves its default locale (es-MX), matching real Playwright.

New tests (render + shim `surface.test.mjs`): `createobjecturl_anchor_download_is_
captured`, `hover_reveals_css_hover_menu`, `tcgetby_scopes_to_root`,
`tcresolvescoped_walks_nth_chain`, `aria_hidden_stays_visible`, waitForResponse
staleness guards, getByTestId/getByRole scoping guards.

Additional shim fixes:

- **`about:blank` navigation** is a no-fetch blank document (a `goto`/`reload` of
  about:blank used to hit the net layer → `builder error for url (about:blank)`).
- **`waitForFunction` resolves the function's return value** (not a boolean) and
  works against a static snapshot — `Number(await waitForFunction(() => 1 + 1))`
  is `2` again (regression from the polling-handle rewrite).
- **`test.extend` custom fixtures inject** — a test fn with its own extDefs (a
  custom fixture or a `page` override) opens its own fixture set instead of
  reusing the base-only shared one (they resolved to `undefined` before).
- **Test isolation:** the env-var-mapping test restored `TURBO_SHIM_*` with
  `delete` instead of `= undefined` (the latter coerces to the string
  `"undefined"`, leaking `testIdAttribute`/`baseURL` into every later context and
  cascading failures across the serial suite).

## [0.2.0]

**turbo-surf is a browserless, native-speed crawler _and_ Playwright-compatible
script runner for AI agents — one engine, no Chromium.** It fetches, parses, and
acts on pages on its own native DOM ([turbo-dom](https://github.com/miaskiewicz/turbo-dom),
the `turbo-dom` Rust crate), and for JS-gated pages it runs the page's own scripts
in a **true V8 isolate** (a `deno_core` runtime — host heap unreachable from the
guest, with a runaway-execution budget) and re-renders the DOM. No headless
browser, no pixels, no layout.

What it does:

- **Crawl** — point it at a domain and stream page records: indexed interactive
  elements, a link/form graph, an accessibility tree, markdown and plain-text
  views, rendered-HTML capture, CSS/XPath queries, schema-driven structured
  extraction. Concurrency + per-host politeness (token-bucket), backoff/retry,
  canonical-dedupe, robots + crawl-delay, depth/page caps.
- **Drive pages (Playwright-compatible)** — the same `chromium.launch()` →
  `page.goto()` → locators → actions → `expect` surface, plus a `@playwright/test`
  drop-in `test` runner, so existing Playwright scripts/tests run unchanged against
  the engine instead of a browser. Network events, request routing/mocking, and
  persistent context state (cookies + `localStorage` + `storageState`).
- **Run page JS, no browser** — recover SPA data either by mining server-embedded
  hydration state (`__NEXT_DATA__`, JSON-LD, `__APOLLO_STATE__`, …) or by executing
  the page's own scripts in the V8 isolate (real jQuery / React-style bundles
  render); `fetch`/`XMLHttpRequest` are bridged to the host net layer.
- **Agent surface** — a 60-tool **MCP** server (stdio JSON-RPC) agents drive
  directly: navigate, click/fill/submit, query, extract, accessibility tree,
  markdown, `crawl`, `batch`, `render`/`eval_js`/`inject_js`, cookies/headers,
  `snapshot`. Available as both a Node server and a native Rust binary.

Engine: a Rust workspace (core / page / view / render / transform / napi / mcp) on
the `turbo-dom` crate, exposed to Node through a napi addon and a Playwright shim.
Performance is network-bound — a pooled HTTP client, a persistent V8 isolate
reused across pages, an external-script cache, and a per-page parse cache. In
benchmarks it runs the same routines as Chromium at parity while being multiples
faster, and outpaces other crawlers (Cheerio/Scrapy/Colly and browser-driving
crawlers) — see the README.
