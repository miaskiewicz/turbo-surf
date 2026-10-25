# Re-measuring API-INVENTORY.json against Chrome 156

This is how to regenerate [API-INVENTORY.json](./API-INVENTORY.json) once a batch of #28
parity work has merged. Run it from the repository root, on macOS arm64 with Node 22+.
It covers the same 75 interfaces as the tracked snapshot, plus an optional expanded scope.
Everything runs on localhost: no search engine, no imported cookies.

## Which configuration to use

The tracked snapshot (2026-10-21) pairs **Chrome 155.0.8059.85 / V8 15.5.35.25** with
the **native155** engine (`15.5.35.23-rusty`, `stableWindowProxy: true`). That engine
needs a source-built SDK (`.native-builds/sdk155-8059.79`, see
[engine-155](./engine-155/README.md)). No prepared SDK exists in any checkout or worktree
on this machine. The pinned sources are still reachable, so you can rebuild it with
`engine-155/prepare.py` on a new, empty destination and then `build.py`. That means a
full V8 compile and multi-GB toolchain downloads. Once the SDK exists, use step 3b.

**When no native155 SDK is available**, use the stock build with every feature: rusty V8
`15.0.245.2-rusty`, `stableWindowProxy: false`, `impersonate`, `trustAnchors`,
`gpuMetal` and `coretext`. The 2026-10-25 trial below shows that this collector measures
no engine-dependent difference on the batch-1 head:

- The full Chrome result is byte-identical between 155.0.8059.85 and 156.0.8078.47.
- The native155 and stock150 results differ only in the ten constructors that batch-1
  changed.

That evidence covers this collector only. It is not general engine equivalence.

**Labeling rules.** `api-inventory.mjs` copies `build.engine`, `chromeVersion` and
`chromeV8Version` from the capture. Never edit these fields. Never describe a Chrome 156
capture as Chrome 155, or a stock150 capture as native155. When you promote a snapshot,
the prose in [PARITY-GAPS.md](./PARITY-GAPS.md) must state:

- the oracle change (155.0.8059.85 / V8 15.5.35.25 → 156.0.8078.47 / V8 15.6.75.19)
- the engine (stock150 or native155)
- that comparisons to the earlier snapshot cross both changes

## Commands

```sh
set -e
ROOT="$PWD"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
SCRATCH="${SCRATCH:-$ROOT/harness/detection/artifacts/inventory-$STAMP}"   # ignored
clean() { env -u TURBO_SURF_ENGINE -u TURBO_SURF_ENGINE_VERSION -u TURBO_SURF_JS_TRACE \
  -u TURBO_SURF_JS_PROFILE -u TURBO_SURF_HTTP_TRACE -u TURBO_SURF_HTTP_TRACE_BODY \
  -u TURBO_SURF_TZ -u TURBO_SURF_NAPI_PATH -u NODE_OPTIONS -u RUSTFLAGS -u CARGO_TARGET_DIR \
  -u RUSTY_V8_ARCHIVE -u RUSTY_V8_SRC_BINDING_PATH -u V8_FROM_SOURCE RUSTUP_TOOLCHAIN=1.98.0 "$@"; }

# 1. Freeze the installed Chrome (here it is under "User Apps"), then check its identity.
SRC_APP="/Applications/User Apps/Google Chrome.app"
"$SRC_APP/Contents/MacOS/Google Chrome" --version          # Google Chrome 156.0.8078.47
APP="$ROOT/.native-builds/oracles/chrome156-8078.47-$STAMP/Google Chrome.app"
node harness/detection/freeze-chrome.mjs "$SRC_APP" "$APP"
ORACLE="$APP/Contents/MacOS/Google Chrome"

# 2. Release build with all four capability features. The capture freezes its own copy.
clean CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=8 cargo build --manifest-path rust/Cargo.toml \
  --release -p turbo-surf-mcp --example detection_trace \
  --features turbo-surf-mcp/impersonate,turbo-surf-mcp/trust-anchors,turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext
BIN="$ROOT/rust/target/release/examples/detection_trace"
"$BIN" --build-info   # all four features true, debugAssertions false; note v8Version

# 3b. (native155 only) Build the same example through the SDK instead, then use that binary:
#   python3 harness/detection/engine-155/build.py "$SDK" --jobs 2 --surf build \
#     --manifest-path rust/Cargo.toml --release -p turbo-surf-mcp --example detection_trace \
#     --features turbo-surf-mcp/impersonate,turbo-surf-mcp/trust-anchors,turbo-surf-mcp/gpu-metal,turbo-surf-mcp/coretext
#   BIN="$SDK/target/release/examples/detection_trace"   # expect 15.5.35.23-rusty, stableWindowProxy true

# 4. Paired captures. The same scope uses the unchanged 75-interface collector
#    (sha256 21d5aafb…); the expanded scope is api-shape-interfaces.js.
clean TURBO_SURF_CHROME_PATH="$ORACLE" node harness/detection/api-shape.mjs --run \
  --binary "$BIN" --out "$SCRATCH/same-scope"
clean TURBO_SURF_CHROME_PATH="$ORACLE" node harness/detection/api-shape.mjs --run \
  --binary "$BIN" --probe harness/detection/api-shape-interfaces.js --out "$SCRATCH/expanded"

# 5. Candidate inventories (written to scratch) and a same-scope diff against the tracked snapshot.
node harness/detection/api-inventory.mjs "$SCRATCH/same-scope/comparison.json" "$SCRATCH/API-INVENTORY.json"
node harness/detection/api-inventory.mjs "$SCRATCH/expanded/comparison.json" "$SCRATCH/API-INVENTORY.expanded.json"
node harness/detection/api-inventory-diff.mjs harness/detection/API-INVENTORY.json \
  "$SCRATCH/API-INVENTORY.json" > "$SCRATCH/diff.json"

# 6. Confirm the frozen oracle did not change during the batch (runbook §1 check).
# 7. Promote after review only: copy $SCRATCH/API-INVENTORY.json over the tracked file and
#    update the PARITY-GAPS "Fresh own-member inventory" prose with the labels above.
```

