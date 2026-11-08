# Dedicated Worker runtime — design plan (turbo-surf#203)

Status: **not implemented**. The render tier currently exposes a construct-only
Worker shell (PR #202 wired CSP `worker-src` enforcement at `new Worker(url)`), but
no script ever executes. This document is the implementation plan the follow-up
sessions work against, not a description of shipped behaviour.

## Target (Chrome 156 contract from the issue's "Done when" list)

1. `new Worker(url)` fetches the script (normal CSP + CORS + redirect path) and
   runs it in a fresh V8 context whose global is a `DedicatedWorkerGlobalScope`:
   `self`, `postMessage`, `onmessage`/`addEventListener`, `close`, `fetch`, timers
   (`setTimeout`/`setInterval`/`queueMicrotask`), `importScripts` (classic), and
   `error`/`messageerror` events. The render runaway watchdog applies. Host-heap
   isolation holds (CLAUDE.md's render-isolate contract).
2. `postMessage` across page↔worker uses the existing `crypto_clone` /
   `message_ports` machinery for cross-realm delivery, including MessagePort,
   ReadableStream/WritableStream/TransformStream and ArrayBuffer transfer, and
   native key snapshots.
3. `new Worker(url, { type: 'module' })` loads the module graph through
   `NetModuleLoader`.
4. `blob:` worker URLs are accepted (resolved against the page's blob URL store).
5. SharedWorker / ServiceWorker: the current honest-shell behaviour stays, with
   `securitypolicyviolation` wired for `SharedWorker` construction. Full
   implementations are out of scope for #203 — see "Follow-ups" below.
6. The CSP control (`csp-directives-chrome156.json` Worker cases) keeps matching,
   plus a new detection control for worker-script execution + `postMessage` round-
   trip under `harness/detection/`. Zero new identity-battery regressions.

## Prerequisite — already landed

`turbo_dom_browser_env::install_worker_global_scope(scope, context)` ships in the
turbo-dom-browser-env crate pinned by `rust/Cargo.toml`
(`crates/turbo-dom-browser-env/src/worker_scope.rs`). It installs
`DedicatedWorkerGlobalScope` → `WorkerGlobalScope` → `EventTarget` and the
Event/MessageEvent/ErrorEvent/PromiseRejectionEvent brands onto a V8 context the
embedder created *without* the DOM `global_template_middleware`. The embedder is
responsible for creating that bare context before calling the hook.

## Architecture

### Where worker contexts live

The render tier already creates secondary V8 contexts inside the main `JsRuntime`
for child frames (see `runtime.rs` `FrameParent` plumbing). A worker is similar
but with crucial differences:

- No DOM install (use the `install_worker_global_scope` hook, not the standard
  DOM extension).
- Separate event loop / microtask queue, independently drained by the shared
  `JsRuntime` turn (same thread, cooperative scheduling via the existing
  `run_next_window_timer` engine).
- No access to the page's `document`/`window` — the context's `globalThis` is the
  `DedicatedWorkerGlobalScope` object itself.

Two viable options for the V8 realm:

- **(A) Secondary `v8::Context` in the page isolate.** Lowest overhead, reuses
  the runaway watchdog and the existing context-scoped op-state. Must take
  extreme care that structured-cloned values do not accidentally share slots
  across contexts. This matches how frames are done today.
- **(B) Dedicated `JsRuntime`/isolate per worker.** Stronger isolation; a worker
  cannot stall the page's microtask drain. Costs a V8 isolate per worker (memory
  + startup). `deno_core` supports this via `JsRuntime::new`; message passing
  would cross threads via `mpsc`.

Recommendation: **start with (A)**, with the explicit boundary that structured
clone always goes through `crypto_clone::structured_clone_for_context` and never
shares `v8::Local` handles across contexts. Promote to (B) later if a specific
page is shown to starve under worker load.

### Worker lifecycle (per Worker object)

```
new Worker(url, opts)
  ├─ CSP worker-src check (already in runtime.rs:12881)
  ├─ Fetch script via script_fetch.rs (classic) / NetModuleLoader (module)
  ├─ Create bare v8::Context (no DOM middleware)
  ├─ turbo_dom_browser_env::install_worker_global_scope(scope, context)
  ├─ Install surf-side worker bindings (see below) on that context
  ├─ Compile + run the script (or evaluate the module graph)
  └─ Register the worker in a per-page WorkerTable
worker.postMessage(x, transfer)
  └─ structured-clone from page ctx to worker ctx, queue a task that dispatches
     a MessageEvent on the worker's self
worker.terminate() / self.close()
  └─ Mark the worker as terminated; drain and drop its queued tasks; release
     the v8::Global<Context>; the GC takes the realm.
```

### Surf-side worker bindings (installed on top of the turbo-dom scope)

- `self.postMessage(msg, transfer?)` — queues a page-direction message using
  `message_ports::enqueue_from_worker`.
- `self.onmessage` / `addEventListener('message', …)` — delivered from the
  page-direction queue on the worker's event-loop turn.
- `self.close()` — mark terminated; drain queues; break out of the current
  script turn.
- `self.importScripts(...urls)` — synchronous fetch + eval for classic workers;
  throws in module workers (per Chrome).
- `self.fetch` — bind the same fetch extension already installed in the page
  context, with the worker's own URL as the referrer base and no `document`
  snapshot.
- Timers (`setTimeout`/`setInterval`/`queueMicrotask`) — reuse
  `window_timer_nesting` but keyed by worker context, not window context.
- Error surfacing — top-level exceptions and unhandled rejections become
  `error` / `messageerror` events on the page-side Worker object (bubble to
  `window.onerror` if unhandled in Chrome's model).

### Page-side bindings (replacing the current wrapper in runtime.rs:12881)

`WorkerWrapper` grows real behaviour:

- `worker.postMessage(msg, transfer?)` → structured-clone into the worker ctx,
  queue a task that dispatches `MessageEvent` on worker `self`.
- `worker.terminate()` → signal termination to the WorkerTable entry.
- `worker.onmessage`/`onerror`/`onmessageerror` wired as real event accessors on
  the existing `EventTarget` brand (do not synthesize a fake EventTarget; the
  shell today already brands correctly).

### Structured clone across contexts

`crypto_clone` already supports cross-realm cloning for child frames and
MessagePort transfer; adapt the entry point so worker↔page follows the same
path. Transferables (MessagePort, ReadableStream, ArrayBuffer) use the existing
`message_ports`/`stream_transfers` machinery. Native key objects must snapshot
through `crypto_clone::native_key_snapshot` so the worker gets its own
`CryptoKey` backing without reaching into the page's crypto backend state.

## Milestones (one PR each, each session-sized)

1. **M1 — classic worker script execution + postMessage round-trip.**
   Un-ignores `classic_worker_script_runs_and_round_trips_a_postmessage` and
   `worker_global_scope_has_no_dom_but_has_timers_and_fetch` in
   `tests/dedicated_worker_runtime.rs`. Wires fetch (classic), the bare context,
   the turbo-dom hook, `self.postMessage` / `worker.postMessage`, `onmessage`,
   top-level `error` surfacing, `terminate`/`close`. This is the MVP the issue
   calls out as item (1).
2. **M2 — structured clone + transferables parity.** Covers MessagePort
   transfer across workers, ArrayBuffer transfer, ReadableStream transfer, and
   native key snapshots. Issue items (2) and the cross-worker edges of
   `message_port_queues.rs`.
3. **M3 — module workers.** Un-ignores
   `module_worker_loads_its_import_graph_through_the_net_module_loader`. Loads
   the entry + its graph through `NetModuleLoader`. Issue item (3).
4. **M4 — `blob:` worker URLs.** Coordinates with the `script_fetch` agent
   adding `blob:` script loading. Issue item (4).
5. **M5 — `importScripts` + worker-side `fetch`.** Classic-worker
   `importScripts(url, …)` synchronous fetch+eval; `fetch` bound inside the
   worker context with the right referrer and origin.
6. **M6 — SharedWorker / ServiceWorker decision.** Issue item (5). At minimum,
   extend the PR #202 CSP construction check to `SharedWorker`. Keep the honest
   shells; add entries to the hydration `unsupported` diagnostics listing.
7. **M7 — detection control.** Add the worker-script-execution +
   `postMessage` fixture under `harness/detection/`, extend
   `csp-directives-chrome156.json` where applicable. Issue item (6).

Each milestone owns its own PR against `release/0.8.1`.

## Open questions

- **Event-loop ordering** between the page microtask queue and a worker's
  microtask queue when both run on the same thread. Chrome's model is that each
  agent has its own microtask checkpoint; option (A) above has to emulate that
  without running one agent's microtasks inside the other's. The current
  `run_next_window_timer` engine is per-window; the worker needs the same
  shape, keyed by worker context.
- **Termination safety** under in-flight structured clone (worker terminated
  mid-`postMessage`). The MessagePort machinery already handles close during
  transfer; audit whether the same holds for worker-side ports.
- **Runaway watchdog** granularity: today a runaway kills the whole isolate.
  For option (A) workers that is acceptable (the worker's runaway equally kills
  the page, matching the shared-thread choice). For option (B) the watchdog is
  per-isolate automatically.
