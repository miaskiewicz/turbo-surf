# Google Chrome parity checklist

This is the canonical Chrome parity checklist for PR #26 and follow-up work.
Every unresolved item remains open until its implementation, behavior regressions
and combined validation are recorded. Work left outside PR26 is tracked in
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28); deferred audio
has its dedicated [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27).
Do not close either issue merely because the PR merges.

The historical [API-SHAPE.json](./API-SHAPE.json) capture used Chrome
155.0.8059.40 / V8 15.5.35.20. The newer
[API-INVENTORY.json](./API-INVENTORY.json) preserves the full measured lists and
source/executable hashes for its Chrome155.80/native155.23 snapshot. Neither
snapshot describes changes integrated after its frozen executable was built.
A missing **own prototype member** is an observed descriptor/ownership difference;
it does not by itself prove the feature is entirely absent. Existing inherited
methods and aliases must be tested before adding another implementation.
A repair is closed only after behavior tests and a fresh paired audit pass.


## Latest repair status

Update this table when a fix or its validation changes. A component pass closes
only its named control; the broader area stays open until its listed requirements
and combined gates pass. Historical inventory/live counts are not updated by
reading source alone.

| Repair | Current evidence | State |
|---|---|---|
| Dictionary store, negotiation and incremental decoding | Pushed ec868b7; Core220 and10 HTTPS controls per transport, strict/format pass | Implemented; final workspace/live gates pending |
| Dictionary discovery and resource task source | Pushed a15d098;32 exact independent Chrome cases pass on stock and actual155 | Implemented; publication timing/encoded Range remain open |
| Live form collections, submitters and radio ownership | Pushed a15d098/canonical51b5b97; actual155 collection2/navigation12/radio1 pass | Implemented within controls; broader forms remain open |
| Native control labels and receiver-realm NodeList | Pushed5c0b886/canonical1e93cae; actual155 and stock production behavior/GC2 pass | Integrated; final full gates pending, label activation open |
| Real native deadlines vs virtual Window timers | Pushed5c0b886; source-pinned Chrome4 outcomes, actual155 affected14 and MCP73 pass | Integrated; final full gates pending |
| Private abort/controller conversion | Actual155 privacy2 and stock privacy2/adapter2 pass; original hostile-hook reproductions retained | Concrete exposed paths fixed; broader hooks/array fast-path count remain in#28 |
| API inventory | Frozen70-interface capture predates these repairs;75-interface collector expansion tested | Rebuild and fresh pair required; no predicted counts |
| Google acceptance | Last frozen544180d baseline0/2 native SERP | Rebuilt fresh GPU/wreq run pending |

### Final full workspace checkpoint

The final default full-workspace command exits0 with1,374 passing tests and nine
explicit ignores (three unavailable stock-SDK native callbacks and six generated
doctests). The actual155 full-workspace command exits0 with1,413 passing tests,
zero failures/filtered cases and six generated doctest ignores. Both commands
include the corrected cancellation observation from the start. A subsequent
test-only font-scope correction preserves the full macOS Chrome comparison and
font-independent shadow Range/retirement assertions on Linux; its whole nine-test
suite passes on stock, with actual155 and final strict checks in progress.
Addon/shim/E2E and fresh optimized API/Google captures remain separate gates.

## Required PR26 completion work

The deferred behavioral residuals remain explicitly tracked in#27/#28. The
following delivery and validation work remains required for this PR:

- Labels, clock, privacy and driver fixes are pushed in5c0b886; finish the
  final test-only cancellation/font controls and delivery evidence.
- Pass final full default/native workspace, all-target strict and formatting gates.
- Rebuild the addon and pass shim/E2E controls with recorded engine/features.
- Rebuild the optimized native executable with wreq, trust anchors, Metal and CoreText.
- Capture the fresh75-interface pair and update every measured inventory count.
- Run fresh native-cookie Google controls with actual consent and genuine forms;
  record the accepted/rejected outcome without importing Chrome cookies.
- Refresh changelog, findings and PR description; retain evidence and remove
  superseded temporary build outputs safely.

## Open work and issue ownership

All rows below have durable follow-up coverage in
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28). They include work
still being attempted in PR26; issue coverage does not defer that work by itself.

