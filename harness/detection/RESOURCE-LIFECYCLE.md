# Native resource lifecycle checkpoint

The authored collectors in this directory compare actual normal Chrome155 behavior
against deterministic localhost native regressions. See [README.md](./README.md)
for commands and [FINDINGS.md](./FINDINGS.md) for current certification bounds.

## Pending task ownership

Resource preparation never stores an unwrapped tree node handle across host turns.
Its document-private plain state is an ordinary V8 edge owned by that document;
the canonical trusted mutation consumer is also document scoped. Retirement
removes the consumer and disables preparation.

| Root | Purpose | Release boundary |
| --- | --- | --- |
| Classic job execution and caller Context globals | Compile in the authentic realm and settle the caller's completion | Job completion, interruption or runtime teardown |
| Classic job original Document global | Preserve the preparation document and its private currentScript record | Job completion, interruption or runtime teardown |
| Optional script element global | Preserve the actual executing script; shadow classics expose null | Job completion, interruption or runtime teardown |
| Pending native PromiseResolver global | Settle exactly that queued script without calling JS completion helpers | Job completion, interruption or runtime teardown |
| Bootstrap script wrapper array | Admit parser scripts once without later DOM rescans | Consumed by initial hydration; cleared on retirement |
| Frame script wrapper array | Authenticate indexed frame script execution and resource event targets | Cleared after all frame scripts/resources finish, or on retirement |
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

Still open: COOP/isolation and navigation embedding compatibility, report-only and
reporting endpoints, permissions policy, full parser/document.write lifecycle,
import maps/speculation rules, async adoption/navigation races, full script stack
source parity, complete CSS cascade/color-scheme, and wider resource cancellation
and scheduling. Passing this checkpoint does not establish Google clearance or
complete browser parity.
