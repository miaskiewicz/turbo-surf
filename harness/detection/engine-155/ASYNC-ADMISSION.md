# Experimental async-op admission recovery

A watchdog can terminate JS after Deno's native operation has been admitted but
before `00_infra.js` registers its promise. Resuming the isolate then throws
`Missing promise @ <id>` when the native completion is delivered. This is an SDK
bookkeeping failure, distinct from a rejected page promise or a Google verdict.

`async-admission-probe` deterministically requests isolate termination during a
native asynchronous operation's initial poll, then clears termination and resumes
the event loop. The exact same probe reproduces the missing-promise error on the
pinned preimage and completes on the patched source. It additionally checks
pending/eager/synchronous-throw results, all ten generated argument arities,
argument forwarding, reservation visibility during admission, reservation removal
on settlement, and absence of unhandled rejections in these cases.

## Application and controls

`async-admission-sources.json` pins both Deno source preimages and postimages.
`apply_async_admission.py SDK` accepts only an explicitly managed SDK with
`cargo-config.toml` and Chrome155/Deno0.412.0 `provenance.json`; it verifies every hash, checks/applies the
patch, and reverse-verifies the result. It refuses mixed or unknown source.
`--reverse` restores the exact preimage for independent failure controls. It never
edits the Cargo registry cache. Run after preparing the experimental155 SDK and
before its Rust compilation. Native C++/archive/bindings do not change.

The patch updates `00_infra.js` and its `rebuild_async_stubs.js` template together.
All ten stubs reserve a unique ID and register a promise before calling the native
operation. Eager and synchronous-throw paths settle the unused reservation without
rejecting it, then retain the existing returned PromiseResolve/PromiseReject path.
Pending operations return their registered promise. The missing-promise check is
unchanged: duplicate/unknown completions still fail rather than disappearing.

The normal `engine-155/build.py` entry point now applies/verifies the patch before
shared SDK compilation and records its patch hash and exact source pre/postimages
in `provenance.json`; successful shared builds carry that record into
`validated.json`. Preparing a new SDK retains its original source until this
build-time verification step.

Run all controls with the managed SDK's hash-verified archive, bindings and Cargo
source config (no absolute machine paths or additional network requests):

```sh
python3 harness/detection/engine-155/run_async_admission.py .native-builds/sdk155-8059.79 --mode all
```

`--mode preimage` reverses to the exact preimage and requires the Missing promise
failure. `--mode recovery` applies the patch and tests successful recovery/all
arities. `--mode rejection` applies the patch and characterizes caught, genuinely
unhandled and interrupted native rejections. `all` runs those modes in order and
leaves the managed SDK patched. The runner verifies archive/binding hashes against
`native-cache/artifact.json`, uses that SDK's target directory and offline locked
Cargo resolution, and defaults to Rust1.98.0 (`--toolchain` can select another
installed toolchain). Never run against an SDK target leased by another compiler.

## Bounds and remaining evidence

This is an experimental source patch, not a published SDK or a blanket promise
recovery policy. IDs now advance for eager/throw calls as well as pending calls;
those IDs are internal bookkeeping. The unused eager/throw reservation adds a
resolved internal catch microtask while preserving the public returned promise.
Termination before native admission can leave a registered reservation with no
native completion; this patch does not reclaim that reservation. Termination
inside promise allocation itself also remains outside the proven admission
window. Such reservations persist until isolate teardown and are not claimed
bounded across arbitrarily many interrupted executions. An interrupted native rejection now reaches the event loop as its genuine error,
not Missing promise. As with a genuinely unhandled normal native rejection, it
remains an unhandled error because termination prevented the caller from attaching
a handler. Normal caught rejection completes cleanly. The patch does not suppress
these errors or promise a guest-side continuation that was itself terminated.
Full SDK/application regressions and live capture after rebuild remain required.

2026-10-16 actual V8 `15.5.35.23-rusty`, native archive SHA256
`1866b4e3ee649fb1209d0dfb6d7a2dac7f5b8a2033adfdd0e112a7c675de7a9f`:
unpatched exact probe reproduces Missing promise; patched probe passes the stated
recovery and30 all-arity outcome controls. Rejection mode additionally confirms
normal caught rejection is clean while normal unhandled and interrupted rejection
retain their genuine error messages and never become Missing promise. Existing managed Deno/Tokio deprecation
warnings are preserved in raw logs, not suppressed.