The raw capture directories keep a copy of the executable, both traces, network logs
and `home.html`. Keep them under the ignored `artifacts/` directory or a private
scratch path, and never commit them. The candidate inventory contains no tokens, only
descriptors and member names.

`api-inventory-diff.mjs` compares only interfaces that both snapshots measured, so a
larger scope can never count as resolved members. It also compares global, method and
contract paths only when the two collector hashes match. Its `identityChanges` field
lists every changed oracle, engine, feature, collector or binary identity.

## Trial on the batch-1 head (4270a4c, 2026-10-25)

Configuration: stock150 (`15.0.245.2-rusty`, `stableWindowProxy: false`), all four
features, binary `d0b3ab6f…`, frozen oracle Chrome 156.0.8078.47 / V8 15.6.75.19
(bundle `465e3ecf…`, still unchanged after the run). Timing:

| Step | Time |
| --- | --- |
| Release build (cold worktree target) | 191 s |
| Freeze Chrome | 15 s |
| Each paired capture | 3.5 s |
| Inventory and diff | under 1 s |

Two consecutive runs gave identical totals.

Totals across the same 75 interfaces:

| | Tracked (native155 / Chrome 155) | Trial (stock150 / Chrome 156) |
| --- | --- | --- |
| Original-four missing | 64 (29 / 29 / 4 / 2) | 35 (Document 14 / Element 15 / HTMLElement 4 / Canvas 2) |
| Expanded missing | 142 | 52 (90 resolved, 0 introduced) |
| Difference paths | 374 | 264 (110 resolved, 0 introduced, 17 key-order paths changed value) |

Resolved missing members, per interface:

| Interface | Resolved | Still missing |
| --- | --- | --- |
| Document | 15 | 14 |
| Element | 14 | 15 |
| HTMLInputElement | 16 | 6 |
| ShadowRoot | 11 | 4 |
| HTMLSelectElement | 8 | 1 (`showPicker`) |
| HTMLTextAreaElement | 8 | 1 (`createValueRange`) |
| HTMLMeterElement | 6 | 0 |
| HTMLOutputElement | 5 | 0 |
| HTMLButtonElement | 4 | 1 (`interestForElement`) |
| HTMLProgressElement | 3 | 0 |

Document also resolves 17 descriptor paths on `exitFullscreen` and the
`fullscreenElement` setter. These interfaces now report native-only extra members:

- HTMLLabelElement: 13
- HTMLSelectElement: 7
- HTMLTextAreaElement: 6

The extras are input-style reflections such as `accept`, `min`, `max` and `pattern`,
and they predate batch-1. Treat them as an open gap.

The expanded scope measures 154 interfaces, with 367 members missing on measured
interfaces. Of the newly added interfaces, these have zero missing, extra or changed
members:

- HTMLDialogElement, HTMLAllCollection, HTMLSlotElement, HTMLAudioElement
- CommandEvent, ToggleEvent and the UI/Pointer/Keyboard/Input event family
- DOMTokenList, NamedNodeMap, StyleSheetList, Permissions

Sanitizer is present but has 12 missing members. Chrome exposes the following
interfaces and native does not:

- Animation, AnimationEffect, AnimationTimeline, DocumentTimeline, KeyframeEffect,
  ViewTransition
- DataTransfer, FileList, EditContext, Selection, VisualViewport, ScreenOrientation
- StylePropertyMap, StylePropertyMapReadOnly
- History, Location, HTMLOptionsCollection
