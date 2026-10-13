# Browser parity backlog

This is the explicit backlog for PR #26. The measured member inventory comes
from [API-SHAPE.json](./API-SHAPE.json), collected on 2026-10-13 against real
Chrome 155.0.8059.40 / V8 15.5.35.20 and native V8 15.4.80.20-rusty.
A missing **own prototype member** is an observed descriptor/ownership difference;
it does not by itself prove the feature is entirely absent. Existing inherited
methods and aliases must be tested before adding another implementation.
A repair is closed only after behavior tests and a fresh paired audit pass.

## Every measured member difference

| Interface | Missing own member | Status |
|---|---|---|
| Document prototype | `Symbol(Symbol.unscopables)` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Document prototype | `all` | Open; verify behavior and ownership |
| Document prototype | `ariaNotify` | Open; verify behavior and ownership |
| Document prototype | `browsingTopics` | Open; verify behavior and ownership |
| Document prototype | `captureEvents` | Open; verify behavior and ownership |
| Document prototype | `caretPositionFromPoint` | Open; verify behavior and ownership |
| Document prototype | `caretRangeFromPoint` | Open; verify behavior and ownership |
| Document prototype | `clear` | Open; verify behavior and ownership |
| Document prototype | `createExpression` | Open; verify behavior and ownership |
| Document prototype | `createNSResolver` | Open; verify behavior and ownership |
| Document prototype | `customElementRegistry` | Open; verify behavior and ownership |
| Document prototype | `domain` | Open; verify behavior and ownership |
| Document prototype | `evaluate` | Open; verify behavior and ownership |
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
| Document prototype | `moveBefore` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Document prototype | `pictureInPictureElement` | Open; verify behavior and ownership |
| Document prototype | `pictureInPictureEnabled` | Open; verify behavior and ownership |
| Document prototype | `pointerLockElement` | Open; verify behavior and ownership |
| Document prototype | `prerendering` | Open; verify behavior and ownership |
| Document prototype | `queryCommandEnabled` | Open; verify behavior and ownership |
| Document prototype | `queryCommandIndeterm` | Open; verify behavior and ownership |
| Document prototype | `queryCommandState` | Open; verify behavior and ownership |
| Document prototype | `queryCommandSupported` | Open; verify behavior and ownership |
| Document prototype | `queryCommandValue` | Open; verify behavior and ownership |
| Document prototype | `releaseEvents` | Open; verify behavior and ownership |
| Document prototype | `requestStorageAccess` | Open; verify behavior and ownership |
| Document prototype | `startViewTransition` | Open; verify behavior and ownership |
| Document prototype | `timeline` | Open; verify behavior and ownership |
| Document prototype | `wasDiscarded` | Open; verify behavior and ownership |
| Document prototype | `webkitCancelFullScreen` | Open; verify behavior and ownership |
| Document prototype | `webkitCurrentFullScreenElement` | Open; verify behavior and ownership |
| Document prototype | `webkitExitFullscreen` | Open; verify behavior and ownership |
| Document prototype | `webkitFullscreenElement` | Open; verify behavior and ownership |
| Document prototype | `webkitFullscreenEnabled` | Open; verify behavior and ownership |
| Document prototype | `webkitHidden` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Document prototype | `webkitIsFullScreen` | Open; verify behavior and ownership |
| Document prototype | `webkitVisibilityState` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `Symbol(Symbol.unscopables)` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `activeViewTransition` | Open; verify behavior and ownership |
| Element prototype | `animate` | Open; verify behavior and ownership |
| Element prototype | `ariaActionsElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaActiveDescendantElement` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaControlsElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaDescribedByElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaDetailsElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaErrorMessageElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaFlowToElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `ariaLabelledByElements` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
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
| Element prototype | `moveBefore` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| Element prototype | `onbeforecopy` | Open; verify behavior and ownership |
| Element prototype | `onbeforecut` | Open; verify behavior and ownership |
| Element prototype | `onbeforepaste` | Open; verify behavior and ownership |
| Element prototype | `onfullscreenchange` | Open; verify behavior and ownership |
| Element prototype | `onfullscreenerror` | Open; verify behavior and ownership |
| Element prototype | `onsearch` | Open; verify behavior and ownership |
| Element prototype | `onwebkitfullscreenchange` | Open; verify behavior and ownership |
| Element prototype | `onwebkitfullscreenerror` | Open; verify behavior and ownership |
| Element prototype | `part` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
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
| HTMLElement prototype | `hidePopover` | Open; verify behavior and ownership |
| HTMLElement prototype | `innerText` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| HTMLElement prototype | `offsetLeft` | Open; verify behavior and ownership |
| HTMLElement prototype | `offsetParent` | Open; verify behavior and ownership |
| HTMLElement prototype | `offsetTop` | Open; verify behavior and ownership |
| HTMLElement prototype | `outerText` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| HTMLElement prototype | `popover` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| HTMLElement prototype | `showPopover` | Open; verify behavior and ownership |
| HTMLElement prototype | `togglePopover` | Open; verify behavior and ownership |
| SubtleCrypto prototype | `decapsulateBits` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| SubtleCrypto prototype | `decapsulateKey` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| SubtleCrypto prototype | `encapsulateBits` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| SubtleCrypto prototype | `encapsulateKey` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| SubtleCrypto prototype | `getPublicKey` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |
| SubtleCrypto constructor | `supports` | Implemented supported contracts; fresh 155 ownership/behavior audit pending |

