# Native resource lifecycle checkpoint

The authored collectors in this directory compare actual normal Chrome155 behavior
against deterministic localhost native regressions. See [README.md](./README.md)
for commands and [FINDINGS.md](./FINDINGS.md) for current certification bounds.

## Pending task ownership

Resource preparation never stores an unwrapped tree node handle across host turns.
Its document-private plain state is an ordinary V8 edge owned by that document;
the canonical trusted mutation consumer is also document scoped. Retirement
removes the resource consumer and disables preparation. A replacement document-owned
metadata-only consumer maintains retained inactive documents' base URL behavior,
without executing or preparing scripts.

| Root | Purpose | Release boundary |
| --- | --- | --- |
| Classic job execution and caller Context globals | Compile in the authentic realm and settle the caller's completion | Job completion, interruption or runtime teardown |
| Classic job original Document global | Preserve the preparation document and its private currentScript record | Job completion, interruption or runtime teardown |
| Optional script element global | Preserve the actual executing script; shadow classics expose null | Job completion, interruption or runtime teardown |
| Pending native PromiseResolver global | Settle exactly that queued script without calling JS completion helpers | Job completion, interruption or runtime teardown |
| Bootstrap script wrapper array | Admit parser scripts once without later DOM rescans | Consumed by initial hydration; cleared on retirement |
| Frame script wrapper array | Authenticate indexed frame script execution and resource event targets | Cleared after all frame scripts/resources finish, or on retirement |
| Private frozen-base record | Keep the actual first active base and its frozen URL with its Document | Replaced when native mutations select another base; ephemeron dies with Document |
| Dynamic resource promise/closure | Keep a genuinely active script/resource request and its target alive | Promise settlement or runtime teardown |

The currentScript previous value and original exception globals are transient
execution roots. Native frame_document/frame_script integers name authenticated
response byte records, not DOM nodes. A frame script index names a captured wrapper
array entry, not a Tree node. Genuine Global wrapper roots expose their document
ownership to the canonical weak native ownership registry. This resource batch
therefore needs no additional NativeNodePin for pending scalar handles.

Interrupted inline execution restores the private currentScript field without
running guest code. Queued execution records termination before TryCatch teardown,
restores the same private field and rejects its native resolver. Temporary
termination cancellation permits only native operations; guest reactions run under
a later ordinary budget. Retired queued jobs are rejected by the owning context's
retirement check when admitted by the pump.

## Measured coverage and remaining work

The default SDK150 checkpoint passes 38 resource/script/frame/input integration
contracts, six native response-policy contracts and one transport credentialless
redirect contract. The independently committed canonical computed-color cohort
passes 136 binding contracts. Those numbers describe the isolated checkpoint;
final integrated workspace and genuine native155 gates remain separate.

Saved controls use Chrome155.0.8059.79 / V8 15.5.35.23 and source hashes, with 54
direct COEP fetch cases, 15 DOM requests, 12 redirect cases/23 hops, inherited
srcdoc policies and 12 structured policy header syntaxes. Tests match original
exception realms, load/error distinction, SRI rejection, private linked colors,
opaque stylesheet access and actual Cookie headers.

CSP base-uri, dynamic head metas and parser completion now have four fresh authored
controls, including the distinct parser/compound-fragment timing and retired-document
fallback. Private base records retain only the genuine current first base element;
replacement/removal releases the previous element at the original mutation boundary.

Still open: form-action, SecurityPolicyViolationEvent/reporting, COOP/isolation and navigation embedding compatibility, report-only and
reporting endpoints, permissions policy, full parser/document.write lifecycle,
import maps/speculation rules, async adoption/navigation races, full script stack
source parity, complete CSS cascade/color-scheme, and wider resource cancellation
and scheduling. Passing this checkpoint does not establish Google clearance or
complete browser parity.

### Shared memory and Permissions Policy controls

The default document now hides the SharedArrayBuffer constructor but permits shared WebAssembly allocation. Local cloning rejects shared memory during deserialization, after regular ArrayBuffer transfers have completed. Window/MessagePort messaging rejects shared memory during serialization. Private native capability checks prevent forged public flags or constructors from granting sharing. The authored `shared-memory-unisolated-probe.js` records this distinction in fresh normal Chrome.

