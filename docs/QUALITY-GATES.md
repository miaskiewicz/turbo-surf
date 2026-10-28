# Quality gates

CI (`.github/workflows/ci.yml`) gates line coverage and cyclomatic complexity on top of the
`lint + test` job. Run the same commands locally before pushing.

## Coverage

Line coverage of the Rust workspace is measured by `cargo tarpaulin` with the LLVM engine
(`rust/tarpaulin.toml`: the member crates' `src/` minus the exclusions under "Not measured", the
whole workspace's tests):

```sh
cd rust && cargo tarpaulin        # CI job `coverage`; fails below `fail-under`
```

Then every crate must meet its own floor (`rust/coverage-floors.json`, checked by
`python3 scripts/check-coverage-floors.py rust/target/cov/report/tarpaulin-report.json
rust/coverage-floors.json`; a crate without a floor fails too). Floors are the values measured
on 2026-10-28 (consol/cov-c4), rounded down to 0.1; the workspace `fail-under` is 92.5
(measured 92.51%, 18912/20444 lines):

| crate | measured | floor |
|---|---|---|
| turbo-surf-core | 95.80 | 95.8 |
| turbo-surf-mcp | 84.39 | 84.3 |
| turbo-surf-page | 100.00 | 100 |
| turbo-surf-render | 91.83 | 91.8 |
| turbo-surf-transform | 100.00 | 100 |
| turbo-surf-view | 99.91 | 99.9 |

Target: >= 99.5 per measured crate. The coverage phase raises each floor to the measured
value as tests land; never lower one to pass a change.

### Not measured

- **`turbo-surf-napi`, `turbo-surf-py`** (excluded in `rust/tarpaulin.toml` and listed under
  `excluded` in `rust/coverage-floors.json`): the Node and Python bindings. Their code runs
  only inside `node` (the Playwright shim and detection suites) and `python` (the wheel
  tests) processes, which tarpaulin does not instrument, and they have no Rust tests.
- **`turbo-surf-render/src/window_proxy.rs`, `frame_location.rs`, `frame_ancestors.rs`**
  (`exclude-files`): compiled only under `cfg(turbo_surf_native_proxy)`, which
  `turbo-surf-render/build.rs` sets for `TURBO_SURF_ENGINE=154|155`. That engine is a V8
  compiled from source at a pinned Chromium revision (`harness/detection/engine-154`,
  `engine-155`: network checkout, the Chromium toolchain, several GB of caches and hours of
  build on macOS arm64); no prebuilt artifact is published, so CI cannot build it and the
  default build has no lines to cover in these files. They are gated in that configuration
  instead: every engine build run by the helper executes the native WindowProxy/security
  regressions and the frame suites (`tests/frame_modules.rs`, `iframe_default_layout.rs`,
  `sandbox_forms.rs` and the `cfg(turbo_surf_native_proxy)` cases elsewhere) before it is
  accepted.

## Complexity

Every function's cyclomatic complexity must be <= 15: one limit for the whole repo, no
allow-list, no baseline (turbo-html2pdf's own crates stay at cc < 6).

```sh
npm run complexity:rust   # CI job `complexity / rust (cc <= 15)`
npm run complexity:js     # CI job `complexity / js (cc <= 15)`
```

- Rust: turbo-html2pdf's `cc-check` over every tracked `.rs` file (`git ls-files`; a `match`
  counts once, `if`/loops/`?`/`&&`/`||` add 1, closures are scored on their own). It is
  installed from its home, not copied here:
  `cargo install --locked --git https://github.com/miaskiewicz/turbo-html2pdf --rev
  923c89f791f3a89cb089a59626a25cef0a9cbd5f cc-check` (the rev CI pins).
- JS: oxlint's ESLint-compatible `complexity` rule over `cli.js`, `index.js` and `harness/`
  (`.oxlintrc.complexity.json`, which enables only that rule). It runs `oxlint@1.87.0`
  through npx because the `lint` script's oxlint 0.11 has no `complexity` rule.

Both jobs are **expected red** until the post-coverage refactor: the functions above 15 when
the gate landed are the refactor backlog below, refactored only once their unit-test coverage
is complete (refactoring without full tests can change behaviour silently). Meanwhile both run
with `continue-on-error` (red job, green workflow) and are not required checks. New code stays
<= 15; never raise the limit or exclude a file to turn the jobs green.

The limit was chosen from the distribution measured when the gate landed (functions above
each threshold):

| scope | functions | > 5 | > 10 | > 15 | > 20 | > 25 | > 30 | > 40 | max |
|---|---|---|---|---|---|---|---|---|---|
| Rust, all tracked `.rs` | 6245 | 365 | 118 | 54 | 28 | 15 | 11 | 9 | 107 |
| JS, `cli.js` + `index.js` + `harness/` | 5630 | 326 | 112 | 55 | 32 | 21 | 10 | 3 | 62 |

At 15 the backlog is 109 functions (0.9% of all functions); 10 would more than double it
(230) and reach into ordinary dispatch code, while 20 would leave a 16-20 band whose branch
sets are already too large to test exhaustively. Two backlog files are hash-pinned by a
Chrome control (`harness/detection/chrome-cdp-trace.mjs` and `input-state.js`, checked by
`resource-scan-order-chrome.test.mjs`): refactoring either re-captures that control in the
same change.

### Backlog (`function:line=cc`)

Rust (54):
- `rust/crates/turbo-surf-core/src/challenge.rs`: detect:211=31
- `rust/crates/turbo-surf-core/src/compression_dictionary.rs`: store_generation:202=17
- `rust/crates/turbo-surf-core/src/compression_dictionary/decode.rs`: decode_reader:105=17
- `rust/crates/turbo-surf-core/src/cookies.rs`: rejected:181=18
- `rust/crates/turbo-surf-core/src/net.rs`: cached_request:1406=42, follow_manually_response:1661=30, fetch_typed_bytes:2023=18
- `rust/crates/turbo-surf-core/src/request_context.rs`: headers:225=21
- `rust/crates/turbo-surf-core/tests/compression_dictionary.rs`: new:38=22
- `rust/crates/turbo-surf-mcp/examples/detection_trace.rs`: capture:428=107
- `rust/crates/turbo-surf-mcp/examples/identity_battery.rs`: run:82=21
- `rust/crates/turbo-surf-mcp/src/lib.rs`: call_tool:3064=16
- `rust/crates/turbo-surf-py/src/lib.rs`: register_functions:336=46
- `rust/crates/turbo-surf-render/src/audio_buffer.rs`: render:20=42
- `rust/crates/turbo-surf-render/src/audio_dsp.rs`: oscillator_events:300=16, render:508=63
- `rust/crates/turbo-surf-render/src/audio_effects.rs`: new:80=16
- `rust/crates/turbo-surf-render/src/audio_timeline.rs`: validate:41=17, render:77=30
- `rust/crates/turbo-surf-render/src/compression_dictionary.rs`: fields:148=17, header_links:204=17
- `rust/crates/turbo-surf-render/src/crypto_clone.rs`: read_host_object:324=17, validate_transfers:444=25
- `rust/crates/turbo-surf-render/src/crypto_hybrid.rs`: import_jwk_inner:330=20
- `rust/crates/turbo-surf-render/src/csp.rs`: matches_source:391=16, new:480=23
- `rust/crates/turbo-surf-render/src/csp_reporting.rs`: send:274=45
- `rust/crates/turbo-surf-render/src/frame_location.rs`: navigate:29=16
- `rust/crates/turbo-surf-render/src/live_layout_backend.rs`: scroll_into_view:859=23, offset:1396=17, range_geometry:1479=21
- `rust/crates/turbo-surf-render/src/live_layout_dom.rs`: capture_kind:77=44
- `rust/crates/turbo-surf-render/src/live_layout_scoped.rs`: prepare_rule:46=20, compound:337=21
- `rust/crates/turbo-surf-render/src/live_layout_zoom.rs`: scale_value:191=17
- `rust/crates/turbo-surf-render/src/live_stylesheets.rs`: append_rules:83=16
- `rust/crates/turbo-surf-render/src/message_ports.rs`: op_port_post_message:587=17
- `rust/crates/turbo-surf-render/src/runtime.rs`: op_fetch:2646=43, queue_window_message:4715=20, (closure):4902=43, allocate_frame:5636=18, clear_storage_from_response:6444=22, op_storage_call:6792=22, install_dom:12835=17, (closure):13437=33, drain_event_loop_until_completion:13234=30, hydrate_installed_document:14395=21, drain_to_quiescence_with_scripts:14618=21
- `rust/crates/turbo-surf-render/src/script_fetch.rs`: fetch:23=26
- `rust/crates/turbo-surf-render/src/window_proxy.rs`: capture_functions:61=17
- `rust/crates/turbo-surf-render/tests/cache_advanced.rs`: resource_loaders_clear_site_data_and_ancestor_partition_match_chrome155:8=23
- `rust/crates/turbo-surf-render/tests/cache_unread.rs`: unread_cache_completion_collection_and_cancellation_match_chrome:33=17
- `rust/crates/turbo-surf-render/tests/csp_reporting.rs`: source_pinned_chrome_reports_use_actual_private_transport:392=20
- `rust/crates/turbo-surf-render/tests/lifecycle_resources.rs`: session_runs_deferred_scripts_at_interactive_and_waits_for_decoded_resources_after_dcl:15=17

JS (55; `function:<line>` is an anonymous function):
- `harness/detection/activation-headers-chrome.mjs`: function:32=17
- `harness/detection/audio-advanced-contract-probe.js`: function:1=18
- `harness/detection/audio-advanced-probe.js`: function:1=26
- `harness/detection/authored-cache-advanced-chrome.mjs`: function:20=38
- `harness/detection/authored-cache-unread-chrome.mjs`: function:20=44
- `harness/detection/chrome-cdp-trace.mjs`: function:152=20, capture:248=62
- `harness/detection/chrome-network-metadata.mjs`: enrichResponseTransport:34=17, summarizeResponseTransport:98=19
- `harness/detection/chrome-trace.mjs`: capture:89=37
- `harness/detection/collector-interruption.test.mjs`: function:102=23
- `harness/detection/compare.mjs`: function:125=21, function:206=30, function:248=17
- `harness/detection/compression-dictionary-server.mjs`: function:74=29
- `harness/detection/cpu-profile-summary.mjs`: summarizeCpuProfile:6=28
- `harness/detection/csp-report-endpoint-chrome.mjs`: function:51=18
- `harness/detection/css-security.mjs`: cssSecurityErrors:21=24
- `harness/detection/document-parser.mjs`: parserErrors:40=35
- `harness/detection/dom-adoption-probe.js`: function:1=28
- `harness/detection/dom-tokens-aria-probe.js`: function:3=16
- `harness/detection/dom-variadic-errors-probe.js`: function:1=18
- `harness/detection/form-methods-probe.js`: function:1=20
- `harness/detection/identity-diff.mjs`: actionFor:367=17, diffVersions:394=19, measuredRelease:508=26
- `harness/detection/identity-server.mjs`: function:150=17
- `harness/detection/identity-wire.mjs`: describeClientHello:78=18, describeFrame:192=17
- `harness/detection/input-state.js`: function:2=19
- `harness/detection/interface-trace.js`: function:121=29, function:210=23
- `harness/detection/leading-space-geometry-probe.js`: function:60=22
- `harness/detection/lifecycle.mjs`: lifecycleErrors:23=23
- `harness/detection/live-text-geometry-probe.js`: function:1=19
- `harness/detection/modules.mjs`: frameModuleErrors:48=26, moduleErrors:73=47, evaluationErrors:126=38, captureModules:187=16
- `harness/detection/native-iframe-layout-probe.js`: function:1=17
- `harness/detection/navigation-observation.test.mjs`: function:279=28
- `harness/detection/redirect-policy.mjs`: redirectErrors:19=26
- `harness/detection/reversion-chrome.mjs`: transportReport:232=20, printReport:305=24, main:338=32
- `harness/detection/script-credentials.mjs`: credentialErrors:43=37, function:93=29
- `harness/detection/storage-probe.js`: function:1=18
- `harness/detection/stylesheet-origins.mjs`: stylesheetErrors:20=22
- `harness/detection/summarize-run.mjs`: processSummary:99=23, summarizeRun:133=31, function:286=20
- `harness/detection/transport-hello.mjs`: parseClientHello:15=23
- `harness/detection/trusted-types-probe.js`: function:2=21
- `harness/detection/window-timer-semantics-chrome.mjs`: captureWindowTimerSemantics:14=19
