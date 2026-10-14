# Browser parity backlog

This is the explicit backlog for PR #26. The measured member inventory comes
from [API-SHAPE.json](./API-SHAPE.json), collected on 2026-10-13 against real
Chrome 155.0.8059.40 / V8 15.5.35.20 and native V8 15.5.35.20-rusty (frozen executable SHA-256
`898b3532c20030ac0d68707b19a13cca7d465e034b9c144d7965bbcd5b65e1c1`).
A missing **own prototype member** is an observed descriptor/ownership difference;
it does not by itself prove the feature is entirely absent. Existing inherited
methods and aliases must be tested before adding another implementation.
A repair is closed only after behavior tests and a fresh paired audit pass.


## Current integration checkpoint (2026-10-14)

The member counts above are historical measurements and have not been recomputed
for the current local source. Canonical Range content operations, advanced offline
audio and native Window/MessagePort transfers are now integrated locally. The
new full default/native 155 gates and rebuilt paired inventory are pending. Full
end-of-work validation is deferred while items 1–4 and 6–7 are implemented; focused
behavior checks continue. The latest
integrated follow-up passes 73 focused Rust and 40 harness tests; the prior batch
passed 848 default workspace tests plus strict Clippy/format. The
published turbo-dom 0.5.3 consumer passes 35 focused adoption, frame ownership,
transfer and teardown contracts. The
native independent document factories and XML local-name/default-namespace helpers
are integrated locally (117 canonical/37 affected Surf contracts pass). The
newest layout control uses actual Chrome 155.0.8059.79 / V8 15.5.35.23; earlier
controls and the development SDK use 155.0.8059.40 / 15.5.35.20. This patch mismatch
remains explicit. A saved oracle is evidence for its exact authored behavior,
not complete browser parity or a verified live SERP.

## Active scope and deferred audio

Finish messaging, layout, Range/DOM, security/resources, fetch/storage, combined
validation, the current member inventory, PR housekeeping and fresh live Google
validation. Remaining advanced audio is a documented TODO in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27),
deferred at the user's request. Existing offline DSP and tests remain included.
Reprioritize deferred audio only with captured detector evidence showing a missing
path matters to the target; no Google-audio cause is established by these controls.

