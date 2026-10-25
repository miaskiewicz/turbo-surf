# Re-versioning turbo-surf to a new Chrome release

This handbook is for whoever (human or agent) moves turbo-surf's emulated browser
identity to a new Chrome stable. That happens about 20 times a year. The work is
data-driven: one command measures the new Chrome, diffs it against the current
identity and against turbo-surf, and rewrites the registry. A person is needed only
when a delta can't be expressed as data. The tool stops and says so when that
happens.

Contents:

1. [What the identity is made of](#1-what-the-identity-is-made-of)
2. [The battery: what each check measures](#2-the-battery-what-each-check-measures)
3. [Bump procedure](#3-bump-procedure)
4. [CI / scheduled mode (`--check`)](#4-ci--scheduled-mode---check)
5. [Handling deltas](#5-handling-deltas)
6. [Never fake](#6-never-fake)
7. [Tests, fixtures and gates](#7-tests-fixtures-and-gates)
8. [Failure modes](#8-failure-modes)
9. [Worked example: 155 → 156](#9-worked-example-155--156)

Files:

| Path | Role |
| --- | --- |
| `harness/detection/reversion-chrome.mjs` | The one entry point: detect, freeze, battery, diff, write, report |
| `harness/detection/chrome-identity-battery.mjs` | The battery collector (`--chrome` or `--native`) |
| `harness/detection/identity-server.mjs` | Loopback HTTPS/H2/H1 servers with ClientHello and H2-frame taps |
| `harness/detection/identity-wire.mjs` | ClientHello, JA3/JA3N/JA4, H2 frame and Akamai parsers (pure) |
| `harness/detection/identity-diff.mjs` | Parity and version diff, classification, registry derivation (pure) |
| `harness/detection/chrome-identity-probe.js` | The JS identity probe (hash-pinned) |
| `harness/detection/chrome-identity-known-gaps.json` | Every tolerated turbo-surf/Chrome difference, with its reason |
| `harness/detection/chrome-identity-import.mjs` | Builds a partial fixture from pre-battery captures (used once, for 155) |
| `harness/detection/chrome-identity.test.mjs` | Offline unit tests for all of the above |
| `harness/detection/fixtures/chrome-identity-<major>.json` | One measured Chrome identity per release |
| `harness/detection/TRANSPORT-<major>.json` (+ `fixtures/transport-*<major>-client-hello.bin`) | Chrome vs turbo-surf TLS/H2 record |
| `rust/crates/turbo-surf-core/src/browser_releases.json` | The registry (compiled by `turbo-surf-core/build.rs`) |
| `rust/crates/turbo-surf-mcp/examples/identity_battery.rs` | turbo-surf's side of the battery (no-JS and render tiers) |

## 1. What the identity is made of

### The registry

`rust/crates/turbo-surf-core/src/browser_releases.json` is the only place a Chrome
version lives. `turbo-surf-core/build.rs` compiles it into `browser::RELEASES`
(newest first) and `browser::DEFAULT_MAJOR`. The build fails with a clear
`src/browser_releases.json: …` panic if the file is malformed. That covers releases
out of order, a `fullVersion` that doesn't start with its major, or a default with
no entry. Each release has these fields:

| JSON field | Rust field | Used by | Measured from |
| --- | --- | --- | --- |
| `major` | `major` | UA `Chrome/<major>.0.0.0`, `sec-ch-ua`, GREASE seed, seed pool | Browser.getVersion |
| `fullVersion` | `full_version` | `Sec-CH-UA-Full-Version(-List)`, `uaFullVersion`, `fullVersionList` | `getHighEntropyValues` |
| `headerOrder` | `header_order` | client-level wreq `OrigHeaderMap` (navigation order, with the `cookie` slot) | cold top-level navigation |
| `navigationAccept` | `navigation_accept` | `Accept` for documents and iframes | cold navigation |
| `defaultDeviceMemory` | `default_device_memory` | `navigator.deviceMemory`, `Device-Memory` hints | `navigator.deviceMemory` on the measuring Mac |
| `transportBaseMajor` | `transport_base_major` | `net::emulate` picks `wreq_util::Profile::Chrome<N>` | not measured: the recipe whose hello/frames matched |
| `requestOrders` | `request_orders` | per-request `OrigHeaderMap` in `net::send` | every battery request, merged per class |
| `accept` | `destination_accept` | `request_context` `Accept` per `Sec-Fetch-Dest` | cold subresources |
| `priority` | `destination_priority` | `request_context` `Priority` per `Sec-Fetch-Dest` | cold subresources |
| `evidence` | (not compiled) | provenance: fixture path, browser, V8, bundle hash | the run |

The following are not stored as data because they derive from `major` in code:

- the UA template (`fingerprint::default_profile`, `profile_at`)
- the GREASE brand algorithm (`fingerprint::brand_versions`, mirrored by
  `brandVersions` in identity-diff.mjs)
- the `Sec-CH-UA-*` value formats (`request_context::hint_value`)

The TLS/H2 recipe isn't stored here either. It belongs to the pinned wreq fork.

**Request classes.** `browser::request_class(method, headers)` mirrors `requestClass`
in identity-diff.mjs. It picks one of:

- `navigate`, `no-cors`, `cors-resource` (CORS script, style or font), `cors` (fetch
  or XHR), `preflight`
- the same with `-hinted` appended when any high-entropy client hint is present

Chrome orders headers differently in each class, and differently again once hints
are negotiated. Each class order is a merge of every measured request in that class,
so every sample's relative order is kept. Names keep Chrome's HTTP/1.1 spelling
(`User-Agent`, `Sec-Fetch-Site`, lowercase `sec-ch-ua`). wreq uses the spelling only
on HTTP/1.1 and lowercases it on H2. Releases measured before the battery (155 and
older) have empty `requestOrders`/`accept`/`priority` and fall back to the earlier
behaviour: the navigation order for everything, and request_context's built-in tables.

### Keeping the wire and the render tier coherent

- **Wire (no-JS tier).** `fingerprint::default_profile()` reads the default release.
  `net::build_headers` and `net::emulate` use that profile. `net::send` applies the
  class order per request. `request_context` reads `accept_for`/`priority_for`.
  `ClientHints::headers` renders the hints from the same profile.
- **Render tier.** `op_fingerprint` in `turbo-surf-render/src/runtime.rs` passes
  `defaultMajor`, `fullVersion`, `defaultUserAgent`, `architecture` and
  `platformVersion` from the same `default_profile()`. `op_browser_brands` and
  `op_browser_device_memory` read the registry. The navigator JS takes its major
  from the UA string, so `userAgentData` and `userAgent` always agree. Version
  numbers no longer appear in JS literals.
- **MCP.** `set_fingerprint` overrides go on top of these defaults. `{}` resets them.

### What the registry does not change

- **The JS engine.** Stock V8 is 15.0.245.2, the native155 SDK is 15.5. Chrome 156
  ships V8 15.6.75.19. Engine work belongs to `harness/detection/engine/`
  (`reversion_v8.py`, owned separately). See [§6](#6-never-fake).
- **The TLS/H2 recipe.** That is the wreq fork pinned in `rust/Cargo.toml` and
  `rust/release-deps/transport`.

## 2. The battery: what each check measures

`chrome-identity-battery.mjs` starts these loopback origins (`identity-server.mjs`).
Each TLS listener captures the exact ClientHello and taps the decrypted
client→server HTTP/2 frames before Node's own HTTP/2 server sees them.

| Label | Origin | Protocol |
| --- | --- | --- |
| A | `https://127.0.0.1:P1` | H2 (ALPN h2, http/1.1) — the battery page |
| AX | `https://localhost:P1` | same listener, a cross-site origin for CORS |
| B | `https://127.0.0.1:P2` | TLS, HTTP/1.1 only |
| C | `https://127.0.0.1:P3` | H2 — client-hint opt-in origin |
| CX | `https://localhost:P3` | H2 — Critical-CH origin |
| D | `http://127.0.0.1:P4` | plain HTTP/1.1 |

Ports, the per-run tier prefix (`/t/<tier>/`) and host names are labelled
(`<A>`, `<A-host>`, `/t/~/`), so two runs compare structurally. The certificate is
ephemeral. Chrome trusts it only via `--ignore-certificate-errors-spki-list=<pin>`.
turbo-surf trusts it only via `ClientHints::with_client_builder(client_builder_with_trust)`.
That flag changes trust, not identity.

### Request kinds (`requests[]`)

| Kind | What it is | Tier(s) on the native side |
| --- | --- | --- |
| `nav` | cold top-level navigation to A (sets a `Secure` first-party cookie) | nojs, render |
| `style`, `script`, `module`, `img`, `iframe`, `font-style`, `font` | subresources; the web font lives in the iframe | render |
| `fetch`, `fetch-post`, `xhr` | same-origin CORS-mode requests | render |
| `fetch-cors`, `fetch-preflight` | cross-site GET, and an `OPTIONS` + `PUT` pair with author headers | render |
| `form-post` | scripted POST submission into a named iframe | render |
| `click-nav` | trusted click → `location.assign` (user-activated, same-origin, with cookie) | render |
| `h1-nav`, `http-nav` | typed navigations to B and D (HTTP/1.1 header casing, `Host`/`Connection`) | nojs |
| `ch-accept` → `ch-sub`, `ch-next` | `Accept-CH` opt-in, then a hinted subresource and a hinted navigation | nojs + render |
| `critical` (×2) | `Accept-CH` + `Critical-CH` → Chrome restarts the request with the hint | nojs |
| `favicon` | Chrome's UI fetch of `/favicon.ico`; timing-dependent, keyed by origin | Chrome only |

For every request the fixture keeps the full header list. It records order, the
original casing, values, protocol, connection id and the HEADERS-frame stream
priority.

### Connections (`connections[]`)

Each connection records:

- the full ClientHello: ciphers, ordered extensions, groups (`X25519MLKEM768`, …),
  key-share groups, signature algorithms, ALPN, the ALPS codepoint (17613 is the
  new one, 17513 the old), certificate compression, ECH, trust anchors (51764),
  `rawBase64`
- JA3, JA3N (sorted extensions) and JA4
- for H2, the frame preamble (SETTINGS order and values, WINDOW_UPDATE, PRIORITY
  frames), pseudo-header order and the Akamai string

The diff uses the first fresh (non-PSK) connection per origin and ALPN.

### JS identity (`js`, from chrome-identity-probe.js)

- `navigator.*`: the UA family, platform, vendor, languages, memory, plugins and
  MIME types, own keys
- `userAgentData`: brands, `toJSON()`, every high-entropy hint
- `screen`
- `intl`: resolved options, `supportedValuesOf` counts, fixed formatting samples
  (ICU data)
- `engine`: the Error stack shape, `stackTraceLimit`, the V8 message wording for
  seven errors, `Function.prototype.toString` forms
- `features`: typeof for 25 recent builtins, `CSS.supports` for 20 recent CSS
  features, `window.chrome` keys, own names of eight prototypes, and the full
  `Object.getOwnPropertyNames(globalThis)` list

Page state lives under `Symbol.for(...)` keys, so the battery adds no globals.

### Reading a diff

Every delta is an object: `{ area, type, tier?, kind?, key?, header?|path?, expected, actual, class|action, gap? }`.

- `area`: `request`, `transport` or `js`.
- `type` (requests): `request-missing`, `request-extra`, `protocol`,
  `pseudo-order`, `header-order` (the relative order of the shared names
  differs), `header-missing`, `header-extra`, `header-value`, `header-casing`
  (HTTP/1.1 only), `stream-priority`.
- `type` (transport/js): `value`, or `order` for a list that has the same members
  in a different order. List deltas carry `added` and `removed`.
- **Parity mode** (`diffParity(chrome, native, {knownGaps})`) sets `class`:
  - `tell`: an unexplained difference. This fails `--check` (exit 12).
  - `environment`: depends on the measuring machine or network. That means
    `downlink`, `rtt`, `ect`, `sec-ch-viewport-width`, `hardwareConcurrency`,
    `screen.*`, the time zone and the connection type.
  - `collector`: an artifact of how Chrome was driven. CDP remote debugging makes
    `navigator.webdriver` true; turbo-surf must report `false`.
  - `known-gap`: matches a rule in `chrome-identity-known-gaps.json` (`gap` names
    it). For list deltas the rule's `allowAdded`/`allowRemoved` pin the exact
    tolerated members, so a new extra global is still a tell.
- **Version mode** (`diffVersions(old, new)`) normalizes every spelling of each
  fixture's own version first: `Chrome/{MAJOR}.0.0.0`, `{FULL}`, `{BRANDS}`,
  `{FULL_BRANDS}`, and brand arrays that follow the GREASE algorithm. What remains
  is the release delta, and each one gets an `action`:
  - `registry`: becomes data (header orders, `accept`/`priority`, `deviceMemory`)
  - `engine`: a V8/ICU/binding delta, recorded for the engine step
  - `human`: [§5](#5-handling-deltas)
  - `info`: environment, collector, favicon or stream-priority noise
- A **partial** baseline (`"partial": true`, from chrome-identity-import.mjs)
  constrains only what it recorded.

## 3. Bump procedure

Prerequisites: macOS arm64, Node 22+, `npm ci` at the root, a Rust toolchain (the
pinned 1.98.0), cmake, and the new Chrome stable installed. Run everything from the
repository root of your worktree.

```sh
# 0. Which Chrome is installed, and is it newer than the registry?
plutil -extract CFBundleShortVersionString raw "/Applications/Google Chrome.app/Contents/Info.plist"
node harness/detection/reversion-chrome.mjs --chrome "/Applications/Google Chrome.app" --check --offline
#    exit 10 = a newer stable exists; 0 = registry is current.

# 1. The whole flow (about 1 min warm, 4–5 min with a cold example build).
node harness/detection/reversion-chrome.mjs --chrome "/Applications/Google Chrome.app"
```

Step 1 does the following, in order. It is safe to re-run.

1. Reads the version from `Info.plist` (or `--version` for a bare executable).
2. Freezes the app to `.native-builds/oracles/chrome<major>-<build>.<patch>/` with
   `freeze-chrome.mjs`. A re-run reuses the copy when the source bundle hash still
   matches the provenance receipt. It refuses if the receipt doesn't match.
3. Runs the battery against the frozen executable and writes
   `fixtures/chrome-identity-<major>.json`. Re-measuring the same build first
   reports drift against the saved fixture.
4. Diffs the current default fixture against the new one (version mode) and
   collects blockers.
5. Runs the optional engine hook (see below).
6. **Stops with exit 3** if any blocker exists. The registry is not touched; read
   the printed reasons and [§5](#5-handling-deltas).
7. Derives the release entry (`measuredRelease`). It inserts or replaces the entry,
   sets `defaultMajor` to the newest measured major and writes `browser_releases.json`.
8. Builds `identity_battery` (`cargo build -p turbo-surf-mcp --example
   identity_battery`), runs the battery against turbo-surf and diffs in parity mode.
9. Writes `TRANSPORT-<major>.json` and the two raw ClientHello `.bin` files, then
   formats the JSON with biome.
10. Prints the delta report and saves it to
    `harness/detection/artifacts/reversion-<major>-<stamp>.json` (an ignored path).
    The exit code is 12 if parity has unexplained tells.

Options:

| Option | Effect |
| --- | --- |
| `--native <path>` | Reuse a built `identity_battery`. It must already contain the new registry: build.rs re-runs on JSON changes, so rebuild after step 7. |
| `--skip-native` | Skip turbo-surf parity. |
| `--fixture-only` | Stop after step 5. |
| `--skip-engine` | Skip the engine hook. |
| `--engine-mode --resolve\|--check` | Mode passed to the engine step (default `--resolve`, read-only). |

**Engine hook (optional, marked in `engineStep`).** If
`harness/detection/engine/reversion_v8.py` exists, the tool runs `python3
reversion_v8.py --chrome-version <x> <mode>`. The script's JSON goes into
`report.engine`, and `report.engineIdentity` compares Chrome's V8 with the native
build's `v8Version`. If the script is absent, the report says `absent`. If it fails,
the report says `failed` and includes stderr. Neither case blocks the identity
bump: the engine gap stays a recorded known gap until the engine moves.

Then the human steps:

```sh
# 2. Review the data diff. Every changed field must match the report.
git diff rust/crates/turbo-surf-core/src/browser_releases.json harness/detection/TRANSPORT-*.json

# 3. Gates (see §7). The tests read the new default from the registry and fixture.
node --test harness/detection/chrome-identity.test.mjs
(cd rust && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check)
npx oxlint && node_modules/.bin/biome format .
npm run test:detection

# 4. Prose that names the current version (not data): README.md "Fingerprint" and
#    "set_fingerprint" sections, PARITY-GAPS.md if a gap moved. Version bumps and
#    CHANGELOG entries belong to the release integrator.

# 5. Commit: the fixture, TRANSPORT-<major>.json, the .bin files and the registry
#    together in one commit, any code or known-gap change in its own commit.
```

A new release that matches the old recipe needs no Rust edits at all. A new major
added to the registry joins the fingerprint seed pool automatically, because
`fingerprint::MAJORS` is `RELEASES.len()`. Older releases stay selectable through
`browser::release(major)`, the pool, and the render tier's
`set_fingerprint({chromeMajor, fullVersion, userAgent})`.

## 4. CI / scheduled mode (`--check`)

`--check` never writes the registry, a fixture or an oracle. It only writes a report
under the ignored `artifacts/` directory.

```sh
# Is there a newer stable? No browser launch, no build. Suitable for a cron job.
node harness/detection/reversion-chrome.mjs --chrome "/Applications/Google Chrome.app" --check --offline
# Full drift check: re-measures Chrome (unfrozen, read-only) and turbo-surf.
node harness/detection/reversion-chrome.mjs --chrome "/Applications/Google Chrome.app" --check [--native rust/target/debug/examples/identity_battery]
```

| Exit | Meaning | Next step |
| --- | --- | --- |
| 0 | Current: no drift, no unexplained tell | — |
| 10 | A newer stable exists (new major or new build) | run §3 |
| 11 | Chrome's identity drifted from its own fixture at the same build: a field trial, profile default or OS change, or `collector-changed` (the collector's hashes differ) | re-run §3 to re-measure; review |
| 12 | turbo-surf has unexplained tells against the default fixture | read the `[tell]` lines; fix the code or document a gap |
| 2 / 1 | Usage error / crash | — |

## 5. Handling deltas

The tool's `blockers()` reasons say which case applies.

**Header order changed for a known header** (`header-order`, action `registry`).
No work is needed. The class orders and `headerOrder` are re-derived. Check that
`navigate` still contains `cookie` in Chrome's slot. `withCookieSlot` re-anchors it
after the header that preceded it in the previous release when no cookie was sent.
The battery's first-party cookie measures that slot directly for every class except
the cold navigation.

**A brand-new header** (`header-extra`, "new header X on K": stop). Nothing in
turbo-surf can emit it, so the registry can't fix it.

1. Identify the header in the Chromium source or release notes.
2. Implement it where it belongs: `request_context::RequestContext::headers` for
   fetch metadata, `ClientHints::headers`/`hint_value` for client hints, `net` for
   transport-owned headers.
3. Re-run. The class orders pick up its position automatically.

**A removed header** (`header-missing` in version mode): stop. Delete it from the
code that emits it, then re-run.

**A changed value other than Accept/Priority** (for example a new `Accept-Encoding`
token): stop. Change the code that owns it (`net::emulate` owns `accept-encoding`).

**Client hints.**

- A new hint name in Chrome's `Accept-CH` response handling shows up as
  `header-extra` on `ch-sub`/`ch-next`. Add it to `hint_value`, and to
  `HIGH_ENTROPY_HINTS` in both `browser.rs` and identity-diff.mjs if it's high
  entropy.
- A changed hint value format (for example `Sec-CH-UA-Form-Factors`) means editing
  `hint_value`.
- If the GREASE brand algorithm changes, `diffVersions` reports an error ("sec-ch-ua
  for Chrome N is …, not the brand algorithm's …"). Port the new algorithm to both
  `fingerprint::brand_versions` and `brandVersions` (identity-diff.mjs). The unit
  test checks the JS mirror against every measured fixture.

**TLS/H2 recipe** (any `transport` delta: stop, "TLS/H2 changed at …").

1. Read `fixtures/chrome-identity-<major>.json` `connections[0].hello` against the
   previous fixture. Typical changes are new extensions or codepoints (ALPS 17513 →
   17613 was one), group order, new signature algorithms, certificate compression,
   and H2 SETTINGS order or values.
2. Check whether upstream wreq-util has a newer `Chrome<N>` profile that matches. If
   it does, adopt it through your own branch of the fork pinned in `rust/Cargo.toml`
   (see `rust/release-deps/transport`). Add its arm to `net::transport_recipe` and
   set `transportBaseMajor`.
3. If no recipe can express the change, keep the closest measured recipe. Add a
   known-gap rule with the exact field and why the fork can't express it, then re-run.
   Never edit fixture bytes.

**JS identity** (`navigator.*`/`userAgentData.*` value changes: stop). These values
are code in `runtime.rs` (`__navData`). Change the code, never the fixture. A
`deviceMemory` change is data (`defaultDeviceMemory`).

**Engine, ICU, builtins, CSS features or globals** (action `engine`). The bump goes
ahead; these are recorded. Parity classifies them through the `js-engine-version`,
`css-feature-support` and `window-global-inventory` gap rules.

- When an extra or missing global appears, update the pinned `allowAdded`/
  `allowRemoved` list only if the reason still holds.
- Otherwise fix the interface (API-INVENTORY.json / issue #28 workflow).

## 6. Never fake

The registry records what a measured Chrome sends. Some things must not be
satisfied by changing strings:

- **The V8/JS-engine version.** Error message wording, builtins (`Iterator.zip`,
  `Iterator.prototype.includes`), ICU data counts and stack shapes come from the
  embedded V8 (stock 15.0, native155 15.5; Chrome 156 is 15.6). They are listed as
  `js-engine-version` known gaps until the engine moves through
  `harness/detection/engine/`. Patching messages or adding builtin shims would make
  behaviour and identity disagree.
- **`CSS.supports`.** It must answer from turbo-dom's real parser. If it said `true`
  for an unsupported feature, `getComputedStyle` on the same page would contradict it.
- **Transport.** A ClientHello the fork does not really produce must never be
  claimed, and `TRANSPORT-*.json` must not be edited by hand.
- **Fixtures.** Never hand-edit `chrome-identity-*.json`. Re-measure it. Never
  relabel a capture from one browser version as another (REMEASURE-156.md
  labelling rules).
- **`navigator.webdriver`.** It stays `false`. The measured `true` is the CDP
  collector's artifact.
- **Missing behaviour.** Don't emit requests to look busy. Favicon, web-font and
  Critical-CH gaps stay gaps until the behaviour really exists.

## 7. Tests, fixtures and gates

Version-agnostic tests read the expected default from the registry or its fixture:

- `harness/detection/chrome-identity.test.mjs`:
  - the registry entry equals `measuredRelease(fixture)`
  - the fixture's collector hashes match the current files
  - the GREASE mirror reproduces every fixture
  - JA4 and Akamai strings parse the same way
  - diff and known-gap classification
  - blockers, `applyRelease`, `releaseStatus`, drift, and the engine hook (with a
    fake script)
- `rust/crates/turbo-surf-core/tests/browser_registry.rs`: the UA, `sec-ch-ua`,
  full version, navigation order, Accept/Priority per destination and class orders
  come from `chrome-identity-<DEFAULT_MAJOR>.json`. Every release stays selectable
  in the seed pool.
- `rust/crates/turbo-surf-render/tests/chrome_identity.rs`: runs the same probe in
  the render tier and compares it with the fixture field by field. The machine-bound
  `architecture`/`platformVersion` come from the profile. Pinned assertions:
  - `KNOWN_GAP user-agent-data-interface`
  - the webdriver collector artifact
- `rust/crates/turbo-surf-render/tests/browser_release.rs`: compares the default
  identity with the fixture, and a pinned 155 identity with the independent
  `browser-release-chrome155.json` control. This is the explicit "older release
  stays selectable" coverage.
- `turbo-surf-core/src/browser.rs` tests: the registry shape, fallbacks, and the
  per-version behaviour that really differs (153 vs 154 order, 155's image/jxl
  Accept).
- `fingerprint.rs` `brand_headers_match_the_captured_low_and_full_versions` pins
  measured brand strings per major. Add the new major's measured pair there; it is
  the one per-version literal a bump adds.

Tests that keep a version literal on purpose:

- Behaviour fixtures named `*-chrome155.json` assert the browser that produced
  them, which is provenance, not the default identity. Leave them alone unless you
  re-measure that behaviour.
- `browser-release-fixture.test.mjs` pins the historical 155 control's provenance.

Hash-pinned files: `chrome-identity-probe.js` is listed in `biome.json`
`files.ignore` and in the `VENDORED` regex of `.githooks/pre-commit`. Reformatting
it would change its hash. The JSON outputs are biome-formatted by the tool.

Gates for the bump commit (turbo-surf):

- `cd rust && cargo test --workspace`: 0 failed
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --check`
- `npx oxlint`: 0 errors
- `node_modules/.bin/biome format .`: clean except the known `API-SHAPE-LAST-RUN.json`
  size notice
- `npm run test:detection`

## 8. Failure modes

| Symptom | Cause | Fix |
| --- | --- | --- |
| `TLS handshake … alert decode error` / no requests seen | A non-RSA certificate encoding that Chrome rejects | Keep `identity-server.mjs` on `rsa:2048` |
| `battery page did not finish; requests seen: …` | The page script threw before setting the done symbol, or Chrome's first-run UI | Read `pageErrors`; check the flags (`--no-first-run`) |
| `exists but does not match … move it aside to re-freeze` | `.native-builds/oracles/chrome<major>-<build>` holds a different bundle | Move it aside (never delete someone else's oracle) and re-run |
| `frozen executable reports X, not Y` | The app updated itself between detection and freeze | Re-run |
| Parity shows many `header-value … user-agent` tells | `--native` points at a binary built before the registry changed | Drop `--native` (the tool rebuilds) or rebuild |
| `orders A / B inconsistently within request class C` | Chrome orders one class two ways | Split the class in both `request_class` (Rust) and `requestClass` (JS), then re-run |
| `collector-changed` drift (exit 11) | The battery files changed since the fixture | Re-run §3 to re-measure with the new collector |
| `favicon` counts vary between runs | Chrome's favicon fetch races the next navigation | Expected; favicons are keyed by origin and are `info` in version diffs |
| `identity_battery exited …` | A render or no-JS error | The tail of stderr is in the message; `nativeRun` in the fixture shows per-step status |
| ECH extension length differs between connections | Chrome's ECH GREASE payload is randomly 186/218/250/282 bytes | Expected; normalization drops the ECH length |

## 9. Worked example: 155 → 156

This is the first run of these helpers, on 2026-10-25, on a MacBook (arm64, 128 GiB,
macOS 27.2).

The oracle was the installed stable, `156.0.8078.47` (V8 15.6.75.19, revision
`1d17a2e2`), frozen to `.native-builds/oracles/chrome156-8078.47`. Its bundle hash
was `465e3ecf…`, the same bundle the REMEASURE-156.md trial froze. Chrome for
Testing in `chromium-1248` is `156.0.8078.4`; `--check` reports it as `older`.

The 155 baseline is `fixtures/chrome-identity-155.json`. It was imported with
`node harness/detection/chrome-identity-import.mjs 155` from TRANSPORT-155.json (one
cold H2 navigation plus its raw ClientHello) and `browser-release-chrome155.json`
(UA-CH). It is partial; nothing else was measured for 155, and no Chrome 155
binary remains on this machine.

Release deltas between 155.0.8059.80 and 156.0.8078.47, after version
normalization: **0**. In detail:

- **Version tokens.** UA `Chrome/156.0.0.0` (reduced). The full version
  `156.0.8078.47` appears in `Sec-CH-UA-Full-Version(-List)`, `uaFullVersion` and
  `fullVersionList`.
- **GREASE brands.** `"Not:A-Brand";v="8", "Chromium";v="156", "Google Chrome";v="156"`
  (155 was `"Google Chrome";v="155", "Chromium";v="155", "Not(A:Brand";v="24"`).
  The existing seed algorithm reproduces both. Only the seed changed, not the
  algorithm.
- **Navigation header order and Accept.** Unchanged (the classic order, `image/jxl`
  Accept).
- **TLS.** The normalized ClientHello is identical. JA4 is
  `t13i1516h2_8daaf6152771_cb7bf5808d99` for Chrome 155, Chrome 156 and turbo-surf.
  Details:
  - X25519MLKEM768 first
  - ALPS 17613
  - Brotli certificate compression
  - ECH GREASE
  - the same 186-byte trust-anchor list
- **H2.** The Akamai string is `1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p`,
  and the HEADERS priority is exclusive, dependency 0, weight 256 for documents.
  This is the same for Chrome 156 and turbo-surf. The 155 capture had only Node's
  settings object; the values agree.
- **`navigator.deviceMemory`.** 32. The registry's 155 value of 16 came from an
  earlier control that can't be re-checked, so 156 records the measurement on this
  128 GiB Mac.
- **JS engine.** Not comparable with the 155 baseline (unmeasured).

Measured for the first time (new registry data for 156): seven request-class orders
(`navigate`, `navigate-hinted`, `no-cors`, `cors`, `cors-hinted`, `cors-resource`,
`preflight`), and Accept/Priority per destination. The image Accept gained
`image/jxl` and the image Priority is `i`; turbo-surf had sent the Chrome 154-era
Accept and `u=2, i`.

turbo-surf parity against the 156 fixture:

- **Before the bump** (registry 155, empty known-gap list): 170 unexplained deltas.
- **After** the registry update and three small code fixes (no client hints on CORS
  preflights, the `ect` hint, registry-driven Accept/Priority/order per request
  class): **0 tells**, 57 known-gap, 10 environment and 1 collector delta.

The 57 known-gap deltas fall under 13 rules:

| Rule | Deltas | Rule | Deltas |
| --- | --- | --- | --- |
| css-feature-support | 18 | dpr-hint | 2 |
| h2-stream-priority | 11 | author-header-position | 1 |
| user-agent-data-interface | 9 | critical-ch-retry | 1 |
| favicon | 5 | form-submission-to-frame | 1 |
| js-engine-version | 3 | web-font-loading | 1 |
| h1-connection-header | 2 | window-global-inventory | 1 |
| h1-priority-header | 2 | | |

Timings: the battery takes about 12 s for Chrome and 8 s for turbo-surf. The first
full run took 52 s with a warm build. A re-run that recompiled `identity_battery`
took 2.5 min. Two consecutive re-runs gave zero same-build drift.