The PermissionsPolicy interface has private native document ownership, genuine receiver checks and Chrome 155 descriptors; FeaturePolicy aliases its constructor. The response-header parser uses RFC 8941 and preserves the measured known-feature defaults and explicit origin lists. The 21 authored header controls include unknown tokens, denied lists, foreign origins, duplicate directives, and invalid structured-field syntax. This batch covers top-level policy reflection; iframe inheritance/delegation, enforced feature effects and positive isolation/embedding remain pending. Native policy records contain strings and booleans; they queue no native DOM handles and require no additional NativeNodePin.

### Native isolation and credentialless state

The subsequent isolation batch enforces actual COOP/COEP header parsing, iframe embedding and inherited Permissions Policy before enabling shared memory. Native group/origin snapshot ownership controls deserialization; Window and MessagePort failures dispatch messageerror asynchronously. Shared snapshots account fixed admitted backing-store bytes, so WASM growth cannot over-release another queued message's accounting. Capability and origin records contain native scalars/strings and queue no DOM handles.

Credentialless iframe navigation derives a private partition from its actual owning document and genuine boolean attribute. The partition owns a CookieJar and backing local/session storage nonce; descendant resource requests reuse it. Credentialless sibling frames share it, ordinary frames retain the default jar/storage, and native partition teardown reclaims only its storage areas. Network snapshots retain the partition through pending work as an Rc containing Rust state; no NativeNodePin is required.

Reusable controls: isolation-probe/policy-header-chrome (42 headers), policy-frame-chrome with policy-frame-probe/policy-frame-child (79 iframe cases), policy-shared-probe/policy-shared-child (8 actual shared deliveries), policy-credentialless-probe/policy-credentialless-child (cookie/storage partition), and permissions-effects-probe (3 actual denied query sets). The header/frame collectors create fresh normal Chrome profiles and save source/case/collector hashes. Optional fourth child-source argument selects the exact child program for shared/credentialless runs. Full browser policies remain partial as recorded in FINDINGS.md.

### Retained Window message queue limits

The native cross-context Window snapshot queue has a4096-message/64MiB default capacity. A captured payload can enqueue recursively or retire its destination while serializing. Admission checks the actual current queue after those getters and before native ownership commits, with no guest callback between check and commit. Failure preserves ordinary ArrayBuffer and MessagePort ownership. Retired/origin-mismatched receivers still complete specified transfers and discard delivery without consuming capacity. Fixed shared snapshot accounting remains stable after shared WASM growth. Host-only test setters are not exposed to page JS.

This bounds the retained native cross-context snapshots; same-context Window deliveries still use the existing private V8 timer/event path. No new queued DOM scalar handles or NativeNodePin are introduced. The queue's existing genuine context Globals and native clone roots retain task ownership and are released by delivery/retirement.

### Permission interface lifetime and mutable containers

PermissionStatus records are per-document private ephemeron entries retaining the authentic owner Document. They inherit a real EventTarget; registered user listeners and onchange handlers use the existing V8-owned event target mechanism. Iframe featurePolicy records retain their genuine host element and query native attributes with captured owner/base accessors. No native integer DOM handles are queued or retained by these records. Changing the current container allow attribute updates its reflected policy; the committed child policy remains the independently captured native header/delegation snapshot. Native155 integrated certification and deeper navigation/lifecycle behavior remain separate.

### Permission query realm ownership

The Permissions instance owns its original query closure through the native interface ephemeron record. A borrowed query delegates only through that private genuine receiver record; its Promise and PermissionStatus belong to the receiver's original realm, as observed in Chrome. Wrong receivers reject with the method realm's captured Promise and TypeError. No native scalar DOM handle is added. Descriptor conversion exceptions preserve the original thrown object and are delivered through the returned Promise.

### Ancestor response admission

Ancestor checks borrow genuine parent contexts only during the synchronous document commit and retain only copied origin strings for policy evaluation. They add no native queued DOM handles or V8 context Globals. The existing replacement/retirement machinery owns the admitted or blocked document. Checks occur before the credentialless COEP exception; explicit framing restrictions apply to both normal and credentialless frames.

### Sandbox origin and module request state

CSP sandbox enforcement uses native context flags and security tokens before document script preparation. Top module origin/site state uses shared Rust Cell booleans, with no queued DOM handles or context Globals; each request snapshots their current values. Child loaders capture their native flags after admission. Script-disabled documents retain their native tree without preparing page scripts; normal retirement and ownership cleanup still applies. The remaining sandbox form/navigation/popup flags are explicitly unsupported.