| Area | Remaining closure requirements | Tracking |
|---|---|---|
| Messaging / GC | Broader realm/frame retirement, detached storage reclamation, endpoint and reentrant failure coverage | #28 |
| Layout | Decorated/empty/mixed-font inline boxes, stacking, scrolling/events/RTL, fixed positioning, iframe migration | #28 |
| Range / caret | Remaining composed-tree/lifecycle/geometry cases beyond the integrated interface/receiver/GC controls | #28 |
| DOM | Parsing/observer edges, custom-element reactions/registries, broader form association/CustomStateSet lifecycle, UTF-16 edge cases | #28 |
| Forms | Broader radio/adoption and form-collection behavior beyond saved controls, FileList/multipart, other targets and encoding/submitter/policy cases | #28 |
| User activation | Consuming APIs, cross-frame propagation, broader trusted input, MCP reconstruction continuity and retained/retired receiver or cross-origin-return redirect behavior | #28 |
| Events / promises | Stream/listener microtask interleaving and broader Object-prototype descriptor hooks during abort/event dispatch and exception reporting | #28 |
| CSS / resources | Remaining selectors/at-rules, used downloaded fonts/animations, images/pseudo epochs, inner SVG | #28 |
| Security | Remaining CSP/reporting/sinks, popup/navigation/sandbox, COOP/COEP and permissions policies | #28 |
| Fetch / storage | Final dictionary renderer integration, publication scheduling and encoded Range behavior; broader streaming/cancellation/cache/signals/referrer and partition/storage-access behavior | #28 |
| API inventory | Every unresolved missing/extra member and descriptor/inheritance/behavior path in API-INVENTORY.json; fresh paired measurement | #28 |
| Distribution | Stock/native V8 alignment, native patch distribution, layout publication and registry-ready transport/crypto dependencies | #28 |
| Browser lifecycle | Watchdog/async-op admission recovery, timeout-signal delivery/GC, listener-count release and retained detached-window active-time suspension | #28 |
| Validation | Final combined workspace/default/native/addon/shim/E2E gates and lifecycle regressions | #28 |
| Transport / GPU / Google | Unmeasured identity/wire/rendering differences and fresh native-cookie consent/SERP run | #28 |
| Audio | Realtime/feedback/parameter connections/multichannel/custom waves/oversampling/suspend-resume | [#27](https://github.com/miaskiewicz/turbo-surf/issues/27) |

When an item is finished, record the relevant test/capture and commit before
checking its issue task. If a narrower follow-up issue is created, link it from
this table and #28 so the residual backlog stays complete.

### Measured abort/stream queue difference

The independent [Chrome control](./fixtures/abort-bridge-order-chrome155.json)
and [unadapted Deno baseline](./fixtures/abort-bridge-deno-baseline.json) retain an
exact open difference: Chrome runs stream sink-abort/cancel reactions before the
microtask queued by the signal listener; the native Deno backend runs that listener
microtask first. The same difference existed before the private event-array fix
and remains tracked in issue28. Event state, dependent reason identity and timeout
results are checked against Chrome separately; a passing state control does not
close this queue-order gap.

The private-array and reason-cache controls do not certify all descriptor paths.
Poisoning `Object.prototype.get`/`set` around the entire abort dispatch still reaches
another event-dispatch/exception-reporting path. The reason-cache read itself is
protected by a null-prototype descriptor; broader descriptor-hook isolation remains
open in issue28.

### Measured dictionary-transport gap

[Saved independent evidence](./compression-dictionary-saved-evidence.json) shows
Chrome receiving a compression-dictionary Link, fetching/storing dictionaries,
sending Available-Dictionary on87 requests and actually decoding one dcb response.
No Origin-Trial response header appears in that saved trace. The frozen live executable receives
the Link but has no discovery/store/decode chain; its transport default advertises
the four base encodings. The next source batch implements that chain, pending
integrated validation; these historical observations describe the frozen executable. Native explicit-header logs alone do not enumerate every
final wire header. The existing bounded TLS/H2 comparison and GPU probe match;
the upstream recipe's154 label is not itself a demonstrated wire mismatch.

Implement genuine partitioned, origin-scoped and fresh dictionary state, validated
headers/hashes, real decoding and policy-aware discovery before advertising the
additional encodings. [RFC9842](https://www.rfc-editor.org/rfc/rfc9842.html) requires
eligible dictionaries for those requests. This missing chain is being implemented;
it is not a proven explanation for Google's rejection. The fresh localhost control
also measures asynchronous Chrome publication: immediate post-GET availability is
absent, while +100 ms/+500 ms requests advertise it. Native EOF publication is
synchronous. That scheduling difference and encoded HTTP Range behavior
remain explicit work in#28; incremental coded-response delivery now passes real
HTTPS header, partial delivery, cancellation and cache controls on both transports,
with32 independent Chrome discovery/lifecycle cases passing in both native155
and stock renderer integration; stable delayed positives do not close the
publication-scheduling or encoded-Range gaps.
See [dictionary controls and bounds](./COMPRESSION-DICTIONARIES.md).

## Earlier integration checkpoint (2026-10-16)

Canonical turbo-test51b5b97 is mechanically vendored with public DOM0.5.7
candidatefb60e6f (latest registry0.5.6). Genuine ElementInternals/CustomStateSet,
checked-state foundation, FormData events, disconnected-form admission and native
form reflection are integrated. The pushed544180d affected native renderer cohort passes
385 tests with zero failures/ignores, including312 library and73 integration tests.
Full all-target actual155 workspace strict checks pass. The earlier native MCP
checkpoint passes72/default71 tests; full final workspace/addon/shim/E2E gates
remain separate and are in progress.

Public immutable layout producer0.3.8 candidate9d7f02fb is integrated; latest
published registry release is0.3.7. Actual V815.5.35.23-rusty matches installed
Chrome155.0.8059.80's V8 patch; SDK source provenance remains155.0.8059.79.
Stock production SDK150 remains a distinct incomplete configuration.

The host-media, modern-CSS and elapsed-wait input failures are repaired. In the
latest two-repeat optimized544180d matrix, both native baselines complete real
consent/save204 and carry the genuine HttpOnly BUCKET cookie before search. Both
submit forms, receive/search200 and follow retry to/sorry429: zero native baseline
SERP successes in two fresh runs. Chrome baselines have9 markers each and zero
recorded errors. Native traced repeat0 fails with Missing promise after home
budget exhaustion; repeat1 reaches/sorry. Both native homes still report20s budget
termination. See [latest run](./GOOGLE-LAST-RUN.json).

An offline admission-window probe reproduces the SDK promise bookkeeping failure;
the pushed26a08b5 experimental155 transaction patch passes independent
preimage/recovery/rejection controls. Its managed-SDK scope, interrupted-reservation
retention and production-distribution bounds remain documented in
[ASYNC-ADMISSION.md](./engine-155/ASYNC-ADMISSION.md). The combined
default suite also found premature GC of an awaited timeout signal. Its realm-local
retention candidate passes the exact failing test with unchanged budget/Chrome
assertion; native retirement/forced-GC controls now pass within155 affected tests, and
all-target native workspace strict checks pass. Final integrated full gates remain
separate. A live or externally retained document currently retains pending timeout frontends even without public
listeners until timeout fires. Listener-count early release and retained detached-
window active-time suspension remain open. Reliable native acceptance is unresolved.

The runtime follow-up default workspace gate passes1,312 tests across120 suites,
with three explicit native-CSP capability skips and six generated doctest skips.
Default all-target strict checks and formatting pass. Native155 affected tests
pass155/155 and native all-target strict checks pass. These results precede the
next canonical DOM/dictionary integrations; addon/shim/E2E, final integrated gates
and fresh live validation remain pending.

### Linux font control and current source integrations

CI for26a08b5 passes18 geometry controls and fails the UTF-16 geometry test because
it compares exact macOS Chrome system-font measurements with Linux's available
fallback fonts. Preserve the full exact macOS control; separately assert original
code units, all selected boundaries, private access, retained caret identity and
lifecycle on every platform. This test correction does not certify Linux glyph or
fallback parity. The correction is pushed as6f9ebc7 and survived the subsequent CI run. A matching Linux
Chrome/font capture remains open under#28.

Pusheda15d098 vendors registry provenance, submitter reflection and genuine live
form collections from canonical51b5b97. The actual155 production cohort passes two real iframe/collection-GC tests,
12 form-navigation tests including reflected submitter POST overrides, and
23 geometry controls. The final actual155 combined component cohort passes46 tests, including the
corrected radio GC policy and all32 exact dictionary task/lifecycle controls.
Stock-engine renderer library tests pass326/326; its dictionary and radio
integration tests also pass. Core passes220 tests, including real HTTPS dictionary
controls on both transports and incremental output/cancellation. Independent
Chrome controls and official dictionary-dependent Brotli/Zstd fixtures retain
source provenance.
Do not close these rows or refresh measured inventory counts until the final
integrated executable is rebuilt and captured.


### Labels and native timer clock follow-up

Pushed5c0b886 mechanically vendors canonical1e93cae. Seven existing
labelable interfaces have genuine native `.labels`; controls cover live identity,
hidden transitions, adoption, first-ID/empty-ID rules, foreign namespaces,
custom-control ordering and hostile public methods/constructors. The NodeList
factory follows the receiver's creation realm. Actual155 production tests pass
both behavior/iframe and retained-list/retired-realm GC controls. Full label
activation is not claimed.

The full native run exposed an independent clock difference: a future virtual
Window timer could overtake an earlier real Deno timeout or network body. The
source fix keeps one precise monotonic wake while native work is pending,
without starving a Window deadline that must abort stalled I/O. Fresh actual
Chrome controls pin four outcomes, including timeout20ms/Window30ms after GC
and with a held signal lacking public listeners. Focused native27 and stock9
checks pass before the additional held-timeout fixture integration; the held
case separately passes actual155. Full native/default retries are in progress.
The offline harness passes78 tests with four explicit skips, plus two Python
SDK-helper tests. Quiescent virtual fast-forward and broader browser timer policy
remain explicit limits; no new native realm root or speculative timer capability
is added.

### Peer layout conformance timing

The stock full retry records1189 passing tests across101 completed suites, plus
one failure and three explicit skips: the peer-layout fixture exhausts its
5-second call budget. An isolated run reproduces the failure.
A native process sample locates the stall in one-time system-font discovery and
font parsing, before the scalar geometry result; this is not evidence of a
Promise, timer or document-ownership leak. The peer-layout conformance fixture now has an explicit60-second budget while
retaining every31/73/97 width,
interleaving and replacement assertion. Production limits and font performance
are unchanged. The current-source shadow8/8 and isolated peer1/1 pass. The sampled failure
and fresh focused proof are retained separately.

### Combined-run follow-ups still in progress

The actual155 full retry exposed a human-interaction completion failure. Its exact
MCP executable passes the isolated test and a later72-test run; default MCP also
passes. The former driver started a sequence Promise but returned undefined,
leaving completion dependent on incidental draining. The source fix now awaits
that actual final Promise while preserving native form commits and their retired
gesture realm. Actual155 MCP73/73 passes, including the new async-form/forged-marker
and rejection regression. Product budgets are unchanged; final full gates remain.

A separate adversarial probe confirms private abort backend values can escape
through guest conversion hooks. The repair needs both private iterable conversion
in the frontend and a source-pinned dense-array adapter in the official WebIDL
sequence converter. Both pieces are integrated and actual155/stock hostile-hook
regressions pass; the wrapper alone was insufficient. Broader descriptor hooks and the separately
measured stream/listener microtask order remain open.

CI for a15d098 independently reproduces the timeout-order failure on Node20;
the source clock fix addresses that exact unchanged assertion. Node22 reaches
the native storage lifecycle fixture and exhausts its5-second debug execution
budget during600 create/mutate/remove operations. Its conformance-only budget is
now60 seconds; the600 operations,1200-node reclamation assertions, identity and
peer-isolation checks are unchanged. Production execution budgets are unchanged.
The corrected storage fixture passes the final actual155 affected cohort;
this is not a performance claim.

The private abort/descriptor repair passes its initial actual155 regression
with the pinned WebIDL adapter. The final source also replaces a mutable
Array.includes decision that could skip stream-controller wrapping. Its independent
Chrome control and native regression pass. Final actual155 affected privacy2,
stream7, clock3 and storage2 tests pass14/14 with zero skips; stock private
regressions2/2 and pinned adapter units2/2 pass. Exact
Chrome array fast-path behavior remains a separate measured difference: Chrome
skips a poisoned inherited Array iterator, while the current wrapper calls it
once. The regression preserves that count difference and asserts no private raw
value escapes. It stays in#28; no guessed Proxy fast path is added.

Full runs and their failures remain recorded separately from component passes.
The stock full run initially failed linking due to disk exhaustion before tests;
a retry later passed MCP and328 renderer unit tests before a controlled cleanup
pause. That retry continues with its exact compiled source. The newly authored
security/driver follow-ups require their own focused validation and updated
combined scope before final gate or live-acceptance claims.

### Cancellation observation in final full validation

The final native no-fail-fast run exposes one Core cancellation regression
assertion: it checks transport closure immediately after worker permits recover.
Those are different completion events because aborting the producer schedules
its teardown. The test now awaits the actual sender-closed notification under
its existing10-second timeout, then retains the closed-state assertion. Worker
recovery is still checked separately. Production cancellation, caps and budgets
are unchanged. The original failing native executable/log and corrected source
are retained. The corrected complete Core unit cohort passes195 tests, and both
final full workspace commands above include the fix and exit0.

## Active scope and deferred audio

Finish messaging, layout, Range/DOM, security/resources, fetch/storage, combined
validation, the current member inventory, PR housekeeping and fresh live Google
validation. Remaining advanced audio is a documented TODO in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27),
deferred at the user's request. Existing offline DSP and tests remain included.
The authored adapter now explicitly selects `std::exp2(float)` on every target;
all 176 independent Chrome frequency-response cells match exactly on Linux x86,
macOS ARM and Rosetta x86. This fixes a platform overload discrepancy without
numeric tolerance or changes to licensed kernels; a Linux Chrome browser audio
comparison remains unmeasured.
Reprioritize deferred audio only with captured detector evidence showing a missing
path matters to the target; no Google-audio cause is established by these controls.

## Fresh own-member inventory (2026-10-16)

[API-INVENTORY.json](./API-INVENTORY.json) retains full missing/extra and descriptor
lists, actual Chrome/native versions and source/executable hashes. The frozen
localhost pair uses Chrome155.0.8059.80/native V815.5.35.23 with all GPU/wreq features.
It predates the implemented collection/label repairs; its absent-interface/member
counts are historical evidence until the rebuilt75-interface capture replaces it.
The original four interfaces have **65** missing own members (Document29,
Element30, HTMLElement4, Canvas2); the expanded70-interface capture has **134**
missing own members among measured interfaces. Two native interfaces are absent:
HTMLFormControlsCollection and RadioNodeList. AbstractRange, NodeRange, StaticRange,
ElementInternals, CustomStateSet, File, FormData, FormDataEvent and UserActivation have zero measured
own-member/descriptor differences. There are349 total difference paths, including
raw own-key order and descriptors. Existing Blob, ValidityState, form, input and
button members now have zero measured descriptor differences. Blob.textStream
and the listed missing form/control members remain open.

Historical counts were94, then66 original/147 expanded. The preceding58-interface
snapshot measured133 expanded members and304 paths; growing the constructor scope
and adding raw key order changes the denominator. The prior69-interface capture
had359 paths before the ten descriptor/order repairs; the new70-interface scope
adds UserActivation with no measured differences and retains349 paths. Real form reflection and existing
input/button descriptor repairs are integrated. Every remaining named path is
tracked under issue28; none of these measurements certifies browser conformance.

## Current measured missing members

Generated from the latest `API-INVENTORY.json` snapshot. A listed missing own
member is a measured shape difference; existing inherited/instance behavior may
already exist. Zero missing members does not certify full behavior. Descriptor
and key-order differences remain separately recorded in the JSON.

| Interface | Missing own prototype members |
|---|---|
| Blob | `textStream` |
| Document | `all`, `ariaNotify`, `browsingTopics`, `domain`, `execCommand`, `exitPictureInPicture`, `exitPointerLock`, `fragmentDirective`, `fullscreen`, `fullscreenEnabled`, `getAnimations`, `hasPrivateToken`, `hasRedemptionRecord`, `lastModified`, `pictureInPictureElement`, `pictureInPictureEnabled`, `pointerLockElement`, `prerendering`, `queryCommandEnabled`, `queryCommandIndeterm`, `queryCommandState`, `queryCommandSupported`, `queryCommandValue`, `startViewTransition`, `timeline`, `wasDiscarded`, `webkitCancelFullScreen`, `webkitExitFullscreen`, `webkitFullscreenEnabled` |
| Element | `activeViewTransition`, `animate`, `ariaNotify`, `checkVisibility`, `computedStyleMap`, `currentCSSZoom`, `customElementRegistry`, `getAnimations`, `getHTML`, `hasPointerCapture`, `onbeforecopy`, `onbeforecut`, `onbeforepaste`, `onfullscreenchange`, `onfullscreenerror`, `onsearch`, `onwebkitfullscreenchange`, `onwebkitfullscreenerror`, `pseudo`, `releasePointerCapture`, `requestPointerLock`, `scroll`, `scrollIntoViewIfNeeded`, `setHTML`, `setHTMLUnsafe`, `setPointerCapture`, `startViewTransition`, `webkitMatchesSelector`, `webkitRequestFullScreen`, `webkitRequestFullscreen` |
| EventTarget | `when` |
| HTMLButtonElement | `command`, `commandForElement`, `formAction`, `formEnctype`, `formMethod`, `formTarget`, `interestForElement`, `labels`, `popoverTargetAction`, `popoverTargetElement` |
| HTMLCanvasElement | `captureStream`, `transferControlToOffscreen` |
| HTMLElement | `attributeStyleMap`, `editContext`, `focusGroup`, `focusGroupStart` |
| HTMLFormElement | `Symbol(Symbol.iterator)`, `elements`, `length` |
| HTMLInputElement | `align`, `alt`, `createValueRange`, `dirName`, `files`, `formAction`, `formEnctype`, `formMethod`, `formTarget`, `height`, `incremental`, `indeterminate`, `labels`, `list`, `maxLength`, `minLength`, `popoverTargetAction`, `popoverTargetElement`, `showPicker`, `size`, `stepDown`, `stepUp`, `useMap`, `valueAsDate`, `webkitEntries`, `webkitdirectory`, `width` |
| HTMLTextAreaElement | `cols`, `createValueRange`, `dirName`, `labels`, `maxLength`, `minLength`, `rows`, `textLength`, `type`, `wrap` |
| MessageEvent | `userActivation` |
| Range | `expand` |
| ShadowRoot | `activeElement`, `clonable`, `delegatesFocus`, `fullscreenElement`, `getAnimations`, `getHTML`, `getSelection`, `onslotchange`, `pictureInPictureElement`, `pointerLockElement`, `referenceTarget`, `serializable`, `setHTML`, `setHTMLUnsafe`, `styleSheets` |

Absent native interfaces in this expanded capture:
- `HTMLFormControlsCollection`: Chrome declares `namedItem`, `constructor`, `Symbol(Symbol.toStringTag)`, `Symbol(Symbol.iterator)`.
- `RadioNodeList`: Chrome declares `value`, `constructor`, `Symbol(Symbol.toStringTag)`, `Symbol(Symbol.iterator)`.

Historical counts were94 original-interface members, then66/147 on an earlier
snapshot. The latest original-four count is65. Expanded counts now cover70
interfaces and134 missing own members; the domain grew by11 interfaces, so the
expanded count is not directly comparable to the preceding133-member snapshot.
[API-SHAPE.json](./API-SHAPE.json) and Git history retain the earlier evidence.

## Behavior and architecture gaps beyond member names

| Surface | Concrete remaining work | Evidence / status |
|---|---|---|
| DOMTokenList | Stable identity, private brand, indexed/live tokens, mutation/error contracts; `part` | Canonical cached/live implementation passes offline mutation/brand contracts; fresh native 155 pair has zero differences for the saved probe |
| ARIA element relationships | ID resolution, tree scope, explicit element assignment, attribute clearing, frozen returned sequences | Scoped/private canonical state implemented and tested; fresh native 155 pair has zero differences for the saved probe |
| Layout and hit testing | Complete decorated inline, stacking, scrolling/RTL, iframe migration and layout-unit behavior | Native0.3.8 candidate drives real rectangles, client/offset/scroll and point hits; original UTF-16 Range/caret, outer SVG, rotation and composed shadow controls pass. See [current bounds](./LIVE-GEOMETRY.md) |
| Rendered text | Complete layout-dependent wrapping/line-box behavior beyond native cascade text flow | Canonical visibility/whitespace/break/table/details/setter contracts implemented; fresh native 155 pair has zero differences for the saved probe |
| Shadow/custom elements | ElementInternals/form association, broader upgrades/reactions and scoped-registry lifecycle | Native slots/manual assignment, closed roots, scoped registries and construction/adoption controls are integrated; ElementInternals and CustomStateSet now pass native behavior/GC controls |
| CSS parsing/layout | Complete grammar/cascade, Typed OM, animation timelines and transitions | Measured stylesheet policy controls pass; complete CSS behavior is open |
| Live stylesheet installation | Explain Google's jc/lc reports during actual execution; compare live ownership and rule state | Minimal authored injection tests pass; reusable CSS diagnostics saved |
| Observers | Real Mutation/Resize/Intersection/Performance observer queues and lifecycle | Native MutationObserver semantic records, microtask delivery, options/old values/transients and a private postconnection consumer are integrated. Full mutation lifecycle and Resize/Intersection/Performance queues remain open |
| Browser lifecycle/resources | Complete parsing/resource scheduling, frame navigation, media and native worker lifecycles | Tested subset only; native workers unsupported |
| Frames/security | Complete WindowProxy/DOM policies in production SDK, opaque/sandbox navigation behavior | Stronger native proxy profile is experimental; default SDK coverage partial |
| CSP | strict-dynamic, report-only/reports/violations, navigation/form/worker/media directives | Enforced supported script/style directives tested; remaining directives open |
| Related resource policies | Full policy effects/reporting, passive upgrades, Integrity Policy and Critical-CH | Tested SRI, Trusted Types, mixed-content/CORP and COOP/COEP/Permissions Policy capability/delegation are integrated. Complete resource behavior remains open |
| Fetch lifecycle | Broader cancellation/cache/signals/referrer/preflight-cache behavior and genuine multipart BodyInit | Genuine response/HTTP2 upload streams, initiating policy snapshots, cache effects and stream transfers are integrated; bounded controls do not establish complete Fetch conformance |
| Cookies/privacy/storage | Complete storage-access, third-party browser policy and private-token APIs | HttpOnly cookie protection, actual partitioned/credentialless storage and peer/clear events are integrated; broader eligibility/policy remains open |
| Element interfaces | Exact HTML namespace/local-name mapping, native borrowed control brands and SVG interfaces | Canonical135 cohort passes the authored723-row matrix's HTML/null mappings and foreign brands. All SVG observations remain saved; SVGScriptElement/SVGImageElement projection/resources and broader custom-element lifecycle are open |
| HTML/XML parsing | Broader XML/sanitizer/template/parser lifecycle and UTF-16 edge cases | Published DOM0.5.6 native provenance/template ownership and original-unit storage are integrated, with cross-document/native observer/script controls |
| Structured clone | Complete web-interface serialization and transfer lists | Native V8 clone, genuine ArrayBuffer/resizable-buffer transfers, DOMException, private Rust-only CryptoKey snapshots, borrowed realms, User Timing, Window/MessagePort delivery and genuine interface rejection have saved Chrome controls. Current integrated transfer24/clone17 contracts pass. Endpoint weak anchors, reservations and queued native material enforce admission before ownership commit; retired context registries, timer roots and Window queues have six lifecycle contracts. Weak native ownership/cppgc tracing and private DOMRect snapshots are integrated; full native155 ownership and detached-node collection integration, Blob/File, SAB exposure and wider serialization remain open |
| BroadcastChannel | Shared cross-realm/tab/browser-session delivery and lifecycle | Local asynchronous delivery/brands tested |
| Range | Remaining composed-tree/lifecycle/geometry edge cases | Native live mutation boundaries and actual text/Range/caret geometry are integrated. AbstractRange/NodeRange/StaticRange passes independent native interfaces, iframe and GC controls |
| Canvas/media | Offscreen transfer, captureStream, audio/media synthesis and media lifecycle | Metal pixels match shared probe. Genuine bounded native offline audio now computes independent Chrome ARM64 PCM/hash controls, with private parameter/buffer ownership and trusted lifecycle tests; scheduled sources, automation and five effects are integrated locally after 32 exact authored ARM64 PCM controls and 474 native155 renderer/MCP tests in the isolated candidate. The current combined gate is pending; realtime, feedback, parameter-signal connections, custom waves, oversampling, suspend/resume and multichannel beyond two remain unsupported. Other media APIs remain incomplete |
| Crypto | Real Chrome 155 encapsulation/decapsulation/getPublicKey/supports behavior and supported algorithms | Official native Deno WebCrypto integrated; hostile inherited `keyData` setter regression discovered a raw-byte leak during non-extractable import, hardened licensed provider now passes inherited-property isolation; final native155/release gates pending. Covered operation controls: real digests, AES/HMAC/Ed25519, ML-KEM-768 and ML-KEM-1024 (512 rejected by the public Chrome profile), keys/permissions/realm metadata pass offline tests; paired 155 audit matches 285 operation/name and 14 parameter controls; current integrated gates pending |
| Hybrid KEM | Genuine MLKEM768-X25519 (X-Wing) key generation/import/export and encapsulation/decapsulation | Real native X-Wing managed keys, raw-seed/raw-public/JWK, permissions, wrapping and encapsulation implemented; known Chrome seed vector and both ciphertext directions pass. Public/native hybrid, child realms and bootstrap gate pass on genuine155; full integration/release checks pending |
| Browser identity/version | Real V8 155, measured transport 155, coherent UA/client hints/headers/environment | Actual native V815.5.35.23-rusty matches installed Chrome155.80; bounded local155 TLS/H2 fields use an upstream154 base recipe; stock150 and distribution alignment remain open |
| Enumeration | Global and Navigator property ownership/enumerability and behavior | Historical isolated pair compared insecure Chrome about:blank with secure native HTTPS; remeasure on the saved identical localhost fixture before treating those counts as a gap |
| Input execution | Stop at private input completion, bound synchronous runaway, cancel queued edits | Eight readiness/input regressions have passed profile cohorts. Private bootstrap arguments replace temporary public host globals; the full combined run exposed a termination race during frame setup, fixed with a checked lookup and current default regression pass. Final genuine155 combined rerun pending |
| Classic script errors | Preserve original exceptions, trusted Window reporting and fetched-script load success; mute no-CORS cross-origin reports | Current default main/iframe and CORS controls pass; fresh Chrome155 authored fixture saved. Genuine155 paired audit pending; Window onerror registry and wider error handling in progress |
| Element event propagation | Include actual Window in native node propagation path with capture/phase/stop/composedPath semantics | Canonical bf2a0ef passes 133 Rust and 63 JS tests, including private generic/Window/Node/Signal collection regressions. Generic EventTarget and AbortSignal listener/handler records are private; direct-target registration order follows fresh Chrome155 controls. Surf integration removes duplicate manual callbacks/DCL and public abort stubs; the earlier integrated native155 gate passed; six current event contracts pass; full default/native gates remain pending. Timeout/any, full composed-shadow retargeting and listener exception reporting remain open |
| Live search | Fresh consent+query+retry flow without imported Chrome cookies | Optimized544180d: both native baselines complete consent/save204 before search with genuine BUCKET cookie, then/search200→/sorry429 (0/2 SERP); Chrome baselines9 markers each, zero recorded errors. Native traced repeat0 has Missing promise; repeat1 reaches/sorry. Both native homes exhaust20s budget |
| Publishing | Clean registry consumers need compatible published DOM and transport APIs; verify per-platform artifacts | Latest published DOM0.5.6 and layout0.3.7 are distinct from pinned public DOM0.5.7/layout0.3.8 candidates; registry-ready transport APIs, distributable hardened crypto provider and per-platform Surf artifacts remain unresolved |

## Closure record

Keep historical observations dated in [FINDINGS.md](./FINDINGS.md). Update this
backlog and the sanitized API snapshot after each rebuilt paired measurement.
Regenerate the durable inventory with `node harness/detection/api-inventory.mjs
<capture>/comparison.json harness/detection/API-INVENTORY.json`. It preserves every
observed difference path, including globals, callable descriptors and raw key
order, alongside the missing-member summary and source/executable hashes.
Missing members tied to browser features cannot be closed by adding no-op methods,
fabricating native results, or changing a version string. Consent handling is a
saved default in run.mjs and both Chrome collectors, with a localhost regression;
native checkpoints retain partial state when execution is interrupted.
