# Quality gates

CI (`.github/workflows/ci.yml`) gates line coverage and cyclomatic complexity on top of the
`lint + test` job. Run the same commands locally before pushing.

## Coverage

Line coverage of the Rust workspace is measured by `cargo tarpaulin` with the LLVM engine
(`rust/tarpaulin.toml`: the member crates' `src/` minus the exclusions under "Not measured", the
whole workspace's tests):

```sh
cd rust && cargo tarpaulin        # one whole-workspace run; fails below `fail-under`
```

CI shards the same measurement (one instrumented link of every test binary exhausts the ubuntu
runner): the `coverage-shard` matrix runs `python3 ../scripts/coverage-shard.py <shard>
<crates>` from `rust/` for render, mcp and core+view+page+transform, with `tarpaulin.toml`'s
settings, and the `coverage` job merges the shard reports (`scripts/merge-coverage-reports.py`:
each crate keeps its own shard's coverable lines, a line is covered when any shard hits it)
and applies the workspace `fail-under` and the per-crate floors to the merge. The render
shard passes `--chunks 4`: every test process writes a profraw of all instrumented code, so
render's test binaries are run in four chunks over one build, the profraws deleted between
chunks (one run filled the 145G runner disk), and the chunk reports unioned.

Then every crate must meet its own floor (`rust/coverage-floors.json`, checked by
`python3 scripts/check-coverage-floors.py rust/target/cov/report/tarpaulin-report.json
rust/coverage-floors.json`; a crate without a floor fails too). Floors are the values measured
on 2026-10-28 (consol/cov-c4 5895357), rounded down to 0.1; the workspace `fail-under` is
93.0 (measured 93.07%, 19287/20723 lines):

| crate | measured | floor |
|---|---|---|
| turbo-surf-core | 99.44 * | 99.4 |
| turbo-surf-mcp | 99.67 | 99.6 |
| turbo-surf-page | 100.00 | 100 |
| turbo-surf-render | 95.38 † | 95.0 ‡ |
| turbo-surf-transform | 100.00 | 100 |
| turbo-surf-view | 99.91 | 99.9 |

mcp was re-measured on its own (`cargo tarpaulin -p turbo-surf-mcp`, 1829/1835 lines).
Its remaining lines need the live network or a real reCAPTCHA token: the
`browserless_google_serp` entry point (fixed to www.google.com; its core
`browserless_serp` is tested on localhost) and an in-isolate `/sorry` token.

\* core was re-measured on consol/cov-c4c with its own tests only (`cargo tarpaulin` over
`turbo-surf-core`: 99.44%, 3922/3944). The workspace run adds the other crates' tests, so this
is a lower bound; the lines still missed by both runs are platform-only (`x86` on Intel
macOS), a `sh` spawn/wait failure, a brotli dictionary-attach failure, a cancellation race,
and two lines of a generic async-trait adapter that tarpaulin never attributes.

† render was re-measured on consol/cov-c4b bb1896f (with consol/cov-c4c/c4d merged) with its
own tests only (`cargo tarpaulin` over `turbo-surf-render`, the exclusions below: 95.38%,
12914/13540); the workspace run adds the other crates' tests, so this is a lower bound. A few
timing-dependent render paths move ~3-5 lines between identical runs, inside the 0.08 margin.
The remaining render lines are mostly V8 allocation/termination/teardown guards, op-argument
guards that only the bootstrap JS can reach, and the network report/redirect corners of CSP
reporting; the release target for render is ~97.5%.

‡ The render floor was set to 95.0 (from 95.3) by user decision on 2026-10-28, when PR CI moved
to ubuntu runners (macOS runners unavailable) and the Linux measurement moved against the macOS one.

Target: >= 99.5 per measured crate. The coverage phase raises each floor to the measured
value as tests land; never lower one to pass a change.

### Not measured

- **`turbo-surf-napi`, `turbo-surf-py`** (excluded in `rust/tarpaulin.toml` and listed under
  `excluded` in `rust/coverage-floors.json`): the Node and Python bindings. Their code runs
  only inside `node` (the Playwright shim and detection suites) and `python` (the wheel
  tests) processes, which tarpaulin does not instrument, and they have no Rust tests.
- **`turbo-surf-render/src/window_proxy.rs`, `frame_location.rs`, `frame_ancestors.rs`,
  `code_generation.rs`, `frame_modules.rs`** (`exclude-files`; `stock_engine.rs` holds their
  stock-SDK stand-ins): compiled only under `cfg(turbo_surf_native_proxy)`, which
  `turbo-surf-render/build.rs` sets for `TURBO_SURF_ENGINE=154|155`. That engine is a V8
  compiled from source at a pinned Chromium revision (`harness/detection/engine-154`,
  `engine-155`: network checkout, the Chromium toolchain, several GB of caches and hours of
  build on macOS arm64); no prebuilt artifact is published, so CI cannot build it and the
  default build has no lines to cover in these files. They are gated in that configuration
  instead: every engine build run by the helper executes the native WindowProxy/security
  regressions and the frame suites (`tests/frame_modules.rs`, `iframe_default_layout.rs`,
  `sandbox_forms.rs` and the `cfg(turbo_surf_native_proxy)` cases elsewhere) before it is
  accepted.

### Native-engine render coverage (manual, not in CI)

The native configuration is measured on a machine with a built engine SDK
(`harness/detection/engine-155`, macOS arm64; on a macOS 27 host build the SDK with
`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk`, the pinned lld cannot read
the 27 SDK's `libSystem.tbd`). `build.py --surf` passes its own cargo `--config`, which
`cargo tarpaulin` rejects, and tarpaulin's source walk skips the helper workspace's symlinked
`src` directories, so the run is: `cargo tarpaulin -p turbo-surf-render --features
native-sdk-frame-read --engine llvm` from `.native-builds/sdk155/surf-workspace/rust` (with
the helper's SDK alias config as `.cargo/config.toml` and its environment:
`TURBO_SURF_ENGINE=155`, `TURBO_SURF_ENGINE_VERSION`, `RUSTY_V8_ARCHIVE`,
`RUSTY_V8_ARCHIVE_SHA256`, `RUSTY_V8_SRC_BINDING_PATH`), then `llvm-profdata merge` of its
profraws and `llvm-cov export -format=lcov` over the test binaries. Measured 2026-10-28 on
consol/cov-c4b bb1896f (LLVM line records, a finer count than tarpaulin's):

| scope | covered/lines | % | floor |
|---|---|---|---|
| render, native engine | 18839/20576 | 91.56 | 91.5 |
| native-only files (window_proxy, frame_location, frame_ancestors, code_generation, frame_modules) | 1277/1736 | 73.56 | 73.5 |

Three render tests fail in that configuration independently of coverage (#71).

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

Both jobs are **blocking**: every tracked function stays at or below cyclomatic complexity 15,
no allow-list, no file skips, no raising the limit. Any PR that introduces a function over 15
must either decompose it or back out the change.

The limit was chosen from the distribution measured when the gate landed (functions above
each threshold):

| scope | functions | > 5 | > 10 | > 15 | > 20 | > 25 | > 30 | > 40 | max |
|---|---|---|---|---|---|---|---|---|---|
| Rust, all tracked `.rs` | 6245 | 365 | 118 | 54 | 28 | 15 | 11 | 9 | 107 |
| JS, `cli.js` + `index.js` + `harness/` | 5630 | 326 | 112 | 55 | 32 | 21 | 10 | 3 | 62 |

At 15 the backlog is 109 functions (0.9% of all functions); 10 would more than double it
(230) and reach into ordinary dispatch code, while 20 would leave a 16-20 band whose branch
sets are already too large to test exhaustively. Many `harness/detection` probes and
collectors are hash-pinned by a Chrome control (for example `chrome-cdp-trace.mjs` and
`input-state.js`, checked by `resource-scan-order-chrome.test.mjs`): refactoring one
re-captures its controls in the same change.

The backlog is cleared: `npm run -s complexity:rust` and `npm run -s complexity:js` both exit
0. See CHANGELOG for the refactor history (the last entry closed #154).
