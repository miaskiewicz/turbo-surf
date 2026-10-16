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
| Range / caret | Remaining lifecycle/composed geometry and fresh interface/receiver/GC validation | #28 |
| DOM | Parsing/observer edges, custom-element reactions/registries, broader form association/CustomStateSet lifecycle, UTF-16 edge cases | #28 |
| Forms | Genuine successful submission transport, entry lists, methods/encoding/targets and CSP/sandbox admission | #28 |
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

## Current integration checkpoint (2026-10-16)

Canonical turbo-test34ca9a8 is mechanically vendored with public DOM0.5.7
candidatefb60e6f (latest registry0.5.6). Genuine ElementInternals/CustomStateSet,
checked-state foundation, FormData events and disconnected-form admission are
integrated. The final native335 cohort, all-target workspace strict checks,
offline66 and2 full-driver localhost tests pass. Full final gates remain pending.

Public immutable layout producer0.3.8 candidate9d7f02fb is integrated; latest
published registry release is0.3.7. Actual V815.5.35.23-rusty matches installed
Chrome155.0.8059.80's V8 patch; SDK source provenance remains155.0.8059.79.
Stock production SDK150 remains a distinct incomplete configuration.

A fresh Google matrix failed all four native passes on missing ForcedColors media
capability during input geometry, before search submission. Chrome baseline got
9 results in both repeats; instrumented Chrome reached /sorry with capture errors.
Media capability repair is active. Reliable native Google acceptance is unverified.

## Active scope and deferred audio

Finish messaging, layout, Range/DOM, security/resources, fetch/storage, combined
validation, the current member inventory, PR housekeeping and fresh live Google
validation. Remaining advanced audio is a documented TODO in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27),
deferred at the user's request. Existing offline DSP and tests remain included.
Reprioritize deferred audio only with captured detector evidence showing a missing
path matters to the target; no Google-audio cause is established by these controls.

## Fresh own-member inventory (2026-10-16)

[API-INVENTORY.json](./API-INVENTORY.json) retains full missing/extra and descriptor
lists, actual Chrome/native versions and source/executable hashes. The latest
localhost pair uses Chrome155.0.8059.80/native V815.5.35.23 with all GPU/wreq features.
The original four interfaces have **65** missing own members (Document29,
Element30, HTMLElement4, Canvas2); expanded measured interfaces have **133**.
All measured interfaces are present. AbstractRange, NodeRange, StaticRange and
ElementInternals have zero own-member/descriptor differences in this capture.
There are304 total difference paths, including descriptors and other behavior.

Historical counts were94 (2026-10-13), then66 original/147 expanded before the latest
Range/forms work. None of these measurements is complete browser conformance.
Active follow-ups cover real form reflection and input/button prototype placement
and descriptors. Every remaining named path is tracked under issue28.

## Historical four-interface member differences

