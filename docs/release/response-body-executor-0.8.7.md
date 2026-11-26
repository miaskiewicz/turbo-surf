# Response-body executor ownership

Background response-body pumps now run on the existing HTTP transport runtime.
The NAPI actor uses a current-thread runtime: synchronous V8 work can prevent a
body pump spawned there from running, even after a peer has sent the bytes.
Headers already used the independent HTTP runtime.

The real localhost TCP regression parks the caller runtime after headers, gates
the peer body, and requires native completion before polling the caller again.
The baseline failed; binary delivery, typed body-limit failure and owner
cancellation now pass. Byte ordering, cancellation watches and completion
ordering remain unchanged. Executor initialization failure is a typed network
error.

Validation on the integrated consumer graph: production Core 356 passed, zero
failed, one existing ignored; rustls Core 347 passed, zero failed; seven render
fetch/body/input binaries 45 passed, zero failed. Strict all-target Core Clippy,
format and the unchanged complexity limit passed. Private raw receipts are in
`.recovery/response-body-actor-stall`.

This is a demonstrated executor bug, not a claim that the application timeout is
fixed. The optimized body-only artifact passed smoke 2/2 and images 18/18, but
the unchanged Flux Super two-company test still exceeded its original 120-second
limit. The later DOM validation/body artifact did too. Auth46 and the wider suite
remain blocked while synchronous timer/geometry/presentation cost is investigated.