Total: **121 observed missing own members across six groups**. No extra
native members occur in these six groups. This collector is bounded; it is not
an exhaustive census of all browser interfaces.

## Behavior and architecture gaps beyond member names

| Surface | Concrete remaining work | Evidence / status |
|---|---|---|
| DOMTokenList | Stable identity, private brand, indexed/live tokens, mutation/error contracts; `part` | Canonical cached/live implementation passes offline mutation/brand contracts; fresh 155 pair pending |
| ARIA element relationships | ID resolution, tree scope, explicit element assignment, attribute clearing, frozen returned sequences | Scoped/private canonical state implemented and tested; fresh 155 pair pending |
| Layout and hit testing | Real CSS box rectangles, client/scroll/offset geometry, caret/element hit testing | Zero-layout compatibility rectangles; native font measurement is not a layout engine |
| Rendered text | Complete layout-dependent wrapping/line-box behavior beyond native cascade text flow | Canonical visibility/whitespace/break/table/details/setter contracts implemented; fresh 155 pair pending |
| Shadow/custom elements | Slots and assigned nodes, ElementInternals, scoped registries, complete upgrade lifecycle | Partial shadow isolation only |
| CSS parsing/layout | Complete grammar/cascade, Typed OM, animation timelines and transitions | Measured stylesheet policy controls pass; complete CSS behavior is open |
| Live stylesheet installation | Explain Google's jc/lc reports during actual execution; compare live ownership and rule state | Minimal authored injection tests pass; reusable CSS diagnostics saved |
| Observers | Real Mutation/Resize/Intersection/Performance observer queues and lifecycle | Existing compatibility implementations are synthetic |
| Browser lifecycle/resources | Complete parsing/resource scheduling, frame navigation, media and native worker lifecycles | Tested subset only; native workers unsupported |
| Frames/security | Complete WindowProxy/DOM policies in production SDK, opaque/sandbox navigation behavior | Stronger native proxy profile is experimental; default SDK coverage partial |
| CSP | strict-dynamic, report-only/reports/violations, navigation/form/worker/media directives | Enforced supported script/style directives tested; remaining directives open |
| Related resource policies | SRI, Trusted Types, mixed content, CORP/COEP/COOP, permissions policy, Critical-CH | Open |
| Fetch lifecycle | Cancellation, native cache/integrity/keepalive/referrer-policy effects, streaming, preflight cache | Some properties reflect without implementing native effects |
| Cookies/privacy/storage | Complete storage-access, partitioning, third-party browser policy/private-token APIs | HttpOnly/SameSite/host-only/default-path/prefix checks pass; broader policy open |
| HTML/XML parsing | XML documents, custom sanitizer configuration, full template behavior, lossless unpaired UTF-16 at Rust boundary | Supported native HTML/Attr/PI/import/adopt subset tested |
| Structured clone | Complete web-interface serialization and transfer lists | Native V8 clone implemented; transfer lists explicitly unsupported |
| BroadcastChannel | Shared cross-realm/tab/browser-session delivery and lifecycle | Local asynchronous delivery/brands tested |
| Canvas/media | Offscreen transfer, captureStream, audio/media synthesis and media lifecycle | Metal pixels match shared probe; media APIs incomplete |
| Crypto | Real Chrome 155 encapsulation/decapsulation/getPublicKey/supports behavior and supported algorithms | Official native Deno WebCrypto integrated: real digests, AES/HMAC/Ed25519, all three ML-KEM variants, keys/permissions/realm metadata pass offline tests; paired 155 audit pending |
| Hybrid KEM | Genuine MLKEM768-X25519 (X-Wing) key generation/import/export and encapsulation/decapsulation | Chrome 155 supports it; current native backend lacks it. Real SHAKE256/ML-KEM/X25519/SHA3 composition has matched Chrome's known-seed public-key vector in a primitive proof; managed native key integration remains open |
| Browser identity/version | Real V8 155, measured transport 155, coherent UA/client hints/headers/environment | Genuine 155 native SDK validated at 15.5.35.20-rusty; bounded local 155 TLS/H2 fields match with upstream 154 base recipe; integrated runtime gates pending |
| Enumeration | Global and Navigator property ownership/enumerability and behavior | Historical isolated pair compared insecure Chrome about:blank with secure native HTTPS; remeasure on the saved identical localhost fixture before treating those counts as a gap |
| Input execution | Stop at private input completion, bound synchronous runaway, cancel queued edits | Six meaningful regressions pass on production SDK and native154; full integration rerun pending |
| Live search | Fresh consent+query+retry flow without imported Chrome cookies | Chrome consent control returns7; native verified SERP still unresolved |
| Publishing | Clean registry consumers need compatible published DOM and transport APIs; verify per-platform artifacts | turbo-dom 0.5.2 published and selected; unpublished transport APIs and per-platform Surf artifacts remain unresolved |

## Closure record

Keep historical observations dated in [FINDINGS.md](./FINDINGS.md). Update this
backlog and the sanitized API snapshot after each rebuilt paired measurement.
Missing members tied to browser features cannot be closed by adding no-op methods,
fabricating native results, or changing a version string. Consent handling is a
saved default in run.mjs and both Chrome collectors, with a localhost regression;
native checkpoints retain partial state when execution is interrupted.