| Interface | Missing own member | Status |
|---|---|---|
| Document prototype | `all` | Open; verify behavior and ownership |
| Document prototype | `ariaNotify` | Open; verify behavior and ownership |
| Document prototype | `browsingTopics` | Open; verify behavior and ownership |
| Document prototype | `captureEvents` | Standard-defined legacy no-effects method matches saved Chrome155 descriptors/receiver controls; canonical tests pass, current native pair pending |
| Document prototype | `caretPositionFromPoint` | Open; verify behavior and ownership |
| Document prototype | `caretRangeFromPoint` | Open; verify behavior and ownership |
| Document prototype | `clear` | Standard-defined legacy no-effects method matches saved Chrome155 descriptors/receiver controls; canonical tests pass, current native pair pending |
| Document prototype | `createExpression` | Genuine bounded XPath 1.0 implementation and canonical regressions pass; latest native155 member audit pending |
| Document prototype | `createNSResolver` | Genuine bounded XPath 1.0 implementation and canonical regressions pass; latest native155 member audit pending |
| Document prototype | `customElementRegistry` | Open; verify behavior and ownership |
| Document prototype | `domain` | Open; verify behavior and ownership |
| Document prototype | `evaluate` | Genuine bounded XPath 1.0 implementation and canonical regressions pass; latest native155 member audit pending |
| Document prototype | `execCommand` | Open; verify behavior and ownership |
| Document prototype | `exitPictureInPicture` | Open; verify behavior and ownership |
| Document prototype | `exitPointerLock` | Open; verify behavior and ownership |
| Document prototype | `featurePolicy` | Open; verify behavior and ownership |
| Document prototype | `fragmentDirective` | Open; verify behavior and ownership |
| Document prototype | `fullscreen` | Open; verify behavior and ownership |
| Document prototype | `fullscreenEnabled` | Open; verify behavior and ownership |
| Document prototype | `getAnimations` | Open; verify behavior and ownership |
| Document prototype | `hasPrivateToken` | Open; verify behavior and ownership |
| Document prototype | `hasRedemptionRecord` | Open; verify behavior and ownership |
| Document prototype | `hasStorageAccess` | Open; verify behavior and ownership |
| Document prototype | `hasUnpartitionedCookieAccess` | Open; verify behavior and ownership |
| Document prototype | `lastModified` | Open; verify behavior and ownership |
| Document prototype | `pictureInPictureElement` | Open; verify behavior and ownership |
| Document prototype | `pictureInPictureEnabled` | Open; verify behavior and ownership |
| Document prototype | `pointerLockElement` | Open; verify behavior and ownership |
| Document prototype | `prerendering` | Open; verify behavior and ownership |
| Document prototype | `queryCommandEnabled` | Open; verify behavior and ownership |
| Document prototype | `queryCommandIndeterm` | Open; verify behavior and ownership |
| Document prototype | `queryCommandState` | Open; verify behavior and ownership |
| Document prototype | `queryCommandSupported` | Open; verify behavior and ownership |
| Document prototype | `queryCommandValue` | Open; verify behavior and ownership |
| Document prototype | `releaseEvents` | Standard-defined legacy no-effects method matches saved Chrome155 descriptors/receiver controls; canonical tests pass, current native pair pending |
| Document prototype | `requestStorageAccess` | Open; verify behavior and ownership |
| Document prototype | `startViewTransition` | Open; verify behavior and ownership |
| Document prototype | `timeline` | Open; verify behavior and ownership |
| Document prototype | `wasDiscarded` | Open; verify behavior and ownership |
| Document prototype | `webkitCancelFullScreen` | Open; verify behavior and ownership |
| Document prototype | `webkitExitFullscreen` | Open; verify behavior and ownership |
| Document prototype | `webkitFullscreenEnabled` | Open; verify behavior and ownership |
| Element prototype | `activeViewTransition` | Open; verify behavior and ownership |
| Element prototype | `animate` | Open; verify behavior and ownership |
| Element prototype | `ariaNotify` | Open; verify behavior and ownership |
| Element prototype | `assignedSlot` | Open; verify behavior and ownership |
| Element prototype | `checkVisibility` | Open; verify behavior and ownership |
| Element prototype | `clientHeight` | Open; verify behavior and ownership |
| Element prototype | `clientLeft` | Open; verify behavior and ownership |
| Element prototype | `clientTop` | Open; verify behavior and ownership |
| Element prototype | `clientWidth` | Open; verify behavior and ownership |
| Element prototype | `computedStyleMap` | Open; verify behavior and ownership |
| Element prototype | `currentCSSZoom` | Open; verify behavior and ownership |
| Element prototype | `customElementRegistry` | Open; verify behavior and ownership |
| Element prototype | `getAnimations` | Open; verify behavior and ownership |
| Element prototype | `getHTML` | Open; verify behavior and ownership |
| Element prototype | `hasPointerCapture` | Open; verify behavior and ownership |
| Element prototype | `onbeforecopy` | Open; verify behavior and ownership |
| Element prototype | `onbeforecut` | Open; verify behavior and ownership |
| Element prototype | `onbeforepaste` | Open; verify behavior and ownership |
| Element prototype | `onfullscreenchange` | Open; verify behavior and ownership |
| Element prototype | `onfullscreenerror` | Open; verify behavior and ownership |
| Element prototype | `onsearch` | Open; verify behavior and ownership |
| Element prototype | `onwebkitfullscreenchange` | Open; verify behavior and ownership |
| Element prototype | `onwebkitfullscreenerror` | Open; verify behavior and ownership |
| Element prototype | `pseudo` | Open; verify behavior and ownership |
| Element prototype | `releasePointerCapture` | Open; verify behavior and ownership |
| Element prototype | `requestPointerLock` | Open; verify behavior and ownership |
| Element prototype | `scroll` | Open; verify behavior and ownership |
| Element prototype | `scrollHeight` | Open; verify behavior and ownership |
| Element prototype | `scrollIntoViewIfNeeded` | Open; verify behavior and ownership |
| Element prototype | `scrollLeft` | Open; verify behavior and ownership |
| Element prototype | `scrollTop` | Open; verify behavior and ownership |
| Element prototype | `scrollWidth` | Open; verify behavior and ownership |
| Element prototype | `setHTML` | Open; verify behavior and ownership |
| Element prototype | `setHTMLUnsafe` | Open; verify behavior and ownership |
| Element prototype | `setPointerCapture` | Open; verify behavior and ownership |
| Element prototype | `startViewTransition` | Open; verify behavior and ownership |
| Element prototype | `webkitMatchesSelector` | Open; verify behavior and ownership |
| Element prototype | `webkitRequestFullScreen` | Open; verify behavior and ownership |
| Element prototype | `webkitRequestFullscreen` | Open; verify behavior and ownership |
| HTMLCanvasElement prototype | `captureStream` | Open; verify behavior and ownership |
| HTMLCanvasElement prototype | `transferControlToOffscreen` | Open; verify behavior and ownership |
| HTMLElement prototype | `attachInternals` | Open; verify behavior and ownership |
| HTMLElement prototype | `attributeStyleMap` | Open; verify behavior and ownership |
| HTMLElement prototype | `editContext` | Open; verify behavior and ownership |
| HTMLElement prototype | `focusGroup` | Open; verify behavior and ownership |
| HTMLElement prototype | `focusGroupStart` | Open; verify behavior and ownership |
| HTMLElement prototype | `hidePopover` | Native lifecycle passes saved Chrome155 control; latest member audit pending |
| HTMLElement prototype | `offsetLeft` | Open; verify behavior and ownership |
| HTMLElement prototype | `offsetParent` | Open; verify behavior and ownership |
| HTMLElement prototype | `offsetTop` | Open; verify behavior and ownership |
| HTMLElement prototype | `showPopover` | Native lifecycle passes saved Chrome155 control; latest member audit pending |
| HTMLElement prototype | `togglePopover` | Native lifecycle passes saved Chrome155 control; latest member audit pending |