## Every measured member difference

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
| Layout and hit testing | Real CSS box rectangles, client/scroll/offset geometry, caret/element hit testing | DOM compatibility rectangles do not use the existing native screenshot layout. The [source audit and required seam](./LAYOUT.md) identify lost inline/text provenance, overflow/scroll state and frame metrics; native text measurement cannot supply those boxes |
| Rendered text | Complete layout-dependent wrapping/line-box behavior beyond native cascade text flow | Canonical visibility/whitespace/break/table/details/setter contracts implemented; fresh native 155 pair has zero differences for the saved probe |
| Shadow/custom elements | Slots and assigned nodes, ElementInternals, scoped registries, complete upgrade lifecycle | Partial shadow isolation only |
| CSS parsing/layout | Complete grammar/cascade, Typed OM, animation timelines and transitions | Measured stylesheet policy controls pass; complete CSS behavior is open |
| Live stylesheet installation | Explain Google's jc/lc reports during actual execution; compare live ownership and rule state | Minimal authored injection tests pass; reusable CSS diagnostics saved |
| Observers | Real Mutation/Resize/Intersection/Performance observer queues and lifecycle | Existing compatibility implementations are synthetic |
| Browser lifecycle/resources | Complete parsing/resource scheduling, frame navigation, media and native worker lifecycles | Tested subset only; native workers unsupported |
| Frames/security | Complete WindowProxy/DOM policies in production SDK, opaque/sandbox navigation behavior | Stronger native proxy profile is experimental; default SDK coverage partial |
| CSP | strict-dynamic, report-only/reports/violations, navigation/form/worker/media directives | Enforced supported script/style directives tested; remaining directives open |
| Related resource policies | SRI, Trusted Types, mixed content, CORP/COEP/COOP, permissions policy, Critical-CH | SRI raw-byte/strongest/opaque/tampering, blocked stylesheet ownership and iframe errors pass native tests. CORP per-hop origin/site/header policy and active mixed-content admission are implemented; paired full-resource audit pending. COEP/COOP, passive image upgrades, Permissions/Integrity Policy and additional directives remain open |
| Fetch lifecycle | Cancellation, native cache/integrity/keepalive/referrer-policy effects, streaming, preflight cache | Some properties reflect without implementing native effects |
| Cookies/privacy/storage | Complete storage-access, partitioning, third-party browser policy/private-token APIs | HttpOnly/SameSite/host-only/default-path/prefix checks pass; broader policy open |
| HTML/XML parsing | XML documents, custom sanitizer configuration, full template behavior, lossless unpaired UTF-16 at Rust boundary | Supported native HTML/Attr/PI/import/adopt subset tested; bf2a0ef plus four Surf contracts cover cross-document identity/realms, fragment overlap, variadic errors, genuine frame retirement and repeated-bootstrap traversal |
| Structured clone | Complete web-interface serialization and transfer lists | Native V8 clone, genuine ArrayBuffer/resizable-buffer transfers, DOMException, private Rust-only CryptoKey snapshots, borrowed realms, User Timing, Window/MessagePort delivery and genuine interface rejection have saved Chrome controls. Current integrated transfer24/clone17 contracts pass. Endpoint weak anchors, reservations and queued native material enforce admission before ownership commit; retired context registries, timer roots and Window queues have six lifecycle contracts. Full realm/node GC, DOMRect integration, Blob/File, SAB exposure and wider serialization remain open |
| BroadcastChannel | Shared cross-realm/tab/browser-session delivery and lifecycle | Local asynchronous delivery/brands tested |
| Range | Mutation-adjusted boundaries and actual geometry | Canonical bf2a0ef implements content operations and private weak live boundaries through text edits/splits, insertion/removal/replacement, normalization, moves/adoption, adjacent HTML, outerHTML and innerText/outerText. Independent Chrome controls cover removal versus text-merge semantics. 133 Rust and 63 JS canonical tests pass; source is vendored locally. Geometry, broader parsing mutation seams and lossless unpaired UTF-16 remain incomplete |
| Canvas/media | Offscreen transfer, captureStream, audio/media synthesis and media lifecycle | Metal pixels match shared probe. Genuine bounded native offline audio now computes independent Chrome ARM64 PCM/hash controls, with private parameter/buffer ownership and trusted lifecycle tests; scheduled sources, automation and five effects are integrated locally after 32 exact authored ARM64 PCM controls and 474 native155 renderer/MCP tests in the isolated candidate. The current combined gate is pending; realtime, feedback, parameter-signal connections, custom waves, oversampling, suspend/resume and multichannel beyond two remain unsupported. Other media APIs remain incomplete |
| Crypto | Real Chrome 155 encapsulation/decapsulation/getPublicKey/supports behavior and supported algorithms | Official native Deno WebCrypto integrated; hostile inherited `keyData` setter regression discovered a raw-byte leak during non-extractable import, hardened licensed provider now passes inherited-property isolation; final native155/release gates pending. Covered operation controls: real digests, AES/HMAC/Ed25519, ML-KEM-768 and ML-KEM-1024 (512 rejected by the public Chrome profile), keys/permissions/realm metadata pass offline tests; paired 155 audit matches 285 operation/name and 14 parameter controls; current integrated gates pending |
| Hybrid KEM | Genuine MLKEM768-X25519 (X-Wing) key generation/import/export and encapsulation/decapsulation | Real native X-Wing managed keys, raw-seed/raw-public/JWK, permissions, wrapping and encapsulation implemented; known Chrome seed vector and both ciphertext directions pass. Public/native hybrid, child realms and bootstrap gate pass on genuine155; full integration/release checks pending |
| Browser identity/version | Real V8 155, measured transport 155, coherent UA/client hints/headers/environment | Genuine 155 native SDK validated at 15.5.35.20-rusty; bounded local 155 TLS/H2 fields match with upstream 154 base recipe; integrated runtime gates pending |
| Enumeration | Global and Navigator property ownership/enumerability and behavior | Historical isolated pair compared insecure Chrome about:blank with secure native HTTPS; remeasure on the saved identical localhost fixture before treating those counts as a gap |
| Input execution | Stop at private input completion, bound synchronous runaway, cancel queued edits | Eight readiness/input regressions have passed profile cohorts. Private bootstrap arguments replace temporary public host globals; the full combined run exposed a termination race during frame setup, fixed with a checked lookup and current default regression pass. Final genuine155 combined rerun pending |
| Classic script errors | Preserve original exceptions, trusted Window reporting and fetched-script load success; mute no-CORS cross-origin reports | Current default main/iframe and CORS controls pass; fresh Chrome155 authored fixture saved. Genuine155 paired audit pending; Window onerror registry and wider error handling in progress |
| Element event propagation | Include actual Window in native node propagation path with capture/phase/stop/composedPath semantics | Canonical bf2a0ef passes 133 Rust and 63 JS tests, including private generic/Window/Node/Signal collection regressions. Generic EventTarget and AbortSignal listener/handler records are private; direct-target registration order follows fresh Chrome155 controls. Surf integration removes duplicate manual callbacks/DCL and public abort stubs; the earlier integrated native155 gate passed; six current event contracts pass; full default/native gates remain pending. Timeout/any, full composed-shadow retargeting and listener exception reporting remain open |
| Live search | Fresh consent+query+retry flow without imported Chrome cookies | Chrome consent control returns7; native verified SERP still unresolved |
| Publishing | Clean registry consumers need compatible published DOM and transport APIs; verify per-platform artifacts | turbo-dom 0.5.2 published and selected; unpublished transport APIs, distributable hardened crypto provider and per-platform Surf artifacts remain unresolved |

## Closure record

Keep historical observations dated in [FINDINGS.md](./FINDINGS.md). Update this
backlog and the sanitized API snapshot after each rebuilt paired measurement.
Missing members tied to browser features cannot be closed by adding no-op methods,
fabricating native results, or changing a version string. Consent handling is a
saved default in run.mjs and both Chrome collectors, with a localhost regression;
native checkpoints retain partial state when execution is interrupted.
