# Auth operation diagnostics

This opt-in Node preload measures driver method counts and inclusive elapsed time.
It never records arguments, URLs, scripts, HTML, selectors, results or error text.
It preserves synchronous values/errors and the original returned Promise identity.
The diagnostic adds wrapping and Promise callbacks, so use a separate equivalent
case after a clean acceptance run. Its output is never eligible for speed claims.
Attaching a rejection callback also changes Node's unhandled-rejection observation;
do not use this mode to validate error-event behavior. Rejections still reach the
caller through the unchanged original Promise.

```sh
node --test harness/performance/operation-profile.test.mjs
```

Set both absolute paths, then append the absolute preload path to the existing
Node preload configuration used by the owned test run. Preserve the existing
network guard and runner loader. Do not replace their `NODE_OPTIONS` flags.

```sh
export TURBO_SURF_OPERATION_PROFILE_DIR=/private/tmp/owned-auth-profile
export TURBO_SURF_OPERATION_PROFILE_SHIM=/absolute/frozen/surf/rust/playwright-shim/index.mjs
# Additional Node option: --import=/absolute/harness/performance/auth-operation-profile.mjs
```

Each process writes a unique `operations-PID.json` at normal exit. A killed process
may leave no report. Calls minus completed calls identifies operations still
pending at exit. Root means there was no instrumented parent operation; unrelated
roots can overlap. Do not sum operation or root durations as command wall time.
Whole-process CPU includes Node and native addon threads but excludes the app,
backend and Chromium processes; it is not a per-operation CPU attribution.

Start with `_collectSession`, `_refreshFromSession`, `_drainBindings`, `_redrain`,
`goto`, `click`, `fill`, and `waitForURL`. `_refreshFromSession` currently performs
multiple serial native collections plus HTML serialization. Default collection
resumes hydration; bounded collection has different event-loop semantics. A high
count or elapsed time is a profiling lead, not proof that changing it is correct.

Existing native `TURBO_SURF_JS_TRACE` plus `TURBO_SURF_JS_PROFILE=1` records Inspector
CPU samples and native layout aggregates **only during initial hydration**. It
does not currently profile subsequent login, grant or dialog actions. Layout
stages overlap inclusively. Keep these intrusive samples separate from the
uninstrumented matched 46-case acceptance comparison.

For that comparison, preserve all 46 original identities, assertions, deadlines,
fixtures, backend reset, app source, viewport and worker count. Use an optimized
immutable Surf addon, fresh contexts, both engines passing without skips/retries,
and sequential quiet runs. Record complete monotonic command wall time separately
from per-case elapsed sums, prevent host sleep with an owned process, and reject
suspended or contended runs. Report Chrome wall / Surf wall; do not infer parity
from failing, instrumented or partially paired cases.