Total: **94 observed missing own members across four groups**: Document 42,
Element 39, HTMLCanvasElement two and HTMLElement 11. No extra native own members
occur in these groups. The earlier V8 154 inventory had 121 members across six
groups; the 27 now matching include scoped ARIA relationships, token/part lists,
state-preserving moves, text/popover reflection and Crypto methods. The collector
is bounded; it is not a census of every browser interface.

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
| Browser identity/version | Real V8 155, measured transport 155, coherent UA/client hints/headers/environment | Genuine 155 native SDK validated at 15.5.35.20-rusty; bounded local 155 TLS/H2 fields match with upstream 154 base recipe; integrated runtime gates pending |
| Enumeration | Global and Navigator property ownership/enumerability and behavior | Historical isolated pair compared insecure Chrome about:blank with secure native HTTPS; remeasure on the saved identical localhost fixture before treating those counts as a gap |
| Input execution | Stop at private input completion, bound synchronous runaway, cancel queued edits | Eight readiness/input regressions have passed profile cohorts. Private bootstrap arguments replace temporary public host globals; the full combined run exposed a termination race during frame setup, fixed with a checked lookup and current default regression pass. Final genuine155 combined rerun pending |
| Classic script errors | Preserve original exceptions, trusted Window reporting and fetched-script load success; mute no-CORS cross-origin reports | Current default main/iframe and CORS controls pass; fresh Chrome155 authored fixture saved. Genuine155 paired audit pending; Window onerror registry and wider error handling in progress |
| Element event propagation | Include actual Window in native node propagation path with capture/phase/stop/composedPath semantics | Canonical bf2a0ef passes 133 Rust and 63 JS tests, including private generic/Window/Node/Signal collection regressions. Generic EventTarget and AbortSignal listener/handler records are private; direct-target registration order follows fresh Chrome155 controls. Surf integration removes duplicate manual callbacks/DCL and public abort stubs; the earlier integrated native155 gate passed; six current event contracts pass; full default/native gates remain pending. Timeout/any, full composed-shadow retargeting and listener exception reporting remain open |
| Live search | Fresh consent+query+retry flow without imported Chrome cookies | Chrome consent control returns7; native verified SERP still unresolved |
| Publishing | Clean registry consumers need compatible published DOM and transport APIs; verify per-platform artifacts | turbo-dom 0.5.3 selected;0.5.4 detached storage published and verified from PR10, consumer integration in progress; unpublished transport APIs, distributable hardened crypto provider and per-platform Surf artifacts remain unresolved |

## Closure record

Keep historical observations dated in [FINDINGS.md](./FINDINGS.md). Update this
backlog and the sanitized API snapshot after each rebuilt paired measurement.
Missing members tied to browser features cannot be closed by adding no-op methods,
fabricating native results, or changing a version string. Consent handling is a
saved default in run.mjs and both Chrome collectors, with a localhost regression;
native checkpoints retain partial state when execution is interrupted.
