# Promise work and MessageChannel tasks

`measurePromiseWork(page, { depth, fanout, turns })` runs on a Playwright-compatible
Page. Each MessageChannel callback queues the next message before awaiting a
chain and fanout of reactions to a genuinely fulfilled parent Promise. It checks
that the next message does not reenter before those reactions finish.

The result contains the completed task count, exact checksum, requested workload,
and elapsed host time. `expectedPromiseWork(options)` provides the expected count
and checksum. Elapsed time includes the facade and engine; it is contextual
measurement, not a pass threshold. The requested counts do not claim to count all
native Promise hooks or allocations.

Run the offline functional workloads with a built addon:

```sh
TURBO_SURF_NAPI_PATH=/absolute/path/to/turbo-surf.node \
  node --test rust/playwright-shim/promise-work-perf.test.mjs
```

The file is also registered in `npm run test:shim`. Missing addon artifacts produce
explicit registered skips; other import errors fail. The Rust
`terminal_cache_tests` separately count actual native Promise-state observations
and verify pending transitions, fulfilled values and rejected state. Existing
parent-collection and held-peer tests guard ownership semantics.
