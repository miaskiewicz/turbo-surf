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

## Matched original 46-case reports

`node harness/performance/compare-auth.mjs /absolute/path/manifest.json` reads
real nested Surf TAP and Playwright JSON. The manifest declares `schemaVersion: 1`,
`expectedCases` (the 46 original inventory records with `file`, `title_path`, and
`project`), and `pairs: [{ surf, chrome }]`. Each engine record contains:

- `artifact: { path, sha256 }`, relative to the manifest; Surf TAP or Chrome JSON.
- `exitCode`, `signal`, `wallMs`, `startedAt`, `endedAt`, `workers: 1`.
- `instrumented: false`, `suspensionDetected: false`, `contended: false`.
- `clockGapMs`, `clockGapLimitMs`: measured largest wall/monotonic discrepancy and
  its predeclared allowable measurement gap. The limit concerns clock integrity,
  never a permitted speed deficit. Whole-invocation timestamps must agree too.
- `cachePolicy`, `coldWarmPolicy`, `fixtureReset`, `setupNotes`, `provenance`:
  nonempty recorded notes/pinned receipt references; first three match per pair.
- Surf additionally requires `optimized: true`.

Keep raw source/addon/app/assertion/compiler/browser receipts alongside the
manifest. This analyzer verifies declared evidence consistency and hashes; it
cannot authenticate operator-supplied notes. Distinct report paths are required
for repetitions. Invocations must be sequential. Failures, skips, TODOs, retries,
unexpected/missing/duplicate identities, overlapping runs and reported sleep or
contention reject the comparison. Output gives complete-command paired median
`Chrome / Surf`, ranges, and per-case elapsed medians. Zero-resolution cases keep
a null ratio. There is deliberately no invented parity tolerance or pass flag.

```sh
node --test harness/performance/auth-comparison.test.mjs
```

## Separate grant diagnostic recipe

After the clean 46-case gate passes, the owned payroll runner can execute the
original `delegatedWriteStep.spec.ts` flux-super-admin grant case separately.
Create a diagnostic config in the fresh owned output directory, importing the
unchanged absolute `playwright.surf-validation.config.ts`, with `testDir` set to
the application's absolute `e2e` path, and override both root and project `grep`
with `/flux super admin can request, hold, and end a write grant/`. Keep the
original timeouts, fixtures, setup, teardown and assertions. Use the same frozen
source, addon, app, backend, approved auth origin and disposable accounts.

Append the profiling preload to **the runner's final** `NODE_OPTIONS`, after its
tsx loader and network allowlist. An outer `NODE_OPTIONS` export is insufficient
when that runner overwrites it. Set the two absolute profile environment paths
above and invoke this argv (substitute the pinned absolute paths):

```text
node --import tsx /FROZEN_SURF/rust/playwright-shim/bin/turbo-surf-test.mjs
  --config /OWNED_OUTPUT/grant-profile.config.ts --reporter tap
  /PAYROLL/e2e/src/auth-guards/delegatedWriteStep.spec.ts
```

Do not add an unsupported CLI `--grep`; filtering belongs in the diagnostic
config. Retain the runner's global lock, scoped environment whitelist, approved
origin guard, owned sleep inhibitor, package-symlink restoration, PID cleanup
and source/addon/spec/config integrity checks. Mark the resulting receipt
`diagnosticOnly: true`, `instrumented: true`, `acceptanceEligible: false`; filtered
nonselected cases do not constitute a 46-case result. Never put this diagnostic
TAP into the matched comparison manifest.
