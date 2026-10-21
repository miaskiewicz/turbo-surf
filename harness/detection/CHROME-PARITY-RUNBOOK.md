# Chrome 155 parity runbook

These commands document the verified setup and every opt-in. The latest frozen
capture and full native/default gates use9304df9:1,509/1,464 Rust tests pass,
with strict checks, formatting and verified addon consumers. Both native Google
baselines finish initial hydration with one pump inside the unchanged budget,
then save consent, submit real forms and end429 with zero results. The two
interface-instrumented native captures still exhaust initial hydration. Both
Chrome baselines fail effective consent dismissal, so the current SERP comparison
is incomplete. The earlier546d511 Chrome baseline reached a SERP. Native clearance and full Chrome
parity remain unresolved. Keep historical checkpoints tied to their recorded
source/binary. See [PARITY-GAPS.md](./PARITY-GAPS.md),
[FINDINGS.md](./FINDINGS.md), audio [issue27](https://github.com/miaskiewicz/turbo-surf/issues/27)
and broader [issue28](https://github.com/miaskiewicz/turbo-surf/issues/28).

Order: environment → verified SDK → all-feature binary → localhost controls → final gates → fresh comparison → evidence
review. Run timed checks sequentially; do not overlap builds, Chrome collectors or other heavy workloads.

## Required switches at a glance

| Surface | How to enable it | How to verify it |
| --- | --- | --- |
| Native V8 155 and native WindowProxy | Prepare and build through `engine-155/build.py`; ordinary Cargo builds use the stock SDK. | Actual `--build-info` engine version and `stableWindowProxy`; SDK provenance and smoke logs. |
| wreq/BoringSSL transport | MCP/NAPI `impersonate` feature, normally on by default; keep it explicit in parity builds. | `features.impersonate` and both transport-major fields; request/response evidence. |
| Chromium trust-anchor IDs | MCP `trust-anchors` or `turbo-surf-core/trust-anchors`. This is a compatibility alias for the transport profile. | `features.trustAnchors`; do not infer a separate implementation from the alias. |
| Real GPU and canvas text | `gpu-metal` and `coretext`, both off by default. MCP and NAPI install their compiled hooks. | Both feature booleans plus actual WebGL readback and text/pixel probes. |
| Wall-clock Window timers | Detection example installs `WindowTimerMode::RealTime` before hydration. Other Rust consumers must select it with `PageSessionOptions`. | `trace.windowTimerMode`, paired host/guest clock controls. Default crawler mode remains virtual. |
| Shared page interface tracing | The matrix automatically runs baseline and `interfaces` captures; `--interface-script` selects the shared tracer. | Coverage/skipped/dropped fields and baseline/interface differences. |
| Compiled JS source inventory | `--debugger`; `--native-cpu-profile` additionally enables native CPU/layout profiles. | Inventory acknowledgements, saved sources and profiler errors. |
| Native HTTP diagnostics | The detection example always writes its own `network.jsonl`. | Request/response records, final document status and capture completion. |
| Independent signed-out Chrome | `--chrome-backend cdp --chrome-incognito --chrome-path ...`. | Created nondefault context, empty initial cookie state and captured actual browser/V8 versions. |
| Genuine submission and bounded observation | `--require-native-form-navigation --no-form-fallback --observation-ms 2500`. | Correct focused query/trusted input, consent effects, real form commit and complete observation. |

These switches enable the implemented comparison surfaces; they do not close the remaining Chrome parity gaps. The
`detection_trace` example installs MCP raster/measurement/WebGL hooks through `initialize_render_hooks()`, without solver or `.env` initialization. Production `Session::new()` installs the same hooks. A custom embedder
using `turbo-surf-render` directly must install the public measurement/raster/WebGL callbacks itself; compiling a raster
feature alone does not wire those callbacks into an unrelated embedder. See
[`initialize_render_hooks`](../../rust/crates/turbo-surf-mcp/src/lib.rs) and the public setters in
[`turbo-surf-render`](../../rust/crates/turbo-surf-render/src/lib.rs).

## 1. Environment, from the repository root

The verified native profile is macOS arm64 with a real accessible Metal GPU, CoreText, Apple Command Line Tools, Python 3.12+,
Rust 1.98.0 and Node 22+. CI covers Node 20 and 22; raw CDP needs Node's native `WebSocket`, absent in Node20. Select Node22
with your installed version manager before these commands.

```sh
set -e
PARITY_ROOT="$PWD"
PARITY_NODE="$(command -v node)"
PARITY_CHROME="${TURBO_SURF_CHROME_PATH:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
PARITY_SDK="$PARITY_ROOT/.native-builds/sdk155-8059.79"
PARITY_STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
"$PARITY_NODE" -e 'if(+process.versions.node.split(".")[0]<22||typeof WebSocket!=="function")process.exit(1);console.log(process.version)'
"$PARITY_CHROME" --version
df -h "$PARITY_ROOT"
```

Check Chrome's actual reported version; a newer installed browser is a different oracle. Read the captured
`browserV8Version` as well: the product version or a configured155 identity does not prove the linked V8 version. Source/native builds and linked test
executables can consume tens of GB. There is no guaranteed free-space threshold: measure space before each build, bound jobs,
and remove only audited, consumed artifacts. Preserve SDK source, verified archive/bindings, logs, raw captures and binaries
still needed by tests.

The installed Chrome updated from155.0.8059.80 to155.0.8059.85 during October20 controls. A run spanning that update is not
a fixed-version comparison. Use an owned, version-pinned Chrome app bundle for repeatable collection, pass its executable
explicitly, and record its version/hash before and after the run. An executable copied alone is insufficient: its matching
framework/resources must remain available. Keep existing .80 controls historical; recapture rather than changing their
version fields. The native SDK remains sourced from .79 and the current native full-version identity is .80.
The verified frozen .85 capture reports V815.5.35.25; native remains15.5.35.23-rusty. These are different engine patches,
tracked in issue28, even though both report Chrome155.

```sh
parity_env() {
  env -u RUSTY_V8_ARCHIVE -u RUSTY_V8_ARCHIVE_SHA256 -u RUSTY_V8_SRC_BINDING_PATH \
    -u V8_FROM_SOURCE -u V8_FORCE_DEBUG -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
    -u NODE_OPTIONS -u NODE_PATH -u LIBCLANG_PATH -u GN_ARGS -u RUST_TEST_THREADS -u TZ \
    -u CARGO_TARGET_DIR -u TURBO_SURF_ENGINE -u TURBO_SURF_ENGINE_VERSION \
    -u TURBO_SURF_NAPI_PATH -u TURBO_SURF_JS_TRACE -u TURBO_SURF_JS_PROFILE \
    -u TURBO_SURF_TZ -u TURBO_SURF_EXPECTED_V8_VERSION -u TURBO_SURF_WEBGL_DEBUG \
    -u TURBO_SURF_HTTP_TRACE -u TURBO_SURF_HTTP_TRACE_BODY \
    PATH="$(dirname "$PARITY_NODE"):$PATH" RUSTUP_TOOLCHAIN=1.98.0 \
    CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 TURBO_SURF_CHROME_PATH="$PARITY_CHROME" \
    TURBO_SURF_BROWSER_CMD= TURBO_SURF_BROWSER_FETCH_CMD= python3 -c '
import os, sys
for key in tuple(os.environ):
    if key.startswith(("CARGO_PROFILE_RELEASE_", "CARGO_PROFILE_DEV_", "CARGO_PROFILE_TEST_")):
        os.environ.pop(key)
os.execvp(sys.argv[1], sys.argv[1:])
' "$@"
}
```

Production MCP `Session::new()` also loads the first readable `.env` among the working directory and three parents,
without replacing existing environment values. The current diagnostic example initializes hooks directly to avoid that
side effect. Rebuild before relying on this isolation; older binaries or custom session-based collectors can restore unset
tracing/timezone options. This optional preflight rejects conflicting settings without printing values:

```sh
python3 - <<'PY'
from pathlib import Path
for parent in [Path.cwd(), *list(Path.cwd().parents)[:3]]:
    candidate = parent / '.env'
    try:
        lines = candidate.read_text().splitlines()
    except OSError:
        continue
    keys = {line.strip().split('=', 1)[0].strip()
            for line in lines if '=' in line and not line.strip().startswith('#')}
    blocked = sorted(key for key in keys if key.startswith('TURBO_SURF_'))
    if blocked:
        raise SystemExit('Conflicting parity .env keys in ' + str(candidate) + ': ' + ', '.join(blocked))
    break
PY
```

Run `parity_env npm ci` if root development dependencies need installing. Raw CDP does not need Patchright. Optional legacy
`--chrome-backend patchright` requires `parity_env npm ci --prefix scripts/browser-sidecar`; it is an oracle backend, not
permission to acquire/import its cookies into native. Incognito verification requires `cdp`; both backends run headed.
Both collectors now honor the explicit executable path, then `TURBO_SURF_CHROME_PATH`; Patchright falls back to installed channel `chrome` when neither is set. Actual selected-browser controls remain required.

### Freeze a complete Chrome oracle before controls

```sh
PARITY_APP="$PARITY_ROOT/.native-builds/oracles/chrome-$PARITY_STAMP/Google Chrome.app"
parity_env "$PARITY_NODE" harness/detection/freeze-chrome.mjs \
  '/Applications/Google Chrome.app' "$PARITY_APP"
PARITY_CHROME="$PARITY_APP/Contents/MacOS/Google Chrome"
"$PARITY_CHROME" --version
```

The helper refuses an existing destination or a link escaping the bundle. It copies the complete app without Finder/resource
fork metadata, leaves the installed app untouched, verifies the copy's deep strict macOS signature, and requires source
before/after and copied file contents, modes and internal symlink targets to match. Its adjacent `.provenance.json` records
the full manifest. An incomplete receipt is a failed freeze. Keep macOS trust services accessible for signature verification;
a sandbox-only signature error is not evidence that Chrome's executable was modified.

Recheck the frozen bundle before and after a capture batch; do not assume an unchanged top-level executable proves unchanged
framework/resources:

```sh
parity_env "$PARITY_NODE" --input-type=module - "$PARITY_APP" <<'JS'
import { readFile } from 'node:fs/promises';
import { snapshotBundle } from './harness/detection/freeze-chrome.mjs';
const app = process.argv[2];
const receipt = JSON.parse(await readFile(`${app}.provenance.json`, 'utf8'));
const actual = await snapshotBundle(app);
if (receipt.complete !== true || actual.sha256 !== receipt.snapshot.sha256)
  throw new Error('Frozen Chrome changed or was never verified');
console.log(JSON.stringify({bundleSha256: actual.sha256, verifiedUnchanged: true}));
JS
```

## 2. Verify or bootstrap the separate native SDK

An existing prepared SDK is reverified by `engine-155/build.py`, including archive, bindings, actual engine, native
WindowProxy/module/code-generation controls and the async-admission transaction. Inspect its `provenance.json`,
`validated.json` and logs. Do not substitute an old155/154 archive or manually set a V8 version. For an existing prepared tree
needing provenance verification use:

```sh
parity_env python3 harness/detection/engine-155/prepare.py "$PARITY_SDK" --adopt
```

For a **new empty destination**, choose a new `PARITY_SDK` path and run preparation without `--adopt`. Never run fresh
preparation over a nonempty tree:

```sh
parity_env python3 harness/detection/engine-155/prepare.py "$PARITY_SDK" \
  --profile-file harness/detection/engine-154/profiles/155-8059.79.json
```

See [native155 bootstrap](./engine-155/README.md), [shared build/cache procedure](./engine-154/README.md), and [async
admission](./engine-155/ASYNC-ADMISSION.md). Importing a separately verified source artifact requires **all three** build
options: `--archive PATH`, `--bindings PATH`, `--archive-sha256 HASH`. The helper verifies them and still runs SDK controls.
`--jobs 2` precedes `--surf`; Cargo arguments follow `--surf`. The helper owns its SDK target/private Cargo workspace and
preserves the production lock.

The shared build helper also supports the explicit `turbo-surf-deno-*` registry
package names used by the release dependency preparation. It creates metadata
aliases inside the private Surf workspace, preserving the verified SDK source
files, original manifests and native archive. The facade still points to that
SDK's `v8-exact`; it must not resolve the stock150 backend. Keep the generated
`sdk-release-aliases/metadata-bridge.json` with the build evidence. This
compatibility path has offline guards; a new dependency release still requires
fresh stock and native155 integration tests before claiming build support.

## 3. Build the actual browserless engine with every required bridge

```sh
PARITY_MCP_FEATURES='turbo-surf-mcp/impersonate,turbo-surf-mcp/trust-anchors,turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext'
PARITY_ALL_FEATURES="$PARITY_MCP_FEATURES,turbo-surf-napi/gpu-metal,turbo-surf-napi/coretext"
parity_env python3 harness/detection/engine-155/build.py "$PARITY_SDK" --jobs 2 \
  --surf build --manifest-path rust/Cargo.toml --release -p turbo-surf-mcp \
  --example detection_trace --features "$PARITY_MCP_FEATURES"
PARITY_BIN="$PARITY_SDK/target/release/examples/detection_trace"
"$PARITY_BIN" --build-info
```

The six cross-crate switches cover MCP and NAPI; the CLI reports **four capability booleans**, not six capabilities:
`impersonate`, `trustAnchors`, `gpuMetal`, `coretext`. All must be `true`; `debugAssertions` must be `false` for live timing.
Expect actual native `15.5.35.23-rusty` and `stableWindowProxy: true`. The MCP session installs the compiled
GPU/text/measurement hooks; there is no additional `--gpu` switch. Compiled GPU support does not establish successful GPU
readback: inspect the shared WebGL probe pixels/metadata. Software fallback is not GPU parity proof.

The target identity is155; its upstream wreq TLS/H2 recipe is154. Record `reportedChromeMajor`, `transportChromeMajor`
and `transportBaseChromeMajor` separately. Matching pixels/headers/version strings is not complete browser parity.
Default builds link V8150; probe the selected addon after the gate below. Node's `process.versions.v8` and npm package
version are different identities. Keep `PARITY_BIN` pointing to the SDK155 executable.

## 4. Local controls and final gates

These first controls are localhost-only and verify consent/input/query/clock and slow-form/retry observation stages using the
explicitly selected binary:

```sh
parity_env env TURBO_SURF_DETECTION_BINARY="$PARITY_BIN" "$PARITY_NODE" --test \
  --test-concurrency=1 harness/detection/action-parity.test.mjs \
  harness/detection/native.test.mjs harness/detection/navigation-observation.test.mjs \
  harness/detection/chrome-cdp-trace.test.mjs harness/detection/collector-interruption.test.mjs
parity_env env TURBO_SURF_DETECTION_BINARY="$PARITY_BIN" TURBO_SURF_EXPECTED_V8_VERSION=15.0.245.2-rusty npm run check
parity_env "$PARITY_NODE" -e 'console.log(require("./rust/crates/turbo-surf-napi").engineVersion())'
parity_env python3 harness/detection/engine-155/build.py "$PARITY_SDK" --jobs 2 \
  --surf test --manifest-path rust/Cargo.toml --workspace --features "$PARITY_ALL_FEATURES"
parity_env python3 harness/detection/engine-155/build.py "$PARITY_SDK" --jobs 2 \
  --surf clippy --manifest-path rust/Cargo.toml --workspace --all-targets \
  --features "$PARITY_ALL_FEATURES" -- -D warnings
parity_env cargo fmt --manifest-path rust/Cargo.toml --check
```

The public TLS echo is a separate live diagnostic. Default workspace gates
record it as ignored; network errors no longer count as asserted passes. Its
request has a30-second deadline, and explicit failures remain failures. Run:

```sh
parity_env cargo test --manifest-path rust/Cargo.toml -p turbo-surf-core \
  --features impersonate --test impersonate -- --ignored \
  --exact presents_a_chrome_tls_and_http2_fingerprint --nocapture
```

For the native155 SDK, use the same test arguments after `build.py ... --surf`
instead of ordinary Cargo. The default localhost `client_hello` test still checks
actual TLS bytes. Neither control alone proves complete Chrome transport parity.

Both Chrome backends and the control suites use the explicit frozen executable.
The earlier546d511 checkpoint passes sixteen paired controls without skips, including both rootless
document cases. Full detection passes200 tests on Node22 and191 on Node20 with
nine documented browser skips, plus two Python checks each. The genuine resource
oracle retains every observed value; only collector source hashes change, with
the updated stock Rust consumer passing. The stock/native Rust workspaces pass
1,451/1,496 tests on3a77122; optimized native compile/addon consumers retain their
original4a evidence through reviewed source bridges. The later9304df9/46f4340 checkpoint passes1,464 stock/1,509 native Rust tests and206 Node22 detection controls; retain each receipt's source identity. Latest CI belongs to the
[PR checks](https://github.com/miaskiewicz/turbo-surf/pull/26/checks).

A current document may temporarily have no root or remove it itself. Both Chrome
collectors read root presence, markup and result markers atomically. Missing
roots produce empty HTML/zero results and incomplete final observation while
preserving actual loader-scoped HTTP evidence. No readiness retry, added dwell,
fake pending navigation or older-page fallback is used. Preserve the original
failed null-root capture and other dated failures in the findings.

Consent input must have an observable effect. A dispatched click alone does not
prove a save request, cookie or dismissed overlay. The earlier546d511 matrix preserves
one Chrome baseline with ineffective consent and an obscured query field;
it stops before typing. Its cause is unproved. A separate fresh Chrome baseline
saves consent and reaches nine result markers. Both native baselines save consent,
complete all six actions and navigate a real form, then receive429/zero results.
The later9304df9 matrix has two ineffective Chrome baseline consent attempts and no completed paired SERP baseline. No fresh Google comparison follows the standalone probe-selector correction. Keep failed preconditions separate from anti-bot rejection and reliability.
Native pointer input currently dispatches to the selected node at its measured center; Chrome routes the coordinates through
browser hit testing. Occlusion, clipping and `pointer-events` behavior remain tracked in issue28. Verify actual consent and
query effects rather than claiming complete pointer-routing parity from coordinates or trusted event flags alone.

If collector code changed, refresh its oracle in §6 before freezing code. Retain actual command/toolchain/source/binary
hashes, logs, failures and skips. `npm run check` includes default workspace test/strict Clippy/fmt, rebuilt stock addon,
shim/E2E and serial detection/Python checks. It does not certify the separate native155 engine or addon. Repeat the required
Node20/22 CI matrix separately; never relabel a Node20 raw-CDP skip as a pass.
Verify the actual addon loaded: a packaged `turbo-surf.<platform>.node` shadows a newly built development library. The
`engineVersion()` check distinguishes SDKs but cannot prove the addon contains current source. Retain Cargo artifact/source
hash provenance and the loaded `.node` path; on macOS ad-hoc signing changes the file hash, so compare unsigned private
copies if proving equivalence to the Cargo dylib. Likewise `--release` alone does not prove optimization level3: retain
the actual Cargo JSON artifact profile and check for profile overrides in local Cargo configuration.
The commands use the node selected by `PARITY_NODE`. For the Node20 gate, select
Node20 with your version manager and reset `PARITY_NODE="$(command -v node)"`
before repeating `npm run check`; restore Node22 and reset that variable before
running raw-CDP controls.

For native155 addon consumers, build/freeze/select the actual native library:

```sh
parity_env python3 harness/detection/engine-155/build.py "$PARITY_SDK" --jobs 2 \
  --surf build --manifest-path rust/Cargo.toml --release -p turbo-surf-napi \
  --features turbo-surf-napi/impersonate,turbo-surf-napi/gpu-metal,turbo-surf-napi/coretext,turbo-surf-core/trust-anchors
mkdir -p "$PARITY_SDK/frozen"
PARITY_ADDON="$PARITY_SDK/frozen/parity-$PARITY_STAMP.node"
test ! -e "$PARITY_ADDON" || exit 1
cp "$PARITY_SDK/target/release/libturbo_surf_napi.dylib" "$PARITY_ADDON"
codesign --force --sign - "$PARITY_ADDON"
parity_env env TURBO_SURF_NAPI_PATH="$PARITY_ADDON" "$PARITY_NODE" \
  -e 'console.log(require("./rust/crates/turbo-surf-napi").engineVersion())'
parity_env env TURBO_SURF_NAPI_PATH="$PARITY_ADDON" TURBO_SURF_EXPECTED_V8_VERSION=15.5.35.23-rusty npm run test:shim
parity_env env TURBO_SURF_NAPI_PATH="$PARITY_ADDON" npm run test:e2e
```

## 5. Fresh Google comparison and available flags

```sh
PARITY_OUT="$PARITY_ROOT/harness/detection/artifacts/chrome155-$PARITY_STAMP"
test ! -e "$PARITY_OUT" || exit 1
parity_env "$PARITY_NODE" harness/detection/run.mjs --live --binary "$PARITY_BIN" \
  --chrome-path "$PARITY_CHROME" --chrome-backend cdp --chrome-incognito \
  --repeat 2 --probes --debugger --native-cpu-profile \
  --require-native-form-navigation --no-form-fallback --observation-ms 2500 \
  --page-budget 20000 --capture-timeout 180000 --out "$PARITY_OUT"
```

Chrome gets a fresh temporary profile plus a disposable empty nondefault context; require `browserContext.nonDefaultContext`
and `createdContextVerified` in its trace. Native uses a fresh independent jar. The example does not call a sidecar, read an
ENID cache or import Chrome cookies; the wrapper also clears sidecar commands for every subprocess. Do not add cookie/profile
imports or reuse a signed-in UI session. Consent dispatch is conditional; verify its204/cookie effects before search.

| Actual CLI option | Meaning/default |
| --- | --- |
| `--live`, `--binary`, `--out` | Explicit live opt-in, native executable, private capture directory. |
| `--url`, `--query`, `--selector` | Google homepage, `rust lang`, shared nonhidden `q` control by default. |
| `--dismiss-selector` | Google `#W0wltc`; empty string disables dismissal. |
| `--chrome-path`, `--chrome-backend`, `--chrome-incognito` | Explicit executable in both CDP and Patchright; `cdp` default; verified Incognito context requires CDP. |
| `--repeat`, `--native-first` | 1..10 repeats; reverse Chrome/native order within each pair. |
| `--probes`, repeatable `--probe PATH` | Shared post-verdict probes and separate localhost pair; hashes retained. |
| `--debugger`, `--native-cpu-profile` | Instrumented source inventory; native CPU flag implies debugger. |
| `--wait`, `--observation-ms` | Dwell2500ms; fixed Enter window defaults to `--wait`; each0..60000. |
| `--page-budget`, `--capture-timeout` | 20000ms/180000ms; maxima120000/600000. Do not relax tests to obtain a pass. |
| `--width`, `--height`, `--tz` | 1280×800; `America/Los_Angeles`. Outer height is inner+88. |
| `--steps PATH`, `--no-interaction` | Replace steps with validated JSON; or no default interaction. |
| `--ready-selector`, `--success-selector`, `--interface-script` | Search readiness (empty for custom/no-interaction plans); organic `h3`; tracer path. |
| `--require-native-form-navigation`, `--no-form-fallback` | Require a real native form commit; prohibit serialized GET fallback. |
| `--allow-reduced` | Diagnostic exception to feature checks; unsuitable for all-feature parity claims. |

Language/locale (`en-US`, `['en-US','en']`),1920×1080 screen and scale2 are fixed runner configuration, not invented CLI
flags. Verify actual snapshot values; native languages must match its ordered HTTP profile. Extension disabling is a direct
collector JSON setting `disableExtensions: true`, not a runner CLI flag. This setting is restricted to authored localhost
controls; the collector rejects it for Google or other remote URLs. The matrix sets native `TURBO_SURF_TZ` from `--tz`;
set it explicitly when invoking the native example directly. The detection example selects `WindowTimerMode::RealTime` before
hydration/frames/form replacements; there is no `--real-time` flag. Normal crawler constructors remain virtual. The host API's
one-shot `initial_hydration_budget_ms` can bound a redirect open without reducing later configured operation budgets. The
observation deadline starts before the last Enter, never renews per redirect or stops early on results. Actual overrun,
pending/failed navigation and incomplete snapshots remain explicit; equal configured windows do not prove equal elapsed time.
In the earlier timing-boundary capture, a20ms timer was scheduled after document startup, not immediately after Enter.
Measured startup put its deadline beyond a100ms observation window in either browser. Retain the short boundary diagnostic and use the separately declared1000ms
positive control; never force a future timer to fire or erase incomplete evidence to manufacture matching terminal pages.
For a direct collector JSON config, `readyTimeoutMs` defaults to5000 and `inputCadenceMs` to110; the matrix uses the shared
110ms minimum cadence and does not expose either as a CLI flag. Review the per-step selected value and focus, trusted
events and `interaction.typed` evidence before attributing a challenge to the engine. A completed input call with the wrong
value or BODY focus is a failed action, even if the collector reaches a final snapshot.

## 6. Profiles, genuine oracle refresh and evidence

For direct Rust/MCP embedding, the lower-level diagnostic environment variables are:

| Environment variable | Actual effect |
| --- | --- |
| `TURBO_SURF_TZ=America/Los_Angeles` | Pins the native process timezone before V8 platform initialization; set before launching the process. |
| `TURBO_SURF_HTTP_TRACE=/absolute/private/network.jsonl` | Appends application-layer HTTP diagnostics; no packet-level TLS/HTTP2 transcript. |
| `TURBO_SURF_JS_TRACE=/absolute/private/native-js` | Enables native Inspector source inventory for new page runtimes. Presence enables it; unset it to disable. |
| `TURBO_SURF_JS_PROFILE=1` | Enables native CPU/layout profiling when Inspector inventory is active. Other values do not enable it. |
| `TURBO_SURF_NAPI_PATH=/absolute/frozen/addon.node` | Selects that exact addon; otherwise the loader prefers a packaged addon, then release, then debug. |
| `TURBO_SURF_EXPECTED_V8_VERSION=15.5.35.23-rusty` | Test expectation for the shim's engine assertion; it does not select or upgrade the loaded engine. |
| `TURBO_SURF_CHROME_PATH=/absolute/Chrome` | Executable for both Chrome collectors, overridden by explicit `--chrome-path`. |
| `TURBO_SURF_DETECTION_TEST_ARTIFACTS=/absolute/private/test-captures` | Retains successful interruption-test evidence as well as failures; absent uses temporary directories and removes successful fixtures. |
| `TURBO_SURF_WEBGL_DEBUG=1` | Optional WebGL bridge diagnostics; presence enables logging. It does not enable the GPU backend. |

The example owns its HTTP/inventory/profile destinations: config `debugger` and `nativeCpuProfile` set or clear the latter
two variables, so externally setting them does not override its config. Use runner flags for the example. The SDK helper
owns `TURBO_SURF_ENGINE`, `TURBO_SURF_ENGINE_VERSION` and matching `RUSTY_V8_*` paths after verification; manually exporting
these is not an engine installation procedure. Captured-page replay and GC heap-snapshot settings are focused debugging
tools, not required parity switches; their separate procedures are in the [detection README](./README.md).

Interface wrappers change observable function identity and cover selected interfaces with explicit skip/drop limits. The
baseline still uses CDP and emulation. Inspector inventories record compiled sources; HTTP diagnostics record application
traffic. None alone proves that every JavaScript operation or every wire-level transport difference was observed.

`--native-cpu-profile` writes private Inspector `cpu-profile.json` and host `layout-profile.json` under native isolate
inventories, initial hydration only. Check profiler acknowledgements/errors; host spans overlap and cannot be summed. Cold
font initialization precedes guest profiling. There is no runner `--host-profile` flag; external OS sampling is a separate
diagnostic, with its own process/timing proof.

For font startup, the pinned turbo-html2pdf producer includes
[`font_startup.rs`](https://github.com/miaskiewicz/turbo-html2pdf/blob/8f0aa139490573e7ef7a27a4c392195f265f900f/crates/turbo-html2pdf-core/examples/font_startup.rs).
From that producer checkout, build with `cargo build --release -p turbo-html2pdf-core --example font_startup
--message-format=json-render-diagnostics`, then run the actual Cargo-reported executable directly in a fresh process.
On macOS, wrap the executable with `/usr/bin/time -l` to retain peak RSS without including compilation. Its directory
arguments select font files for diagnostic fingerprints; they do not change the registry's normal discovery policy.
Compare the same installed-file manifest, face count, selection/index/program hashes, metrics and glyph rows. Registry
initialization timings exclude later fingerprinting; whole-process peak RSS includes it. Process-cold initialization
does not establish cold OS disk caches or whole-browser startup performance.

For a saved profile, use `node harness/detection/cpu-profile-summary.mjs /absolute/private/cpu-profile.json`.
The helper aggregates sampled self time across recursive call stacks. It preserves Inspector script IDs and zero-based
line/column locations; resolve those against the same capture's `inventory.json`. Sample counts are not invocation counts,
and inclusive host layout stages must not be added together. URLs are omitted, but function names can contain page data:
keep this analysis private and review any later publication separately.

To isolate native homepage startup from interface-wrapper overhead, copy a capture config into a fresh private directory
and explicitly set `debugger: true`, `nativeCpuProfile: true`, `interfaces: false`, `probes: []`, `steps: []`,
`dismissSelector: ""`, `readySelector: ""`, `waitMs: 0`, `observationMs: 0` and `requireNativeFormNavigation: false`.
Keep the original `pageBudgetMs`, identity and native feature settings. Invoke the same optimized `detection_trace` executable
with that config and a fresh output directory. This is a homepage-only diagnostic with no consent or search actions;
it is not a successful Google flow or SERP acceptance measurement. Record the binary's actual build source and hash rather
than attributing a preserved executable to uncompiled checkout edits.

The October21 rebuilt diagnostic and subsequent full native gate select the same
optimized executable bytes. Its two fresh Google baselines complete homepage
hydration; the interface-instrumented passes still exhaust the original budget.
Keep those passes separate. Four current Chrome flow captures dispatch consent
clicks without a save request and stop before query input. A dispatch acknowledgement
does not prove the intended element handled the click or consent succeeded.
The two standalone Chrome probes separately fail inherited search readiness on
a blank fixture; the probe configuration repair does not change those saved failures.
The runner now clears `readySelector` for that probe-only fixture; interactive
captures retain their configured selector. Two offline runner controls per
Node20/22, full Node22 detection206+Python2 and a genuine local seven-process
matrix pass after the correction. This local control is not a new Google run.

CI disables dev/test DWARF with `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0` to limit accumulated artifacts. These settings do not
disable assertions or overflow checks and do not alter the release profile.
Keep native and stock debug caches sequential; remove a consumed cache only
after all of its tests, addon consumers and artifact queries have closed. Retain
the measured executables, source/archive caches and before/after proof files.

After collector/helper changes, recapture this genuine oracle **before final gates/capture**:

```sh
mkdir -p "$PARITY_ROOT/.native-builds/recovery"
PARITY_ORACLE="$PARITY_ROOT/.native-builds/recovery/resource-order-$PARITY_STAMP.json"
parity_env "$PARITY_NODE" harness/detection/resource-scan-order-chrome.mjs "$PARITY_ORACLE"
```

Review actual results/errors/version/source hashes before intentionally replacing
`harness/detection/fixtures/resource-scan-order-chrome155.json`. Once accepted:

```sh
cp "$PARITY_ORACLE" harness/detection/fixtures/resource-scan-order-chrome155.json
parity_env "$PARITY_NODE" --test harness/detection/resource-scan-order-chrome.test.mjs
parity_env cargo test --manifest-path rust/Cargo.toml -p turbo-surf-render --test resource_scan_order
```

Never edit hash fields to impersonate a recapture. Other controls have their own collectors in the [detection
README](./README.md). Recompute summaries when needed:

```sh
parity_env "$PARITY_NODE" harness/detection/compare.mjs "$PARITY_OUT/repeat-0"
parity_env "$PARITY_NODE" harness/detection/summarize-run.mjs "$PARITY_OUT" "$PARITY_OUT/summary-candidate.json"
```

Raw traces, HTML, network bodies and profiles can contain credentials: preserve them privately. `manifest.json` freezes the
binary, buildInfo, config/source hashes, order and process outcomes. `comparison.json` separates baseline/interfaces, action
completion, observer scope and failures. Public summary schema6 requires typed, measured process outcomes, real-time mode
and a complete fixed observation for native acceptance. An abort, timeout, output failure or termination signal cannot
count as success even with exit0; legacy missing process fields remain explicitly unmeasured. Publish only reviewed allowlisted evidence, update
changelog/findings/parity records, and keep deferred work linked to issues.
