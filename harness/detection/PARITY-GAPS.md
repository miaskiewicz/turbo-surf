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
| Forms | Radio/adoption semantics, live form collections, FileList/multipart, other targets and broader encoding/submitter/policy cases | #28 |
| User activation | Consuming APIs, cross-frame propagation, broader trusted input, MCP reconstruction continuity and retained/retired receiver or cross-origin-return redirect behavior | #28 |
| Events / promises | Stream/listener microtask interleaving and broader Object-prototype descriptor hooks during abort/event dispatch and exception reporting | #28 |
| CSS / resources | Remaining selectors/at-rules, used downloaded fonts/animations, images/pseudo epochs, inner SVG | #28 |
| Security | Remaining CSP/reporting/sinks, popup/navigation/sandbox, COOP/COEP and permissions policies | #28 |
| Fetch / storage | Remaining streaming/cancellation/cache/signals/referrer and partition/storage-access behavior | #28 |
| API inventory | Every unresolved missing/extra member and descriptor/inheritance/behavior path in API-INVENTORY.json; fresh paired measurement | #28 |
| Distribution | Stock/native V8 alignment, native patch distribution, layout publication and registry-ready transport/crypto dependencies | #28 |
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

## Current integration checkpoint (2026-10-16)

Canonical turbo-test2e27274 is mechanically vendored with public DOM0.5.7
candidatefb60e6f (latest registry0.5.6). Genuine ElementInternals/CustomStateSet,
checked-state foundation, FormData events, disconnected-form admission and native
form reflection are integrated. The current affected native renderer cohort passes
385 tests with zero failures/ignores, including312 library and73 integration tests.
Full all-target actual155 workspace strict checks pass. The earlier native MCP
checkpoint passes72/default71 tests; full final workspace/addon/shim/E2E gates
remain separate and are in progress.

Public immutable layout producer0.3.8 candidate9d7f02fb is integrated; latest
published registry release is0.3.7. Actual V815.5.35.23-rusty matches installed
Chrome155.0.8059.80's V8 patch; SDK source provenance remains155.0.8059.79.
Stock production SDK150 remains a distinct incomplete configuration.

The host-media and modern-CSS input failures are repaired. The latest completed
optimized155 matrix has both native runs submit genuine forms, receive/search200
and follow page-script retry to/sorry429. Consent/save204 now occurs, but its
response arrives after search starts; the harness's virtual waits allowed the
original document to be replaced before its HttpOnly response cookie was merged.
The real-elapsed-wait repair passes its delayed-XHR localhost regression and awaits
fresh live validation. Native homepage hydration still exceeds20s; a60s diagnostic
also rejects. Chrome baseline has7 markers and zero recorded errors; its traced
run reaches/sorry. See [latest run](./GOOGLE-LAST-RUN.json). Reliable native
acceptance remains unverified.

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
lists, actual Chrome/native versions and source/executable hashes. The latest
localhost pair uses Chrome155.0.8059.80/native V815.5.35.23 with all GPU/wreq features.
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
snapshot. The latest original-four count is65. Expanded counts now cover69
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
| Live search | Fresh consent+query+retry flow without imported Chrome cookies | Native baseline genuinely submits/search200 then retries/sorry429; instrumented native remains hydration-incomplete. Chrome baseline has7 markers with one capture error |
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
