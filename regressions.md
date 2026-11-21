# Release regressions

## Current v0.8.7 payroll acceptance — native async ownership blocker

On 2026-11-21, a fresh isolated default-feature release NAPI build of Surf
`d1dd8878b7e19ea32c8b789f94fb77ac2522374a` with local unpublished turbo-dom
`d00712fc01c9cb1a2a81ed11df2cd338f76d3541` fails the unchanged Flux super admin
`delegatedWriteStep.spec.ts` case with `Native async work has unclassified ownership`.
The signed artifact SHA256 is
`9a6e75d6d5251ce198289a8e43dd833597957c10c9f2e4ac33538dc5c483b83c`.
Actual compiled inputs and feature fingerprints are in Root
`.recovery/payroll-v087-20261121/ARTIFACT.json`; all six Surf crates compiled fresh.
The development DOM patch remains in use; this is not a registry release proof.

The case passed real PropelAuth login, admin-shell navigation, read access, and
the exact read-banner geometry assertions (height 44), then failed during the
grant portion after 64.184 seconds. It did not reach the write-banner assertions.
Result: 0 passed, 1 failed, 0 skipped, 0 canceled. The native failure-capture
fixture also observed the authenticated admin shell. The first run did not identify which native guard rejected the action. The
separately labeled diagnostic rerun below now confirms the same-root collected
parent branch. Surf owns the repair; no producer defect is established.
The independently reproduced collected-root-parent defect is fixed in `728fd61`;
application causality is confirmed by the diagnostic run below. Restored-green
application validation remains pending. The offline control and gate results
below do not establish an auth pass.

The service owner explicitly reserved ports 3000/3001 and paused edits/tests/writes
for this single case. Frontend: the audit worktree at `7ad381b96` plus pending #772
changes; backend: disposable flux-apis `b822541ab8f1ddfcc874096bcbf15b73a7c8e882`.
The run consumed that worktree's freshly restored fixtures, with global setup,
teardown, and webServer disabled; fixture JSON remained byte-identical. The owner
then ran the exact Chromium case successfully (1 passed, 8.1-second case), including
baseline revoke, grant, end-session, and afterEach cleanup. The service window is
returned; further native service use requires another coordinated window.

Current main-checkout auth inventory is 46 cases in 15 files. Do not combine that
inventory with the single owner-worktree execution as if the whole suite ran.
Fresh-artifact offline controls pass 34/34 (14 Promise/load-boundary controls and
20 deadline/hydration controls), with zero skips/cancellations. Preserve the
separate initial sandbox-listener EPERM attempt as an environment failure.
Evidence: Root `.recovery/payroll-v087-20261121/owner-window/{PREP,RUN,RESULT}.json`,
`run.log`, `artifacts/**/failure.json`, `offline-controls-localhost.log`, and
`deadline-controls.log`; owner Chromium log `/tmp/codex-e2e-audit-restored-smoke.log`.
No deadline, assertion, coverage floor, security policy, or exclusion was relaxed.

An independent deterministic offline candidate now reproduces the same error
without the application: retain a reaction child of an abandoned pending Promise,
prove its parent was actually collected using the existing native GC barrier,
then dispatch a real button click that opens a DOM modal. The click runs exactly
once and opens the modal, but the action returns the ownership error. Awaiting
the unresolved child still rejects; its reaction never runs. Retaining the live
parent makes the identical action pass. Baseline: 1 passed, 1 failed, 0 ignored
(Cargo exit 101), in 0.48 seconds after compilation. Test-only source is
`.worktrees/payroll-v087-pending-parent-repro/rust/crates/turbo-surf-render/tests/pending_parent_action.rs`;
receipt `.recovery/payroll-v087-20261121/pending-parent-repro.log`.
This establishes an offline Surf defect, not yet application causality. A separate
diagnostic-only NAPI build labels the original rejection site and numeric guard
conditions, without changing selection or admission. Live use remains subject to
a new explicit service-owner window. Production repair `728fd612` is locally integrated into `release/v0.8.7`, with
fixture correction `9feee266` and evidence commit `69132a7b`. The development DOM
patch is retained and must be dropped/replaced with the published dependency
graph before any release tag; no tag or publication is part of this work.

The candidate permits only the existing same-authenticated-root collected-parent
classification for generic action/timer selection, preserving unresolved explicit
completion, foreign/unknown parent, unowned operation, and inconsistent-owner
rejections. The unchanged original modal regression passes; restoring the exact
original production file reproduces its error (3 passed / 1 failed across four
integration controls), and restoring the candidate byte-for-byte passes 13/13
related action, Promise-parent, and native lifecycle tests. Four additional native
guard controls pass, including a genuine unowned operation and collected unknown
parent. The deliberately contradictory-registration test uses real native Promise
and document identities as an invariant test. A new guard fixture initially failed
to enter its destination context; its corrected setup and rerun are separately
logged, with no production-policy change. Affected strict all-target Render Clippy,
workspace formatting and workspace CC15 pass. The complete restored workspace
gate passes 2,268 tests, zero failures, seven existing ignores (one live TLS echo
and six documentation examples), zero filtered/canceled cases. Workspace
all-target Clippy with denied warnings also passes. No application pass is claimed. The standalone
pre-existing unicode-range assertion formatting issue is corrected with rustfmt.
Receipts: Root `.recovery/payroll-v087-20261121/pending-parent-{candidate,guards-restored,inverse,related-restored}.log`,
`inverse-source.json`, `CANDIDATE-SOURCE.json`, and `candidate-{fmt-restored,clippy,complexity-restored}.log`.

### Confirmed application guard branch and full shim validation

A second explicitly reserved two-minute window ran the exact diagnostic case
with original production selection logic plus numeric error labels. It passed
login, admin shell, read permission and exact read-banner geometry, then failed
at 69.512 seconds with:
`selection inconsistent=false unknown_ops=0 unknown_parents=4 collected_root_parents=4 strict=true`.
This confirms the application hits the same collected, authenticated root-parent
state as the deterministic modal regression. It is not an unknown operation or
inconsistent owner. All 5,157 captured frontend/E2E source files and the shared
fixture file remained unchanged during this run. The window was immediately
returned; another live run requires a new explicit owner READY.

The fixed signed artifact from clean commit `69132a7b` has SHA256
`186a83cf4d855d8cf408bc48bd635fdf627fb8ee3aa9e332006bcbc34cdaf05e`.
Its full standard shim suite passes 873 tests, zero failures/cancellations and
one intentional skip that tests skipped bodies do not execute. Node's actual
loaded-native report contains only this selected addon. The branch's package
version remains 0.8.6 despite the v0.8.7 branch name. Exact native source, features,
compiled package paths and library hash are recorded in Root
`.recovery/payroll-v087-20261121/FIXED-ARTIFACT.json`; `fixed-shim.log` is the full
exit-zero receipt. The archive identity and unchanged 2,075-file producer source
comparison are recorded separately. This remains a local unpublished DOM graph.

The current service-owner auth inventory is 46 tests in 15 files. One unique
case has failed on the original and diagnostic artifacts; 45 cases remain unrun.
The fixed artifact rerun below reaches a later modal wait failure. No full auth
or broader payroll acceptance pass is claimed. Owner coordination, source/fixture hashes,
exact command, counts and diagnostic guard data are preserved in
`.recovery/payroll-v087-20261121/diagnostic-owner-window/`.

### Fixed artifact follow-on: request-write modal does not appear

A fresh explicitly reserved service window ran the exact Flux super admin case
against fixed artifact `186a83cf...cdaf05e`. Login, admin navigation, read access,
and read-banner geometry passed. The request-write click returned without the
old ownership error, then the existing 30-second visible wait for
`delegated-write-modal-note` timed out. The whole case took 99.970 seconds; this
was a locator timeout, not the 120-second case deadline. Result: 0 passed,
1 failed, zero skips/cancellations. No note fill or grant-create submit stage was
reached; the original baseline revoke and afterEach remained enabled.

Page error and console error captures are empty. All 5,157 captured served source
files and the fixture JSON stayed unchanged during this window. The owner
confirms API image `flux-apis:codex-e2e-audit`, commit `6e6e54e4b` (cleanup PR #1254),
with no restart during the run. The window was immediately returned; shared
service usage is again explicitly held for #772. The owner then ran the exact Chromium case on the same backend commit:
1 passed, zero skipped, 7.8-second case (8.3 seconds total), including read/write
banners, grant end, and afterEach cleanup. The earlier different-backend control
is not substituted for this new one.

The new failure's root cause and ownership remain unproven. Basic offline
React/MUI Button+Dialog controls pass. A separate plain-DOM probe shows a
potential frozen-target mechanism: replacing a child during focus/mousedown can
leave later shim events on that detached child. A separately identified Chrome 159.0.8154.6 oracle (the pinned 156 binary was
unavailable) also suppresses the final button click in both replacement modes;
the unchanged-child contrast opens normally in both engines. Thus this does not
explain the application modal failure and no production retargeting change is
justified. Chrome does send pointerup/mouseup to the replacement node while the
shim dispatches on its detached prior target; that independent event-path
difference remains tracked, unintegrated, and without application-causal proof.
The oracle used the required wrapper with an explicit existing-binary override,
localhost-only flags and no download. Fix SHA for the modal failure: none.

Regression subset remains the exact original delegated-write case, with the
narrow offline controls under Root `.recovery/modal-repro/`. Full run/error,
source/fixture identities and result receipt are under
`.recovery/payroll-v087-20261121/fixed-owner-window/`. The same one unique auth case
remains failed; 45 auth cases are unrun. A bounded diagnostic observer is prepared for a future coordinated run: passive
event-path/connectedness records, synchronous no-drain DOM reads, and manually
selected React boolean/lane metadata only. Its offline fixture proves exactly
one real handler, matching live/cached modal state, false-to-true boolean state,
and omission of a sensitive sentinel. It does not replace handlers or reset
React state; diagnostic perturbation is explicitly labeled. No live observer run
is claimed while services are on HOLD. No deadline, assertion, fixture premise,
error policy, or exclusion has been relaxed.

## v0.8.7 unicode-range workspace test compilation

The first full workspace gate on the candidate stops before test execution with
Rust E0063 at `turbo-surf-mcp/src/lib.rs:7436`: the existing
`fetch_font_face_skips_unresolvable_and_refused_urls` test omits the newly added
`WebFontSource.unicode_range` field from release commit `d302301a`. The fixture
now explicitly uses an empty range vector, the documented no-descriptor default;
its existing failure-path assertion is unchanged. The independent existing
Render unicode-range assertion is rustfmt-normalized. No runtime logic, coverage
floor, or test scope changes. Corrected in `9feee26`; the restored full workspace gate passes as recorded above. Receipts:
Root `.recovery/payroll-v087-20261121/candidate-workspace-test{,-restored}.log`.

Offline regression subset: `cd rust && cargo test -p turbo-surf-render --test pending_parent_action`
and `cargo test -p turbo-surf-render --lib selection_guard_tests` (four controls
each). The exact baseline production inverse preserves all assertions and fails
the original modal control; the candidate restoration is byte-identical.

## Historical v0.8.5 work

Target: `release/v0.8.5`. Updated 2026-11-19.

Original batch: **34 test cases (11 Rust, 23 shim)**. **All 34 have component proof and their fixes are merged.** This is not a fresh combined-suite result. Existing intentional ignores/skips are unchanged.

All additional bugs found during this work are explicitly in scope and tracked below until fixed and validated. No deferrals. Payroll/backend/frontend tests and service changes remain paused until a new express go-ahead. Required supporting dependency publication was explicitly authorized and completed; Surf publication is authorized only after the final release gates and artifact proofs pass.

## Remaining input and content execution budgets — component validation

Actual localhost tests expose three remaining whole-operation watchdogs: public input after a 140 ms wait, root content replacement after a real 140 ms external-script response, and targeted content replacement after the same response. With the unchanged 200 ms internal budget and bounded 100 ms callback, all three enter once but are interrupted with `active=true` and `exited=0`. The other failures in the first eight-case snapshot concern hard-error wording, not additional causal evidence; explicit caller controls now retain their pre-existing exact timeout text.

The fix applies the existing cooperative internal execution guard to input and both content entry points. It preserves the actual selected input document for cancellation, joins execution/caller workers before clearing termination, and always performs history cleanup. Content caller deadlines remain absolute and hard: actual caller expiry retains the requested `setContent timeout` error, while an earlier internal stop reports its own budget/cause. Infinite callbacks retain the stable hard-interruption error. Public input returns that fatal error before following a queued form.

Focused ON controls currently pass **18 / 0**: twelve input/content finite, infinite and caller-cause cases, plus six public form/error-precedence cases. Restoring only the former whole internal watchdog reproduces all three dirty finite-callback tails. Removing only the public input fatal-return branch starts one real form HTTP request and masks the fatal error with a later navigation timeout. Both inverses retain the original budgets and genuine callbacks/transport; neither fabricates document authority or state failure. The first restored adjacency run also exposed a nested-hard-error classification race: the selected target guard could return the stable fatal error while the outer guard observed only aggregate expiry. The shared guard now retains that already-returned fatal cause; ordinary errors and successful results are unchanged. The failed **68 pass / 1 fail** receipt remains preserved. With temporary post-join diagnostics removed, exact final restoration passes **70 / 0** across the new controls and existing input, content, caller, history, module, initialization and retirement controls. Affected Render/NAPI all-target Clippy with denied warnings, workspace formatting, CC15 and whitespace checks all pass; no budgets, floors or assertion contracts were relaxed. These local component results do not claim a new combined Linux or full-workspace pass.

## v0.8.5 packaging-only hotfix

- [x] Reproduce the published npm v0.8.4 Unix CLI failure with a normal installation: the native executable is mode `0644` and launching fails with `EACCES`.
- [x] Identify the masking proof: the old extracted-tarball smoke added execute permission before launching. Preserve the broken published archive and installed failure evidence.
- [x] Fix staging before packing (`31f9ecd4`): set all three Unix executables to `0755`, verify actual tar-member permissions, and remove the extracted-consumer permission repair. Windows staging is unchanged.
- [x] Validate actual TDD: seven baseline failures become twelve passing packaging controls; all 27 supervisor controls pass. A fresh untouched candidate installation passes CLI, NAPI, facade, native runtime and signature checks. Its payload bytes equal the published v0.8.4 bytes; only the native executable mode changes.
- [ ] Complete v0.8.5 CI and exact release-artifact checks, including untouched Unix executable consumers.
- [ ] Publish and verify the actual v0.8.5 registry packages, then deprecate npm v0.8.4 with an upgrade notice.

The v0.8.4 engine gates completed successfully: local Rust **2,185 passed / 0 failed / 7 intentional ignores**, JS **1,172 passed / 0 failed / 3 intentional skips**, current PR/push/main CI green, published Rust and Python consumers verified. The post-publication npm permission failure is the sole hotfix scope. Engine implementation, turbo-dom 0.8.4, Deno and V8 dependencies remain unchanged. The older pending-gate snapshots below are historical evidence, not additional unresolved v0.8.5 work.

## Independent collector correction — validated and integrated

- [x] Prove that an admitted native root navigation with response headers held can disappear from the guest navigation field while the collector incorrectly reports the fixed observation complete. The deterministic real-PageSession control observes the actual HTTP request and native target's `pendingNavigation: true`; the original collector then fails the expected incomplete-result assertion.
- [x] Correct only `turbo-surf-mcp/examples/detection_trace.rs`: record the native root's host-owned network state after the final snapshot and retain a native-root pending marker when no guest destination remains. Existing guest pending destinations are preserved; no URL is invented and native identity errors propagate. Engine behavior, observation windows, execution budgets and assertions are unchanged.
- [x] Validate baseline RED → all nine example controls GREEN → recording-call-only inverse RED → restored nine GREEN, with target Clippy, formatting and complexity checks passing. Evidence: `.worktrees/detection-pending-root-navigation/.recovery/held-navigation/FINAL.json` (SHA256 `5e4a30828f0a023abc97a7e764f3985b675856c1e7e706845fadc8819ea1cc6d`).
- [x] Integrate the reviewed collector correction into `release/v0.8.5` after express user approval. This adds only collector reporting and its deterministic regression to the packaging repair; engine implementation and dependency sources remain unchanged. Whether it caused the repeated CI `navigation-observation.test.mjs:186` failures remains unproven; interruption between recording `timerFiredDate` and assigning `location.href` is a separate alternative.

## v0.8.4 integrated repair batch — historical pre-gate snapshot

The current batch adds the actual actor-opening readiness barrier, final-refresh close guard and initial-navigation network-idle wait. Genuine native controls reproduce each defect before its fix; the registered opening/close controls and existing network-idle controls pass **19 / 0**, with the original tests and timeout limits retained. These component results do not replace the next complete JS run.

Timer-owner recovery now preserves the actual caught termination exception and uses typed interruption provenance without clearing genuine authority failures. The matched native baseline fails, the fix passes **12** resource/timer/private-owner/quiescence controls, a caller-only inverse fails while preserving the catch/rethrow, and the exact restored source passes all **12** again. Clippy, formatting and complexity checks pass. The separate background API and matching-task fixture repairs below also have component proof.

The ongoing older frozen `d813` full Rust run has observed failures in `coverage_background_recovery` and `target_network_state::background_turn_waits_for_real_timer_and_can_be_interrupted`. It continues without fail-fast. The latter single-poll assertion is being checked on the repaired background API; its cause is not inferred from the shared symptom. Old `d813` package, Python and binary artifacts retain their recorded source identity and must not be relabelled as artifacts for new native source.

## Realtime target wait — matching-task fixture contract

The frozen `d813` full Rust run also fails `target_network_state::background_turn_waits_for_real_timer_and_can_be_interrupted` at line 270: one background poll completes inside the 20 ms observation while the test expects a timeout. The unchanged original source (`5534e072`) reproduces that assertion failure on the accepted ready-wake API fix (`e39524ee`), so the independent wake correction alone does not establish this fixture's expectation.

A genuine zero-delay benign task alongside the requested 100 ms timer isolates the bad premise: the actual benign counter is **1** and the requested `done` flag is still **false**, then the old one-poll timeout expectation fails. This is valid competing-work RED (`375bd3bd`), not an early target timer or a changed timeout. The corrected local collector completes only when the actual target flag is true, across real background turns, under the same **20 ms initial total bound** and **2 s subsequent total bound**, with the target delay still **100 ms**. Native errors propagate; no sleep, clock override, production API change or broader quiescence pump is added.

The unchanged original and controlled baseline fail, then the corrected control (`b04f3785`) and its entire target pass **9 / 0 / 0 ignored**. Private-target source, signed-binary and raw result receipts are under `.worktrees/background-timer-wake-recovery/.recovery/background-target-wait/{original,competing-baseline,on}`. Focused target Clippy with denied warnings, formatting and CC15 all pass; full combined gates remain required. The exact completed task in the historical frozen run was not recorded, so no matching historical task identity is invented.

## Background watchdog recovery — matching-task fixture contract

The background API completes one actual task, which need not be the timer a caller just scheduled. A genuine competing-task control registers the infinite zero-delay task first and a benign 100 ms task second, then keeps the original **500 ms** watchdog operation. After interruption, the benign task is already older than the newly scheduled recovery timer. Under the old single-poll assertion, the actual watchdog error, reached DOM, root identity and benign counter **1** all pass, then the recovered-DOM expectation fails: **0 / 1**. This proves the single-turn assumption is invalid without fabricating clock, ownership or queue state.

The corrected fixture collects real background turns until the actual recovery marker within the same **single 3 s total bound**, propagating errors and checking root identity and unchanged reached state on every intervening turn. It retains the original final DOM assertion and verifies the benign task ran exactly once. The same reordered competing-task control passes **1 / 0**. The earlier candidate that registered the benign timer first could fail before entering the intended watchdog task under load; that receipt is preserved as an **invalid precondition**, not causal RED or a fix failure. Accepted matched source pins are baseline `84c4c2e4` and corrected `c5c1380b`, with receipts under `.worktrees/background-timer-wake-recovery/.recovery/background-wake/competing-reordered-{baseline,on}`.

This is separate from the independently proven ready-wake API bug below. The historical `d813` full-run failure has the same unmet recovery milestone, but its exact completed task was not recorded; neither component claims that missing branch witness. Both component scopes pass focused strict checks; combined full gates remain required.

## Background timer wake versus task completion — independent API defect

`BackgroundTurn` previously converted an already-ready timer sleep into successful task completion even though no callback had run. The shared parking helper instead documents that it wakes the caller for another poll to select the due task. A deterministic unit contract uses the same genuinely completed Tokio `Sleep` and its deadline, then a counting waker: the wake prerequisite passes, and baseline fails only because it returns `Some(Ready(Ok(())))` instead of remaining pending. No paused clock, synthetic owner or guest bridge is used.

The narrow fix leaves the caller pending after that ready wake; the helper already requests the next poll. Baseline **0 / 1** becomes **1 / 0**, and the original unchanged real-PageSession recovery control passes with its original **500 ms** operation budget and **3 s** outer bound. Adjacent form-admission and realtime timer controls pass **17 / 0**. Focused Render library/recovery-target and NAPI library Clippy checks with denied warnings, formatting and CC15 all pass; the combined all-target sweep remains a final-gate obligation. Private-target receipts are under `.worktrees/background-timer-wake-recovery/.recovery/background-wake/{baseline,on,competing-on}`. This independent production defect does not prove which task completed in the historical `d813` full-run recovery failure; the separate fixture contract is tracked when its correction is integrated.

## Earlier integrated repair snapshot — 2026-11-19

Canonical local source `203bf5da8b1251584f3f070fe1108c392a3903e9` contains the following reviewed repairs. They are **FIX READY / WAIT NEW COMBINED LINUX AND LOCAL FULL GATES**, not a publish-ready or new combined-suite result. Diagnostic cleanup and final source freeze are still separate pending steps.

| Repair | Actual causal/affected proof | Current obligation |
|---|---|---|
| Resource-owner recovery after interrupted input (`host_input` case at line 902), adopted as `203bf5da` | Matched causal RED → ON **32 passes**; inverse returns RED, restored fix passes **three** controls; affected strict checks zero. The original Linux `9de` target was **25 pass / 1 fail**, with termination true before and after owner-bridge restoration. | New integrated Linux confirmation is pending. The earlier 507 baseline stopped at its **5 s precondition** because the guard did not fire: it never attempted public recovery and is **not** a stale-stop causal RED or production-fix proof. No repeat or unrelated cause is inferred from it. |
| Quiescence owned-watchdog classification, adopted as `eb1958f4` | Deterministic selected-predicate RED → ON **10 passes**, with strict checks zero; full details and unchanged 200 ms contract are preserved below. | Integrated locally; new combined Linux confirmation remains pending. The older owner-admission wording without its own diagnostic remains historically unproven. |
| Host-bindings collector timer fixture, adopted as `a59ef437` | Matched RED → ON **seven** host-binding controls, including inverse RED and restored ON; strict checks zero. The fixture waits for its own guest timer's actual completion, rather than assuming one globally due turn must run it; an earlier rendering timer can consume that turn. | This is a fixture-protocol correction, not a production repair or waiver of the no-rehydration assertion. Validate it in the next full graph; the old failed full run remains failed. |
| Complete shim registration, adopted as `049911ab` | Inventory is **194 official files + one local control = 195 files**, unchanged **43 product files**, with **14 checker controls**. Registration and completeness metadata are corrected. | Require every registered file in the final full run. Inventory/metadata changes receive no causal runtime-test credit for `9de5b4bc` or another product fix. |

Historical local full Rust `93806` remains **2,177 pass / 1 fail / 7 ignored**, 2,185 registered, with the collector fixture failure; its source and receipts are immutable. Local full JS `3271` remains failed with **nine shim failures**, E2E unrun and detection **278 passes**. Later unchanged timer/observer checks and launcher controls are **finite non-reproductions only**; the historical timer and launcher-eight obligations remain for the final combined comparison, not claimed fixed by an unrelated cache repair. Full workspace Clippy/formatting `5520` passed on its recorded earlier `afa34f5f` source, not retroactively on this new graph. No old failed receipt is relabelled green. Final controllers must bind the frozen final HEAD and derive actual new test totals rather than reuse 2,185.

## Metadata refresh reentrancy — component validation

A genuine public-API control found a separate Surf cache-order defect: an authored, delegating `Element.prototype.getAttribute` override mutates the root heading when normal iframe metadata collection reads `name`. The collector first dirties the real DOM; its immediately captured `page.content()` still contains the earlier heading, while a subsequent public DOM read proves the metadata method ran once and the live heading changed. Original source `aa54cf87`, control `9b534f07`, unchanged native addon `5c11d5e`: baseline **0 pass / 1 fail**, failing the cached-content assertion after both actual-DOM premise assertions passed.

The fix retains initial serialization for frame discovery and takes the final root snapshot after existing metadata collection and network-event delivery, guarded by the current live session and open Page. It adds no guest evaluation, task pump, timeout or new synchronization to the read-only collector path. The same unchanged control passes with the two-line fix (**1 / 0**). Original evaluation controls (including read-only one evaluation / zero synchronization), public frame/Page controls, native history metadata and opening/close controls pass **17 / 0 / 0 skipped**. Syntax, lint, formatting and pinned Oxlint 1.87.0 CC15 checks pass. These component results are integrated as `9de5b4bc`; they are not a new full-suite claim. The historical aliased-zero-timer failure remains **UNPROVEN / NONREPRODUCED**: its unchanged isolated case and separate observer case both pass, so this component proof does not claim their root causes match.

## Owned-watchdog quiescence classification — causal component proof

The `9de` Render-B failure now has the missing authority evidence: `quiescence_budget::watchdog_during_dom_signature_preserves_reached_interaction_state` returns `Selected async document work predicate is unavailable` while **its own watchdog flag is true both before and after joining the guard** and the isolate termination flag is false. The old result classifier recognized termination only from error wording, so this deadline interruption was incorrectly returned as a genuine helper failure. Raw evidence remains at `.recovery/ci-release-watch/9de-job-135230409524.log` (job `135230409524`, run `44425710055`).

A new deterministic real-PageSession control uses the existing engine namespace fault seam: an infinite selected-work predicate at the unchanged **200 ms** budget. Its reached-DOM assertion passes before the baseline fails with the **same generic predicate error and owned deadline flags**. A separate missing-predicate control at the default budget proves that the **same error remains an error when the operation's watchdog has not expired**, and that restoring the helper permits healthy work.

The fix carries the stable flag sampled after joining this operation's watchdog into its existing best-effort reached-result branch. It does not admit a missing predicate or owner, change budgets, use a parent/global deadline flag, add a task pump, or suppress an unexpired predicate error. Tests are unchanged across baseline and fix: causal baseline **0 / 1**, then quiescence **5 / 0**, private owner/error identity **3 / 0** and driver synchronization **2 / 0**, with no ignored or filtered controls in those ON targets. Affected Render/NAPI all-target Clippy with denied warnings, formatting and CC15 all pass. Receipts: `.worktrees/fix-quiescence-owned-deadline/.recovery/owned-deadline/20261119T005554Z-baseline` and `20261119T005732Z-on`. This is focused local causal proof; combined Linux confirmation is still required. The earlier owner-admission wording variant without its own-deadline diagnostic remains historical evidence, not an independently proven common cause.

## Earlier CI discovery and component checklist

Nine observed families remain tracked. The current release candidate `afa34f5f` locally adopts the validated clock (`e2e55b86`), layout (`2a749c7c`), history (`e0adb000`), host cleanup (`104be`), activation provenance (`2ca824`) and code-generation (`afa34`, source `ee590`) repairs. They are **FIX PUSHED / WAIT COMBINED LINUX CI**, not a current whole-CI green claim. Popup closure and prior scoped JS prerequisite passes remain valid historical evidence. The latest quiescence selected-predicate deadline failure has separate local causal proof above, integrated as `eb1958f4` and awaiting new combined Linux confirmation. The historical matching-peer recovery failure and earlier owner-admission wording variant remain **OPEN / UNPROVEN**; neither is masked by a later pass or another repair. The batch was subsequently pushed successfully as `afa34f5f` (`43386917` → `afa34f5f`); combined Linux CI is still pending. All failed receipts, original budgets, assertions and source/provenance distinctions remain preserved.

| Done | Required family | Actual current failure | Remaining proof |
|---|---|---|---|
| [x] | Popup deadline setup, four cases — **RESOLVED: ORDINARY + INSTRUMENTED LINUX PASS** | The original [job 135042600053](https://github.com/miaskiewicz/turbo-surf/actions/runs/44371738228/job/135042600053) has **0 / 4** at setup; `ac0d81fb` diagnoses setup at 255 ms against 250 ms. After fixture-only [`357dd3ea`](https://github.com/miaskiewicz/turbo-surf/commit/357dd3ea6e7314cecd3eedfb138b886e4bd41e52), `bdd4aa96` [Render-A 135095071343](https://github.com/miaskiewicz/turbo-surf/actions/runs/44387125194/job/135095071343) passes all four under LLVM instrumentation (**3.63 s**), and [ordinary full-Rust job 135102560594](https://github.com/miaskiewicz/turbo-surf/actions/runs/44387125194/job/135102560594) passes the same four (**1.89 s**). Both are **4 pass / 0 fail / 0 ignore / 0 filtered** with the same verified source. | Setup/recovery is separated from the unchanged **250 / 250 / 100 ms** tested watchdog contracts. This closes only the four-case popup family. The overall Render-A job still has a host-input failure, and ordinary full Rust later fails in `window_message_source`; no coverage-gate or whole-suite green claim follows. |
| [ ] | Interrupted-frame input recovery — **CLEANUP PUSHED / WAIT COMBINED LINUX CI** | Historical `06d1ba34` and `bdd4aa96` `host_input` targets fail **25 / 1** at final healthy input after their security assertions. In `43386917` [ordinary Linux job 135140486660](https://github.com/miaskiewicz/turbo-surf/actions/runs/44398946903/job/135140486660), both the genuine provisional-allocation regression (**raw 2820**) and unchanged original host-input case (**5266**) pass on the repaired source. | Clean [`9927f544`](https://github.com/miaskiewicz/turbo-surf/commit/9927f5448a68b0d994c35bb562abef7338cc4384) has causal baseline RED → **34 local passes** and strict zero, followed by this ordinary Linux confirmation. Original security assertions and budgets are unchanged. The final validated cleanup revision is locally adopted as `104be` in candidate `afa34f5f`. Earlier ordinary-Linux confirmation applies to its recorded source, not automatically this new combined graph; new ordinary/instrumented confirmation remains required. This is not a whole-Rust pass. |
| [ ] | Action-deadline recovery after a matching peer failure — **OPEN / INTERMITTENT** | `ac0d81fb` [Render-A job 135067016904](https://github.com/miaskiewicz/turbo-surf/actions/runs/44379168997/job/135067016904): `action_signal_deadline` completed **2 pass / 1 fail**. `matching_peer_failure_cannot_release_an_owned_navigation_deadline` fails at `action_signal_deadline.rs:156` with `Owner entry failed` during recovery navigation. In later `bdd4aa96` Render-A, that case and its target pass **3 / 0**. | **Historical CI cause remains UNPROVEN.** The later pass is a non-reproduction, not a causal repair. The same recovery error recurred on `e6201df4` in Render-A job `135933820968` at line 156. A separate real canceled-caller control now has causal local proof and a guarded cleanup repair, documented below; it does not establish that the historical CI case took this cancellation path. No failure-time `a085` recovery witness has been captured for the historical failure. The 1,000 ms exceeded-watchdog metadata in the `bdd4aa96` log belongs to the intentional host-input frame loop, not this recovery navigation; it cannot establish a fresh recovery expiry. Original assertions, deadlines and expected errors remain unchanged. |
| [ ] | Layout-certificate initialization, two cases — **FIX PUSHED / WAIT COMBINED LINUX CI** | `bdd4aa96` Render-B has **3 pass / 2 fail** at `layout_query_cache.rs:71`. Latest `43386917` [Render-B job 135136936264](https://github.com/miaskiewicz/turbo-surf/actions/runs/44398946903/job/135136936264) passes both historical cases and the complete old target **5 / 0**, with no failure-only budget diagnostic captured. Separately, the causal finite-work control fails on **500 pumps** before the fix and passes after **three pumps** with all settlement assertions intact. | Local clean `2a749c7c` has **16 passes** and strict zero, and is adopted in candidate `afa34f5f`. The current old-code pass is a non-reproduction, not validation of that new fix. The genuine 500-pump bug and local repair proof remain valid; original cases must still pass on the integrated repaired Linux graph. Original 5,000 ms budgets and certificate/settlement assertions remain unchanged. |
| [ ] | DOM-signature watchdog owner admission — **LATEST PREDICATE FAILURE FIXED LOCALLY / WAIT LINUX** | `bdd4aa96` Render-B reports **2 / 1**, with `watchdog_during_dom_signature_preserves_reached_interaction_state` failing at `quiescence_budget.rs:18`: `Timer owner admission: Owner entry failed`. Latest `43386917` [Render-B job 135136936264](https://github.com/miaskiewicz/turbo-surf/actions/runs/44398946903/job/135136936264) passes that case and its complete target **3 / 0**, with no corresponding failure diagnostic captured. | Latest `9de` owned-deadline flags establish the selected-predicate classification defect with matched local RED → GREEN proof above. The original three controls and 200 ms budget remain unchanged; new quiescence/owner/driver controls pass 10 / 0. Affected strict checks pass; integration as `eb1958f4` is complete and new combined Linux confirmation remains pending. The earlier owner-admission wording variant still has no independent deadline proof. |
| [ ] | Incumbent-window messages — **FIX PUSHED / WAIT COMBINED LINUX CI** | `bdd4aa96` [ordinary full-Rust job 135102560594](https://github.com/miaskiewicz/turbo-surf/actions/runs/44387125194/job/135102560594) reports **1 / 1**, observing only one of ten expected messages. The same-JS control proves Chrome .4 message-before-timer versus baseline Surf timer-before-message. Clean `7996a028` passes **11 local controls** and strict checks, and is adopted locally as `e2e55b86d880603a2f93f8ee7199c487b30c4ff1`. | Candidate `afa34f5f` includes the validated repair; combined Linux confirmation is pending. Virtual task-base ordering, RealTime/outside-task fallback and authority checks are preserved. The original ten-row assertion and original 156.0.8078.47 probe/oracle are unchanged; the original Linux failure stays open until the integrated graph passes it. |
| [ ] | Oracle/config completeness — **JS SCOPE PASS / PIN FIX PUSHED / WAIT LINUX CI** | `43386917` JS job 135136935977 retains actual detection **275 / 0 / 0**, five Chrome156 activation captures and all eight previously skipped config controls passing. However, [Render-B job 135136936264](https://github.com/miaskiewicz/turbo-surf/actions/runs/44398946903/job/135136936264) fails `activation_gates_follow_trusted_host_input_like_chrome` at `user_activation_gates.rs:37`: stale `collectorSha256`. | Keep every recorded JS pass, but source-bound activation completeness remains unchecked. The collector changed with CI argument passthrough; the validated provenance repair is adopted as `2ca824`: measured hash `3d3cfde…` is retained and current compatible `895e097…` is strictly guarded; Linux replay remains pending, with no invented capture date. JS still fails nested history and E2E is unrun; prior retry readiness causality remains unproven. |
| [ ] | Nested same-document history notifications — **FIX PUSHED / WAIT COMBINED LINUX CI** | `43386917` [JS job 135136935977](https://github.com/miaskiewicz/turbo-surf/actions/runs/44398946903/job/135136935977) fails the case declared at `history-groups.test.mjs:249`, assertion **265**: expected `["/first","/second","/second"]`, actual `["/first","/second"]`. Normal repair `d87a32ccfb5458c1de8d44eb860e2a3b713d0582` is locally adopted as `e0adb000668f7dda88b8c5b7b5248cb34cf2d890`. | The stale captured-batch race has deterministic local OFF **4 fail / 1 pass** → ON **35 pure passes**, original native case **1 pass**, and related controls **64 passes**, with final review/strict approval. The completed command waits for a fresh event batch rather than accepting a snapshot captured before it; no URL deduplication, three-notification expectation waiver or added sleeps. Candidate `afa34f5f` includes the validated repair; combined Linux validation remains pending. |
| [ ] | Native code-generation capacity and reentry drain — **FIX PUSHED / WAIT COMBINED LINUX CI** | Historical `43386917` ordinary job 135140486660 reports **32 / 1**, failing at `isolation_policy.rs:1716` under the original **5,000 ms** budget. The clean code-generation repair is adopted in local candidate `afa34f5f` (validated source `ee590`). | Actual **34 native cases** pass, with Render/NAPI all-target Clippy, formatting and CC15 zero and final review approved. Critical OFF preserves two semantic passes but fails the count control; ON passes all three with **73,736 reads**. Capacity **4096** and budget **5000 ms** remain unchanged; temporary diagnostics are absent. The original Linux failure and combined graph still require CI validation. |

Final batch readiness: all six adopted repair components above have reviewed actual focused proof; this is not a new combined full-suite result. For code generation, the decisive OFF/ON contrast is two semantic passes plus one count failure → three passes with 73,736 reads, with the 4096 admission cap and 5000 ms budget unchanged. Its full 34-case native set and affected strict checks pass without diagnostic code. For activation, only the correct-source 1-pass replay, 12 fast controls and strict proof are accepted; the initial stale candidate is quarantined. The provenance bridge retains the original measured `3d3cfde…` source and strictly identifies compatible `895e097…`, without manufacturing a new capture timestamp. Local component success does not close the original Linux failures; quiescence and historical matching-peer recovery remain explicitly unproven.

The popup setup cause is established by [diagnostic job 135072001033](https://github.com/miaskiewicz/turbo-surf/actions/runs/44379168997/job/135072001033): **3 pass / 1 fail**, with the engine-read case failing in child setup at 255 ms against 250 ms. Root hydration is already `Complete`, with `budget_exceeded: false` and `open_error: None`; the actual about:blank child exists. Raw log `.recovery/ci-release-watch/ac0-job-135072001033.log` remains immutable. The correction uses the existing standard **5,000 ms setup/recovery allowance**, applies 250 ms immediately around each of the two tested runaway operations, and leaves the explicit setContent 100 ms cap unchanged. All four cases' scripts and assertions are byte-identical to the original after removing only the phase-boundary budget setters; `UNCHANGED-CONTRACTS.json` records that comparison. No production engine code changes.

The clean test source SHA256 is `1a27911946d8d309f15a969e64538168cc1d914ee8e3189ad412e4008795c5f5`. Mac four-case proof, zero-exit Clippy/formatting/CC15 checks and contract comparisons are preserved under `.worktrees/popup-budget-phases/.recovery/popup-budget-phases/20261118T194700Z-9d4ee5e6/`. Linux instrumented confirmation is in `.recovery/ci-release-watch/bdd-job-135095071343.log`, lines **3202–3212**, at **2026-11-18 20:23:33–20:23:37 UTC**; ordinary Linux confirmation is in `.recovery/ci-release-watch/bdd-job-135102560594.log`, with all four actual names passing at **20:35:19–20:35:20 UTC**. Independent same-merge/source receipts are `.recovery/popup-ci-phase-confirmation-bdd/FINAL.json` (SHA256 `acacffdb6b5404133d1636c8585876350b1f35a76f17958e910b895e27341175`) and `NORMAL-FINAL.json` (SHA256 `24f25a6eb96f3509c12c68451d62d67ea4d064882f29c96f26ee9915096e40b2`). Both bind PR merge `5602a0e0b52e2bcc9078635b87b82b2fa758465b`, branch `bdd4aa966398d847baeac3ced597811f922bd405`, and the clean source hash. They close the popup family only: Render-A and ordinary whole-workspace Rust still fail on unrelated cases.

New Render-B evidence is immutable at `.recovery/ci-release-watch/bdd-job-135095071406.log`: the layout failures and **3 / 2** summary are at lines **1917–1949**, and the signature-watchdog failure and **2 / 1** summary are at **1996–2014**. All three failure blocks report `ACTION_RECOVERY_DIAG bridge stage=Owner entry failed`, with `terminating_before_restore=false` and `terminating_after_restore=false`. These are native observations at that point, **not proof of a common cause**, and do not establish whether an earlier watchdog fired. The independent layout handoff proof is recorded below; it is not inferred from these flags. The recurring host-input diagnostic and **25 / 1** summary are separately preserved in `.recovery/ci-release-watch/bdd-job-135095071343.log`, lines **5242–5265**. No source-reviewed candidate receives functional or Linux-fix credit before its actual proof.

The layout handoff has separate causal proof at `.worktrees/layout-postload-handoff/.recovery/layout-handoff/TDD-20261118T213031Z-ea79f37f/{BASELINE,CLEAN-ON}/FINAL.json`. Baseline code completes all real localhost fetch, load-listener delayed DOM work, dynamic script/microtask, rAF, finite-animation, document-identity and unsafe-animation guard assertions, then fails the final check on **500 completed pumps (a count, not a millisecond budget)**. The unchanged regression passes on the clean handoff after **three pumps**; all settlement checks remain intact. All **16** selected layout/hydration/dynamic-script/quiescence/module controls pass, with Render/NAPI all-target Clippy, formatting and CC15 zero. Timer-only work is handed from the selected-native phase to the existing following full-quiescence phase; timers are not dropped and the document is not prematurely declared complete. The original two Linux layout failures still await new-graph CI. Passing three quiescence cases locally as adjacencies does not by itself resolve the separate historical signature-watchdog failure.

The latest `43386917` Render-B log `.recovery/ci-release-watch/433-job-135136936264.log` records both old layout cases passing at **1594 and 1598**, target **5 / 0** at **1600**, and the signature-watchdog case passing at **2195**, target **3 / 0** at **2197**. No associated failure-only diagnostic is captured. These observations do not close the historical signature failure or validate the still-unpushed layout repair. The old 500-pump causal RED and clean ON16 proof remain separate evidence, not contradicted or replaced by a later old-code pass.

Deadline provenance correction: `.recovery/ci-release-watch/bdd-job-135095071343.log`, lines **1984–1989**, records the matching-peer deadline case passing within its **3 / 0** target. The `exceeded_before_join=true`, `exceeded_after_join=true`, `elapsed_us=1004284`, `budget=1000` fields at **line 5254** are instead from `host_input::interrupted_frame_creation_never_exposes_bootstrap_host_capabilities`, whose authored frame-creation loop is intentionally endless. They are not a failure-time observation of `matching_peer_failure_cannot_release_an_owned_navigation_deadline`. No ledger claim that a fresh recovery deadline expired follows from those fields; the historical `ac0d81fb` recovery failure remains open without that witness.

Five bounded macOS reruns of that same matching-peer case pass with the unchanged 1,000 ms budget and `a085` diagnostic artifact. Receipt `.worktrees/action-recovery-owner-ac0/.recovery/action-recovery-owner/bounded-five/FINAL.json` records each raw-log hash, unchanged artifact, status `FIVE_PASSES_NONREPRODUCTION_ONLY` and `noFixClaim: true`. These are repeated non-reproductions, not five distinct obligations or a repair; the historical Linux failure remains **OPEN / INTERMITTENT**, with no captured failure-time recovery-expiry witness.

The host-input cleanup now has actual causal component proof at `.worktrees/provisional-frame-rollback-bdd/.recovery/provisional-frame-rollback/proof/`: the deterministic genuine native-allocation baseline returns the same unavailable-owner error, then the clean repair passes that regression (**1**), the unchanged host-input target (**26**) and ownership/error adjacencies (**4 + 3**), for **34 distinct passes, zero failures or ignores**. Clippy, formatting and CC15 all exit zero, source guards remain unchanged, and semantic review approved the repair. Commit `9927f544` only retires genuinely interrupted unpublished frame allocations under the existing cleanup guards; Root, installed peers and the original host-capability privacy assertions remain protected. The initial component proof did not claim Linux or full-suite green; subsequent ordinary-Linux confirmation is recorded below, without relabeling old full-suite proofs.

Ordinary Linux host confirmation is independently sealed at `.worktrees/provisional-frame-rollback-bdd/.recovery/provisional-frame-rollback/LINUX-NORMAL-CONFIRMATION.json` (SHA256 `02a47665914c911d9631850be2e973fc63d37b83c20daddc23a9e2d863dd7c56`). It binds head `433869170a6a9ee930b4bf44aaa1ed8f73236a5d`, actual executed PR merge `a356431b90eef285a002b6bd4f817949edb26e48`, and `.recovery/ci-release-watch/433-job-135140486660.log` SHA256 `341a99043091853b3714a53a7960cf81cdac5ec5b44e9a40e6a9b87d50425879`. The genuine rollback regression passes at **2820**, original host-input case at **5266**, and matching-peer deadline recovery at **2914**. The latter remains a non-reproduction of its historical failure, not a causal fix. Instrumented host confirmation is pending, and the ordinary job's code-generation failure prevents any whole-suite green claim.

The ninth family's code-generation failure is preserved in that same log at **5454, 5480–5492**. Execute records `elapsed_us=1398839`, `budget=5000`, and `exceeded_before_join=false` / `exceeded_after_join=false`; subsequent drain records `elapsed_us=5001890`, the same budget, and both exceeded flags `true`. Both have `outer_timeout=false` and `terminating=false` at observation. These are failure-time phases of `native_code_generation_tasks_bound_capacity_and_release_before_reentry`, not a witness for matching-peer recovery or another test. The **32 / 1** target summary and job exit101 remain intact; cause is unproven and no budget waiver is made.

The history notification repair preserves the completed-command boundary: if an actor batch was already captured, a command requiring fresh events waits for that batch and then reads a fresh one. It keeps existing event ordering, actor-close exclusion and failure propagation; identical URLs are not deduplicated. Actual proof is under `.worktrees/history-drain-433/.recovery/history-drain-fix/`: `pure-ci-20261118T214758Z/FINAL.json` pins the five focused old-code cases (**4 fail / 1 pass**) and the clean whole-file run (**35 pass**); `native-causal-20261118T215007Z/FINAL.json` passes the unchanged original case, and `native-neighbors-20261118T215037Z/FINAL.json` passes 64 related controls. The native runs use the immutable signed `501ec1a` addon under the reviewed unchanged native-API source bridge, not a newly built final-release artifact. `strict-20261118T215134Z/FINAL.json` and final review approve the component. These scopes are reported separately, not summed into a new full-suite count. No sleeps, URL-only suppression, native owner changes, test expectation changes or new authority are introduced; integrated Linux confirmation remains required.

The new ordinary-Rust message-source failure is preserved in `.recovery/ci-release-watch/bdd-job-135102560594.log`, lines **7026–7044**, including the exact one-message actual array, full ten-message expected array and **1 / 1** target summary. The job exits 101 after that failure; its earlier four popup passes must not be presented as a whole-Rust pass. No cause is inferred from the other families' owner-entry diagnostics.

The stronger message-order control is sealed at `.worktrees/window-message-source-triage/.recovery/window-message-ci/CAUSAL-RED/{PREP,ARTIFACT,RESULT}.json`, with actual failed native log `NATIVE.log` SHA256 `9212398b27a6413318123f77797f5ba33c1e43200c4fb6cf9e56e6529119d207`, exit101 and unchanged source. Both engines execute the exact shared fixture SHA256 `14924fc5ba25553c9d36a12e69aed7c739833c215fd369c1500ced0c8be0bb42`: a real Root timer callback performs 60 ms of authored work, posts to the child, then schedules a Root timer for 40 ms; the Promise resolves only after **both** deliveries. Actual Chrome command/protocol identity is **156.0.8078.4** and returns message before timer. Surf test `posted_message_precedes_later_nested_timer_after_a_loaded_task` returns timer before message (**0 pass / 1 fail**, native attempt10919). In both results, data is `task-clock-message` and `sourceIsWindow`, `currentTargetIsChild`, `originMatches`, and `trusted` are true. Oracle and source hashes are retained in PREP; this does not relabel that browser as the separately frozen 156.0.8078.47 profile. The minimal RED demonstrates ordering divergence independently of the original ten-row test's observation horizon; it does not itself repair that test or prove its only cause.

The clean clock repair is normal commit `7996a0286892c08e21f0bf55c326a39d7b9285a3`, with actual ON proof `.worktrees/window-message-source-triage/.recovery/window-message-ci/CAUSAL-ON/FINAL-VALIDATED.json`: **11 distinct passes, zero failures or ignores**, unchanged source, Clippy/formatting/CC15/JS lint all zero and final semantic review approved. The exact causal ordering control flips from RED to the Chrome order; the unchanged original ten-row case, its realm-entry sibling, message-order/budget controls and nested-timer adjacencies pass locally. Virtual posted-message deadlines select the actual source Window's running task base, then its agent's active task base, then the existing live clock outside a task; RealTime and rendering fallback retain their existing clock behavior. This is private reuse of existing native timer services, with **no new ops or authority paths**. Temporary failure-only message diagnostics are removed. The original .47 control is byte-unchanged; the new compact .4 audit receipt preserves the shared-probe evidence and is not substituted as the native test's expected input. These local results make the clean fix ready in candidate `afa34f5f`, not an integrated Linux or fresh full-suite pass; the original Linux ten-row obligation remains unchecked.

Chrome startup evidence is the actual `ac0d81fb` [stderr artifact 14466775576](https://github.com/miaskiewicz/turbo-surf/actions/runs/44379168997/artifacts/14466775576), retained at `.recovery/ci-release-watch/ac0-chrome-diag/artifact.zip` (SHA256 `64215629b12d2546bf6d95758b31bcab17cabe86ae298475a80068b126be1e3f`). The extracted `chrome-stderr.log` files are in `turbo-cdp-fixture-dfFwcO/`, `turbo-cdp-rootless-fixture-2PcBnw/`, and `turbo-cdp-tls-fixture-iwSitL/` under that directory. All three contain the native Chrome sandbox fatal; the ordinary and rootless captures additionally record `Received signal 6`. This establishes a sandbox startup abort before successful oracle capture, not a live-browser endpoint failure; the earlier Chrome-alive inference is superseded. The AppArmor/user-namespace wording comes from Chrome's own diagnostic; no separate kernel-policy probe is claimed. The CI-only flag is now exercised by the six passing cases below, not added as a production default. It does not establish the cause of the earlier nested retry failure.

The earlier `bdd4aa966398d847baeac3ced597811f922bd405` JS inventory (merge `5602a0e`, raw `.recovery/ci-release-watch/bdd-job-135095071300.log`) remains **273 / 1 / 1** for detection, plus eight separate config skips. The latest actual inventory is `433869170a6a9ee930b4bf44aaa1ed8f73236a5d` (merge `a356431`), run [44398946903](https://github.com/miaskiewicz/turbo-surf/actions/runs/44398946903), raw `.recovery/ci-release-watch/433-job-135136935977.log`: detection **275 / 0 / 0**, shim **816 / 1 / 1**, E2E **unrun**. The nested-history failure is at raw lines **3351–3382**, shim totals at **7224–7232**, and detection totals at **8907–8915**. The earlier local adoption point `ca831d46` is superseded by batch candidate `afa34f5f`, containing clock `e2e55b86`, layout `2a749c7c`, history `e0adb000`, host cleanup `104be`, activation `2ca824` and code-generation repair `afa34`; origin advanced successfully from `43386917` to `afa34f5f` after ledger preparation. Combined CI results remain a separate pending event, not inferred from the push. Local adoption and component proofs are not a combined CI/full-suite or coverage-gate pass.

| Done | CDP/completeness obligation | Actual evidence and remaining work |
|---|---|---|
| [x] | Six original raw-CDP cases recover under CI sandbox arguments | The `bdd4aa96` Linux log records actual passes for #4 consent/query/form stages, #36 result/JS-shell/challenge classification, #37 worker/OOPIF capture, #38 HTTP/2 TLS redirects, #40 rootless post-Enter capture and #80 owned-CDP SIGTERM cleanup. This closes this six-case subset only. |
| [x] | Current #186 retry observation — **LINUX PASS; PRIOR CAUSE UNPROVEN** | The `bdd4aa96` `retry-fast-boundary-chrome` launch failure remains preserved with **0 requests** and missing nested stderr. Diagnostic-only [`779f89db`](https://github.com/miaskiewicz/turbo-surf/commit/779f89db491f6eb39d12a2046333bb970d6ad587) expands collection; it is not a production repair. In the `43386917` Linux log, `both collectors use a fixed Enter window for slow forms and delayed retries` actually passes at **line 8368**. This closes the current execution prerequisite only: the prior intermittent readiness cause is not established or claimed fixed. |
| [x] | Required activation oracle — **LINUX PASS** | The old CI-installed Chrome **149.0.7827.55** mismatch and skip remain historical. [`8946483a`](https://github.com/miaskiewicz/turbo-surf/commit/8946483a871c6d48568a8ebed9eb78e4dfaa9c29) pins official **156.0.8078.47**, verifies archive SHA256 `e6c94490f0b6b337225aeb2d2a5034858516b7017356980ba94f5176d8774a50`, and forwards explicit Chrome arguments. After the 12 local controls, `43386917` CI executes the frozen activation control with **five actual captures** and passes its single registered test (raw **8817–8818**), rather than skipping. No oracle expectations are rerecorded. |
| [ ] | Rust activation source provenance — **FIX PUSHED / WAIT COMBINED LINUX CI** | Historical Render-B135136936264 reports **0 / 1** on stale collector metadata: measured `3d3cfde…` versus current `895e097…` after CI argument forwarding. The proper provenance repair is locally adopted as `2ca824` in candidate `afa34f5f`. | The original measured source hash is retained with an exact compatibility guard for the current collector. The **correct-source** Rust control passes **1 / 0**, fast controls **12 / 0**, strict checks zero and final review approved. The initial stale-source proof is quarantined, not counted. No blind hash replacement, changed behavioral golden or invented `capturedAt` is claimed. Integrated Linux confirmation remains required. |
| [x] | Eight old configuration-control CI skips — **LINUX PASS** | [`d8e96100`](https://github.com/miaskiewicz/turbo-surf/commit/d8e9610074a2f0534bb9f250166acc128de44487) removes only the obsolete `skip: CI` guards. Local `CI=1` contrast remains **22 pass / 8 skip → 30 pass / 0 skip**, natural exit, at `.recovery/ci-config-controls-bdd4aa96/COMPONENT-RECEIPT.json`. All eight formerly skipped config controls execute and pass in `43386917` Linux CI with unchanged expectations; the separate shim failure is nested history, not these prerequisites. Prior skipped executions are not relabeled as passes. |

The four popup cases are `a_target_engine_read_watchdog_preserves_reached_state_and_other_pages`, `a_target_authored_script_watchdog_rejects_without_poisoning_next_evaluation`, `content_script_timeout_keeps_the_root_live_and_accepts_a_later_replacement`, and `polling_an_actor_after_root_close_services_its_actual_surviving_child`. Activation provenance failure is immutable in `.recovery/ci-release-watch/433-job-135136936264.log`, **3964–3976**: the sole Rust activation test fails in 0.04 s before its source-pinned behavior replay. The control records collector SHA256 `3d3cfde8111a1fb4500827672e60f9981034e4b440b88f9c1faf991798fd5aa9`; the current source independently hashes to `895e097cb165707fc87c96a59b7bdcf2ce87d27506209a4d74a56efda9cd5c17` after the reviewed optional Chrome-argument forwarding. This was a source-pin regression within the existing activation family, not evidence that the five successful JS captures failed. The proper provenance repair is now locally validated and adopted as `2ca824`: original capture metadata and behavioral expectations must remain truthful, and no new `capturedAt` is asserted from a metadata-only change. No source fixture is modified by this ledger update.

The seven original JS failures were the consent/query/form-stage control (#4), result-document versus JS-shell/challenge classification (#36), worker/OOPIF capture (#37), HTTP/2 TLS redirect metadata (#38), rootless post-Enter capture (#40), owned-CDP SIGTERM cleanup (#80), and slow-form/delayed-retry Enter windows (#186). These are observed test numbers in that immutable log, not new executions or passing credits.

Raw logs remain under `.recovery/ci-release-watch/`: `06d1-job-135042600053.log`, `06d1-job-135040792899.log`, and `06d1-job-135040792679.log`. Diagnostic batch `ac0d81fb` and follow-up CI [44379168997](https://github.com/miaskiewicz/turbo-surf/actions/runs/44379168997) supplied the popup setup and Chrome fatal witnesses above; that diagnostic run has ended, without establishing a green release. `ac0-job-135067016904.log` preserves both the new **2 / 1** action-deadline failure (lines 3274–3291) and the later **26 / 0** host-input non-reproduction (lines 5188–5198); the latter supplies no failure-only root-owner metadata and does not close the earlier failure. The unchanged diagnostic candidates pass four popup cases plus one host-input case and their strict checks on macOS, recorded at `.worktrees/popup-budget-ci-proof/.recovery/popup-budget-ci/20261118T191217Z-86ad4687/` and `.worktrees/interrupted-frame-owner-06d/.recovery/interrupted-frame-owner/proof/`. Those results establish local diagnostic validity only; they do not establish a Linux fix or remote green status.

Two previously observed families have completed component repairs:

| Done | Resolved family | Actual closure evidence |
|---|---|---|
| [x] | Virtual timer clamp assertion used the wrong clock origin | Test-only commit [`74db5c2f`](https://github.com/miaskiewicz/turbo-surf/commit/74db5c2f86f1db14244efeee5a218c2ce2892b19), integrated as `9828c264`, checks the next due time against the running task's published deadline. The real 8 ms busy-callback baseline fails the old live-clock assertion; the correction passes **20 timer controls**, affected Clippy, formatting and CC15 with unchanged production timer code. Evidence: `.worktrees/timer-taskbase-ci/.recovery/timer-taskbase/proof-20261118T182307Z-23b84e38/{baseline,on}/`. |
| [x] | Owned web-server retirement produced transient `kill EPERM` | Commit [`2675a318`](https://github.com/miaskiewicz/turbo-surf/commit/2675a318940bb7c7835e4ddfe5567463769fd9b1) retains bounded shutdown and persistent-error behavior. Complete local registry JS run **79917** passes **1,147 official + 9 local controls**, with exactly three intentional skips, all 275 detection cases passing without skips, all 43 product files loaded and all coverage gates passing. Independent seal: `.recovery/final-coverage-next/js-registry084-webserver6acafd0/independent-seal/FINAL-INDEPENDENT-JS90-REGISTRY-WEBSERVER-PASS.json`. This does not close the separate current Linux Chrome-readiness failures above. |

The earlier complete local Rust proof (session 57272: 2,175 passes, zero failures, seven explicit ignores; seal `.recovery/final-coverage-next/full-rust-registry57272-independent-seal/FINAL-INDEPENDENT-FULL-RUST-REGISTRY-PASS.json`) and local JS proof 79917 remain source/platform-bound historical evidence. They are not relabeled as results of the current Linux CI or diagnostic graph. All original failed receipts and subsequent diagnostic receipts remain preserved.

## Current integrated repair state

All failures observed in the frozen `c953e15c` discovery now have reviewed, integrated component repairs. This is **not** a claim that the new combined source passes the regression set. Preserve the failed discovery receipts and rerun every required case on the next single public-graph build before running full suites or coverage.

| Observed family | Integrated repair and focused evidence |
|---|---|
| Frame rollback fault injection | `0336d449`: actual document fault hooks; unchanged two tests pass, disabling the fix reproduces both missing receipts, restoration passes. |
| Complete default CSP resource reports | `d0c36469`: removes obsolete filtering and compares all 19 unchanged Chrome rows; original and full-oracle sibling pass. |
| Four layout recapture crashes | `962152c0`: test-only mutation hooks enter their captured document; all four unchanged recapture controls pass. |
| Navigation timeout provenance | `f3de2a22`: authentic owner/deadline errors retain navigation meaning; original sibling-popup and deadline/peer controls pass. |
| Bounded quiescence and signature errors | `a2f3896d`: fresh original three plus seven ownership controls pass; single-seam OFF stalls, exact restoration passes. The earlier stale-library candidate run remains invalid evidence. |
| Retained streams and realm collection | `208a139e`: original retirement oracle, genuine completion/GC, borrowed realm, task-denial and transport controls pass (17 distinct cases); specific OFF fails and restoration passes. |
| Collected terminal promise parents and iframe fetch ownership | `85237e27`, `f7c774f4`: original named-window/cookie controls and five adjacent controls pass; each independent inverse reproduces its specific unknown-ownership error. Pending/unseen parents still fail closed. |
| Initial module host-reply handoff | `700f12e1`: 34 controls pass, including original fulfill/route.fetch and unrelated-peer isolation; disabling only handoff reproduces the original navigation timeout. |
| Storage delivery, FIFO and timer interleaving | `35c3b88d`: 51 same-candidate controls pass, including original partition, storage events, JS peer wakeups and unchanged 200ms watchdog. Essential delivery OFF reproduces both JS failures; restoration passes. Waiter-only OFF was inconclusive and is not claimed as causal proof. |
| Child external-classic fetch ownership | `bd068102`: original classic and adjacent cases plus native parent/child authority negatives pass; raw-fetch-only OFF reproduces the original unknown-ownership error, exact restoration passes. |

The public dependency graph uses native `turbo-dom-v8` 150.4.2 and the reviewed Deno `.4` family. Exact canonical archive checksums, source/license audits and actual public-registry consumers are retained. The next all-known inventory is 843 eligible obligations across 141 jobs (845 audit IDs retain two current-profile-ineligible entries without PASS credit). Required sequence: **all-known regressions 100% green → independent combined review → full Rust and JS suites green → coverage**. Any new failure is appended and fixed; no required failure may become a skip.

## Original cases

| Done | Kind | Original test | Commit / remaining owner and work |
|---|---|---|---|
| [x] | Rust | `late_driver_scripts_update_live_children_and_future_realms_without_rerunning_user_init` | `14226971`; targeted proof complete, combined rerun pending |
| [x] | Rust | `retired_child_host_work_does_not_hold_the_owner_event_loop` | `14226971`; targeted proof complete, combined rerun pending |
| [x] | Rust | `child_imports_preserve_realm_authority_through_eval_function_and_source_url` | `b7007ae`; targeted proof complete, combined rerun pending |
| [x] | Rust | `cross_origin_frames_enforce_native_global_boundaries_and_clone_messages` | `b7007ae`; targeted proof complete, combined rerun pending |
| [x] | Rust | `frame_loader_rejects_guest_parser_code_and_forged_owners` | `b7007ae`; targeted proof complete, combined rerun pending |
| [x] | Rust | `frame_navigation_retains_the_old_blank_realm_without_exposing_the_replacement` | `9cb394f`; targeted proof complete, combined rerun pending |
| [x] | Rust | `iframe_frame_tree_length_and_parent_link` | `f35933a0`; targeted proof complete, combined rerun pending |
| [x] | Rust | `adopted_iframe_retires_its_context_and_reinsertion_gets_a_fresh_document` | `f35933a0`; targeted proof complete, combined rerun pending |
| [x] | Rust | `retired_iframe_document_has_no_default_view_and_keeps_its_own_metadata` | `d6ecd5a2`; targeted proof complete, combined rerun pending |
| [x] | Rust | `same_origin_sandboxed_frames_navigate_top_only_with_allow_top_navigation` | `68e5186e`; targeted proof complete, combined rerun pending |
| [x] | Rust | `debugger_inventory_captures_real_eval_function_and_child_sources` | Merged `8049cbc3`; original and critical inverse/restored pass, actual registry consumer inventory/module/extension/Node controls and strict pass; final full gates pending |
| [x] | shim | `commit reload hydrates the new document only when requested` | `81423b6d`; targeted proof complete, combined rerun pending |
| [x] | shim | `commit history document replacements retain on-demand execution` | Merged `3aaeed4d`; fresh committed-source history 31/31 and interruption/identity 3/3 pass, affected strict/fmt/CC/hooks pass; current merged-source all-target check passes. Final combined gates pending |
| [x] | shim | `aliased DOM mutations, errors, cookies and child documents stay synchronized (#297)` | `e75cc661`; targeted proof complete, combined rerun pending |
| [x] | shim | `real Location navigation and same-URL reload still fetch and execute (#319)` | Merged `3aaeed4d`; all 31 original/parity/policy/alias controls and three interruption/identity tests pass on fresh committed-source artifacts. Native document-group restoration, awaited history tasks and initial-blank load bookkeeping are included; final combined gates pending |
| [x] | shim | `initial module top-level awaits can complete host fulfillment and route.fetch` | Merged `cf51f309`; five published dependencies verified, real registry graph passes native 5, Node 4, original TLA, inventory 1, extension 5 and strict/fmt/CC; official controls registered; final full gates pending |
| [x] | shim | `background binding collection errors remain observable` | `f1fb5c63`; targeted proof complete, combined rerun pending |
| [x] | shim | `native tabs and navigation match the source-pinned Chrome profile control` | `ff0a767`; targeted proof complete, combined rerun pending |
| [x] | shim | `HTTP attachments emit binary downloads without replacing the live document` | `e51c6f21`; targeted proof complete, combined rerun pending |
| [x] | shim | `headers remain observable during initial-close hydration` | Merged `f4f5ca7e`: real completion acknowledgment before actor teardown, including concurrent final root/popup closure. Original/restored 11 + Rust 15 + module 5/inventory 1/Node 5 + strict pass. Deterministic host-terminal production inverses give two assertion failures and one passing control, exact restore passes; native final-teardown OFF noncompletion/cancellation is corroborating evidence, not claimed as a clean 5s assertion. Fresh combined 7f gates remain pending |
| [x] | shim | `unrouting every handler survives a fresh realm and later route reactivation` | `ff0a767`; targeted proof complete, combined rerun pending |
| [x] | shim | `a missed web-first assertion hydrates default navigation while commit stays static` | `98c8a8b8`; targeted proof complete, combined rerun pending |
| [x] | shim | `request metadata and client hints survive hydration and stay context scoped` | `8f511cfa`; targeted proof complete, combined rerun pending |
| [x] | shim | `no-content navigation retains the active document and history (#428)` | `efdc3f0d`; targeted proof complete, combined rerun pending |
| [x] | shim | `direct navigation referrers are explicit and reload retains them (#426)` | `8f511cfa`; targeted proof complete, combined rerun pending |
| [x] | shim | `history navigation honors networkidle hydration (#425)` | `d1b61ed7`; targeted proof complete, combined rerun pending |
| [x] | shim | `navigation deadlines also cancel slow hydration (#427)` | Merged `37062d94` with the verified public `.3` Deno family. Exact owned absolute deadlines bound nested initialization without a persistent actor-wide watchdog; interrupted child work and old module evaluations retire only after outer guards unwind. Current actual-archive original/official/module controls pass 10/10, plus history 31/31 and interruption/identity 3/3. Read latency fell from 8545ms to 12ms while finished documents, cached/fresh imports and a peer's held module survive. API-only OFF reproduces the old unresolved-TLA recovery failure; exact restore passes. Core host API guards, affected strict/fmt/CC/hooks and real-registry all-target check pass. Final full gates pending |
| [x] | shim | `viewport resize updates live layout media and only its own page (#369)` | `864b3343`; targeted proof complete, combined rerun pending |
| [x] | shim | `same-URL form navigation invalidates retained screenshot image bytes (#489)` | Merged `5144ec81`; original plus seven image admission/ownership/pixel controls pass. Essential handoff OFF reproduces exact extra-image counts, byte-exact restore passes. Native resource-cap/retirement controls, action/history combined checks and affected acceptance pass; final full gates pending |
| [x] | shim | `frame locator typing uses canonical child selection and child event constructors` | `0a451d99`; targeted proof complete, combined rerun pending |
| [x] | shim | `Browser credentials answer Basic challenges and stay origin scoped (#394)` | `90ca133b`; targeted proof complete, combined rerun pending |
| [x] | shim | `static frame identity survives promotion to the live native registry (#447)` | `0a451d99`; targeted proof complete, combined rerun pending |
| [x] | shim | `in-flight screenshot assets cannot repopulate a reloaded document cache (#489)` | `dcdbbdf7`; targeted proof complete, combined rerun pending |
| [x] | shim | `frame locator interactions do not crash (#280)` | `205a5556`; targeted proof complete, combined rerun pending |

## Additional findings — required before completion

- [x] **Locator.count stale document** — merged `94e4915b`. Original count 0→1, fix-only OFF reproduces 0, exact restore passes six targeted/read-only controls; root merged-source routing and locator-read files pass 10/10 (3.62s) on signed ba119 artifact. Reads current live DOM without evaluate, guest execution or full serialization; final combined rerun pending.
- [x] **Locator.all stale document** — merged `e00e36e7`. Regression and fix-only inverse reproduce stale 0; restored live enumeration passes, including returned/indexed locators and static-disabled controls. Root merged-source routing/read files pass 10/10 (3.51s); final combined rerun pending.
- [x] **Platform-value ownership** — merged `659a1931`: Rust 6/6, platform/subclass 7/7 and infrastructure 7/7 pass. Allocation-only OFF produces five expected failures with DOMPoint controls green; byte-exact restore 14/14 green. Render strict/format/CC/hooks pass. Final combined build/gates pending.
- [x] **Broader native history gap** — merged `3aaeed4d`. Authenticated native document/group/state restoration, protected private-mirror acknowledgment and exact interrupted-document retirement pass fresh history 31/31 plus interruption/identity 3/3. Materialization, replacement and initial-load receipt inverses reproduce specific failures; cleanup-only OFF reproduces an active unacknowledged root, exact restoration passes. Task-pump OFF blocked at the outer deadline and is recorded only as corroborating evidence. Current Case51/activation composition passes all-target Render/NAPI check; prior affected strict/fmt/CC/hooks pass. Navigation API cross-group promises remain explicitly unsupported; driver/History cross-group traversal and Navigation same-group traversal are covered. Initial-open deadline interoperability remains separate, and final combined gates remain pending.

## Additional confirmed failures and investigations

- [x] **Normal iframe parent-load completion** — merged `92f5901c`. Official positive and held/error-peer controls pass; timer-only OFF reproduces the exact 2000ms load TimeoutError, byte-exact restoration passes. Strict, fmt, complexity and hooks pass. Deadline safety remains separate.

- [x] **Native JSON collectors preserve authored getter errors** — merged `ec46bdf2`. New root/child and unchanged original host_input.rs:239 all pass (3/3); serializer-only OFF reproduces all three specific generic-error failures, byte-exact restore passes. All-target Render/NAPI strict, fmt and CC pass. Script-navigation admission remains separate/open.
- [x] **Script navigation captures admission activation** — merged `f79d9a97`. Accepted admission metadata and trusted activation use the actual native guest Document. Original #398 plus six quota/activation/peer/HTTP controls pass; selector-only OFF reproduces three failures, exact restore and strict pass.

Baseline classification: the Enter, frame-top form and programmatic-submit cases all reproduce on byte-exact old main `6b583d98` shim plus verified registry addon `bdcebe65` (3 intended failures, no skips/cancellations, 3.442s). These are preexisting gaps, not regressions introduced by the new action candidate. All remain in scope.

- [x] **Enter-key form completion** — merged `5144ec81`. Implicit-submit and whole-press signal controls pass; essential caller inverses reproduce missing activation and early press settlement, exact restoration passes. Raw input and ordinary evaluate/read remain nonblocking; native trusted-input control passes.
- [x] **Frame form targeting top-level completion** — merged `5144ec81`. Actual native inherited base resolution preserves source/origin/CSP; original plus eight base/ownership controls pass, including parent/child bases, spoofed getters and auxiliary blank documents.
- [x] **Programmatic form background scheduling** — merged `5144ec81`. Ordinary evaluate plus waitForURL now progresses accepted native form work. Real wire/document and dropped-background-turn controls pass without making passive reads wait or admitting unrelated target work.

- [x] **Classic-script response visibility during opening close** — merged `8c62b9dc`. Independent four-control ON passes; observer-forwarding-only OFF yields two explicit missing-header assertion failures; exact restored fresh build passes all four. Pure Rust 2 and all-target Render/NAPI strict, formatting, CC and hooks pass. CORS/CSP/SRI and body/header assertions remain intact. Opening-close acknowledgment race is separate and remains open.
- [x] **Child-frame click navigation (#280) older-addon failure** — whole surface replay on newer platform addon passes the original. Old E118 failure was missing accepted native fixes; final composed build must still rerun it.
- [x] **Shadow-link default navigation** — merged `5144ec81`. Target-owned action completion and authoritative mirror updates pass the original and combined action controls; passive reads do not pump guest navigation.
- [x] **Page.click GET form completion** — merged `5144ec81`. Accepted forms are scheduled for the actual target and click waits for commit. Native multi-target queue/isolation controls, held-load commit timing, typed failures and mixed queues pass; full combined gates pending.

- [x] **NAPI all-target strict dead-code errors** — merged `680ee757`: restored missing crate-root `live_image_assets` re-export. Test-first E0425 RED → export fix GREEN; focused task-construction unit and NAPI all-target strict pass. No lint suppression or removed capability.

- [x] **Detection same-URL POST control** — fresh current `4e1634a6` source with immutable MCP example `6403299b` passes the original in 7.35s. Old pending-Promise symptom is gone. Verified clear without an additional source fix; not a new merge.
- [x] **Detection consent native input** — merged `1f7d97f1`. Actual guest dispatch with same-owner cancellation, retired-skip and captured wait primitives. Original consent passes; selector-only OFF reproduces the same Illegal invocation, byte-exact restore passes. Three new input/cancel/poison guards pass and all-target Render/NAPI strict passes. Whole host-input cohort has 24 passes and two independent tracked failures; no existing tests skipped.
- [x] **Detection retry / MCP root navigation progress** — merged `016ebd7b`. Product collection and example observation progress only admitted native root work within the original deadline; cooperative polling preserves executor progress and does not consume a peer's history task. All eight combined controls pass, including real root/peer queued forms, held/rejected peers, peer history, fixed-deadline unfinished I/O and the unchanged original retry. Separate product-only and harness-only inverses reproduce stale collection and zero results, exact restoration passes; strict/fmt/CC/hooks pass. Final full gates pending.

- [x] **Frame Enter after top navigation** — merged `5144ec81`. Atomic native-root routing preserves physical keyup after replacement, and post-commit mirrors retire stale frames. Six focused plus 20 adjacent controls pass; routing-only and mirror-only OFF reproduce failures, exact restoration passes.
- [x] **FontFaceSet private WeakMap exposure** — merged `5144ec81`. Captured primitives protect both private maps; zero-hook/font brands/status/ready controls pass. Native get-only OFF reproduces eight hooks, exact restoration passes.

## Original component tally

Accounting: 34 original test cases plus 19 additional tracked cases/findings. This is not a count of unique GitHub issues.

- Merged with component proof: **52**.
- Verified clear without an additional source fix: **1** detector POST control.
- Historical component batch: **53 done of 53**. This does not count the later Rust/JS failures or describe current pending work.
- These 53 findings describe the earlier component campaign. The full-suite failures below reopen release completion; this tally is not a claim that the current release is green.
- Final combined gates and coverage remain pending for the release as a whole.

## Full-suite regression follow-up

The frozen `f1b4d10e` discovery run recorded **78 Rust assertion failures**, **13 cases without outcomes across two crashed targets**, and **207 JavaScript failures**. The regression subset retains every case and appends newly exposed failures. Source proposals do not count as fixed until runtime proof passes. The authoritative case manifest is `.recovery/final-gates-f1b4d10e/all-full-run-regressions-root-manifest.json`.

- [x] **Native CSP code-generation callback support** — merged and pushed `df9721df`. The three original default-engine ignored controls now run normally and pass, together with two authored Chrome Trusted Types controls. All five pass on the isolated release component without the pending async-quiescence change; exact all-target Render/NAPI strict, formatting, CC, hooks and policy-intersection unit pass. The graph pins turbo-dom `af946ea0` and its generated V8 `03839d09`, using the same native V8 archive. Receipt: `.worktrees/native-csp-release-adoption/.recovery/native-csp-adoption/final-receipt.json`.
- [x] **Stale native layout override ignore** — merged `b24e24a1` after the unchanged explicitly ignored control passed.
- [x] **Fullscreen task ordering** — integrated modular commit `b0f14b20`. The original fullscreen golden and eight timer controls pass on the independently built component without the async-quiescence proposal; exact all-target Render strict, formatting, CC and hooks pass. Each timer task now retains its task boundary through the complete microtask checkpoint, and reentrant timer dispatch defers to the outer pump.
- [x] **Trusted Escape/input document ownership** — merged `f54f08a3`. Host input dispatch and close-watcher processing enter the authenticated actual document. All four focus/interest controls pass, including the unchanged original Escape behavior, canceled/untrusted input, peer isolation and adopted targets; strict and normal hooks pass. String-timer changes are excluded from this commit.
- [x] **Independent cancellation and host-route ownership guards** — merged `50c7f331` through `8c43590b`, excluding the unsafe async-quiescence and logical-root eval proposals. All 202 required cases pass across the original batch and corrected screenshot case replay; host routes 20, same-actor peer isolation 1 and actor controls 30 pass. Exact Render/NAPI/MCP all-target strict reports zero warnings. Current popup/load closure checks remain a separate obligation.
- [x] **Native screenshot residency contract** — merged test-only `6fdb37f8`. Four controls pass independently without the async-quiescence proposal: existing viewport/full-page/prototype checks and a genuine unretained 61st image requiring one fallback HTTP fetch, correct pixels and warm reuse. The original residency limit rejects additional retention rather than evicting the first record; no production cache behavior was changed.
- [x] **String-timer execution and policy ownership** — merged `a893e18b` through `d832cee5`. Both code-generation admission/reporting and source execution use the authenticated actual document. All three unchanged string-timer controls and the corrected actual-realm timer-nesting fixture pass; all 12 nesting/task/checkpoint assertions remain. Strict and hooks pass.
- [x] **Observable error ownership and unknown locations** — merged `29dc4233` through `6456e53d`. Original Chrome and child-realm controls plus the five-error host/proxy/unknown-column control pass independently. Checked column conversion maps unknown locations to zero; guest identity, known locations and safe host-error conversion remain enforced. Exact fresh Rlib, strict and hooks pass.
- [x] **Font registry ownership and initialization** — merged `24baa970`. Every actual window starts with the same cached font policy; attaching or removing web fonts updates only its authenticated document registry. Independent canvas 2 and font-face 2 controls pass, including the unchanged 428-metric oracle and peer-detach stability.
- [x] **Transferred-stream completion observation** — merged `fa38e0e7`. The private native observer handles infrastructure promises without invoking guest Promise hooks. All eight original stream controls and the corrected transfer-phase/brand/poison control pass. Font/stream strict, hooks and fresh native Rlib/test provenance pass.
- [x] **Native window names** — merged `39f4e46e` through `c4043f0d`. Captured authentic native name access preserves the original three Chrome controls plus the poisoned-getter/rename/removal control. Independent 4/4, strict, hooks and fresh native artifacts pass; no cross-origin authority or producer-private access was added.
- [x] **Storage error ownership and quota atomicity** — merged `fc395053`. Reserved native replies produce method-realm TypeError or QuotaExceededError without exposing infrastructure exceptions. Original brand, borrowed-method, foreign-interface and Chrome quota controls pass independently.
- [x] **WebSocket binary ownership** — merged `c38209d8` through `aa10a887`. Captured socket-realm typed arrays copy the exact native byte view for WebSocket and WebSocketStream; original transport/CSP/nonblocking and new root/child buffer/event checks pass. Combined Storage/WebSocket 8/8, strict, hooks and fresh native artifacts pass.
- [x] **Dictionary document dispatch** — merged `28f3e069` through `317693aa`. Authenticated document access and the existing complete window-context inventory service actual root/auxiliary queues. Independent original 4/4, including the 34-case oracle and 300-link completion, pass without the async proposal. Strict and native library/dispatcher assembly provenance pass. Earlier mislabeled quiet artifacts contained old code and remain explicitly invalid candidate proofs.
- [ ] **Remaining full-suite failures and crash outcomes** — expanded replay executed all 48 selected binaries: **752 passed, 12 failed, 4 ignored, zero crashes**. All original failures and all 13 formerly unreported crash cases have matched outcomes: original 78 → 72 pass / 6 fail; crash 13 → 11 pass / 2 assertion failures. Four additional controls fail. No required case was unmatched. The later focused Escape fix is recorded above. The legacy root-eval held-peer timeout and sticky-activation carry remain open. IDB has 10 of 11 controls plus the role/crypto control passing; its new peer-continuation control still fails. JavaScript replay matched all 207 original failures: **205 pass / 2 fail**; its broader 465 cases yielded 461 pass / 3 fail / 1 intentional skip. Final combined gates remain pending.

## Completion gates

User sequencing requirement: collect the complete Rust and JavaScript failures, fix every failure with targeted and adjacent checks, and independently review the final combined changes before repeating both full suites. Coverage follows green full gates.

- [ ] Validate each production fix: unchanged meaningful RED → GREEN → critical fix-only OFF reproduces intended RED → byte-exact restore GREEN.
- [ ] Merge and push every validated component; keep this file and the detailed receipt ledgers synchronized.
- [ ] Verify latest turbo-dom release head before the final integrated build, then pin and push the final graph.
- [ ] Fresh build from that pushed commit; verify source and artifact identity.
- [ ] Rerun all original failures and every additional regression on that source/artifact.
- [ ] Complete Rust workspace tests, strict all-target Clippy, core no-default-feature Clippy, formatting, Rust/JS CC≤15, shim/e2e/detection and release utility gates.
- [ ] Measure current official coverage shards and pass unchanged aggregate/per-crate floors. No floor reductions or new exclusions.
- [ ] Report all results, intentional exclusions and remaining limitations. Payroll validation requires separate express user go-ahead.

## Latest broader component replay

On main shim/test source `21361462` and immutable platform addon `6b8b`: six full locator/shadow files completed **232 total: 227 passed, 4 failed, 1 existing skip, 0 cancellations (83.37s)**. Failures are the tracked shadow-link, GET form, deadline #427 and same-URL form screenshot #489 cases. This is not the final composed source/artifact result. Source/artifact hashes remained unchanged.

## Evidence

- Original case manifest: `.recovery/issue-510/regression34-cases.json`.
- Test/run orchestration: `.recovery/issue-510/orchestration-checklist.md`.
- Component receipts: `.recovery/integration-regression-checklist.md` and each isolated worktree’s `.recovery/` directory.
- Native-profile full original receipt: `.recovery/issue-510/native-profile-e118-main814/`.

## Exhaustive Rust and JavaScript failure inventory

All **298 baseline regression cases** are listed below: 78 Rust assertion failures, 13 crash-affected Rust cases and 207 JS failures. These are mandatory rerun obligations, not 298 unique causes or a claim that they remain failing. Historical names remain recorded even when a reviewed parser correction is required. Passing in a worktree does not establish a passing integrated full suite.

Additional discovered blockers remain in scope until integrated proof:

- [ ] Async ownership: unrelated held peer must not block root evaluation; explicit peer dependencies and fire-and-forget crypto/streams must finish correctly.
- [ ] IDB collection, storage correctness and root-to-peer continuation dependency controls.
- [x] Sticky activation across same-origin navigation, initialization, cross-origin and sandbox controls: `57e0355a` integrated after genuine-input security controls passed.
- [x] Child fragment clearing preserves the authored trailing hash; HTTP and srcdoc controls: `a8e085f0` integrated after eight Rust and one unchanged Node control passed.
- [x] String timer ownership and code-generation admission: `a893e18b` integrated with original three controls and exact nesting fixture green.
- [x] Dictionary admission/enumeration and link-load completion: independent `28f3e069` integrated with both original failures and positive controls green.
- [x] Font registry initialization and peer-detach stability: `24baa970` integrated with independent original and ownership controls green.
- [x] Observable host-error unknown-column overflow and safe realm attribution: `29dc4233` integrated with independent three-control proof.
- [x] Stream sampling at the correct phase, guest brands and poisoned-hook isolation: `fa38e0e7` integrated with independent 9/9 proof.
- [x] Popup managed-context fixture and timer-nesting fixture: `d08aac47` and `a893e18b` integrated; original failures retained and corrected contracts pass.
- [x] History budget control uses supported push operation while retaining budget and atomicity assertions: `94effd92` integrated with the five policy controls green.
- [x] MCP form admission assertion and detection form-commit attribution: `04581609` integrated with the unchanged physical-request and native-form controls green.
- [x] Screenshot 61st-image fallback: independent 4/4 proof and test-only `6fdb37f8` are integrated; final combined rerun remains required.
- [x] Independent navigation deadline/host ownership guards: `50c7f331` is integrated with independent 202-case proof; separate popup/load contracts also pass 20/20. GitHub issue closure is audited separately.
- [x] Storage receiver/quota ownership, WebSocket buffer ownership and window-name isolation controls: `fc395053`, `c38209d8` and `39f4e46e` integrated with independent focused proofs.

Ignored/skipped audit obligations: native CSP three controls and native layout one control are enabled and component-green; IDB ignore must be resolved with the dependency controls. Keep the existing external TLS probe explicitly classified as live/manual. Audit the six generated documentation examples separately. Run the 12 eligible Chrome/Patchright oracle cases with dependencies enabled; retain only the three intentional runner-semantics skips, whose bodies are designed not to execute. No failure may be reclassified as skipped to pass a gate.

### Additional validation findings

- [x] **Late native navigation attribution** — `89b36f35` integrated the final-counter refresh after the existing observation window. Three gated Rust controls, the unchanged full navigation oracle and four adjacent native controls pass (8/8); strict checks have zero warnings. All twelve eligible oracle contracts now have actual passing evidence across retained source epochs, not a newly claimed single twelve-case batch. The original assertion failure and interrupted timeout remain recorded; clocks and expectations are unchanged.
- [ ] **Real compression stream exposure** — the combined async product's new fire-and-forget SHA256/gzip/HTTP control reaches crypto, then fails because `CompressionStream` is an existence-only placeholder rather than a constructor. The exact published `14_compression.js` and native operations are already pinned and available. Minimal private-backend wiring and constructor/roundtrip/error guards are under validation; the failing stream assertion is retained.
- [ ] **Final async ownership matrix** — private current-engine proof now covers IDB/dependency/lifecycle 19, legacy void-IO/peer controls 4, crypto/stream error controls 3, unhandled-rejection controls 2, module/TLA controls 3 and hardened current-engine core controls 6. These passing receipts span explicit source revisions; they are not a final integrated full-suite run. Essential inverse/restored proof, pure boundary tests, strict checks, independent review and published dependency identity remain required before merge. Bootstrap, Infrastructure timer, JSON-read fixture and missing ScriptOrigin failures remain recorded.
- [ ] **Zero-IV crypto error class** — native encryption/decryption returned `NotSupportedError`, while the exact Chrome 156 oracle returns `OperationError`. A narrow owned-dependency zero-length-only correction passes encryption, valid-ciphertext decryption, owning-realm and private-constructor controls. Publication, causal inverse/restored proof and integration remain pending; other unsupported lengths are unchanged.
- [ ] **Authored script module referrer** — a real peer's relative dynamic import failed before HTTP because native authored compilation omitted ScriptOrigin. The reviewed actual-document URL fix passes all three original held/rejected/void-module controls on the private candidate. Integration and final gates remain pending.
- [x] **Current-engine SDK callback ABI** — `0aa572fc` selects the interceptor callback ABI from the actual linked engine rather than the optional feature. Turning the fix off reproduces both E0631 errors; exact restoration compiles and seven native controls pass, including the formerly ignored current-feature case. Affected strict checks pass.
- [x] **GPU control strict lint** — `fba7ec3a` preserves all 64 exact RGBA assertions using array views; full workspace all-feature strict checks and actual Metal controls pass.
- [x] **Secure-upload fixture strict lint** — `e22a3877` removes only a redundant unit expression. Full all-feature strict checks and both credential/TLS controls pass.

The eligible-oracle receipts and source epochs are retained in `.worktrees/oracle12-current/.recovery/oracle12/MONOTONIC-LEDGER.json`; discoveries append to, rather than replace, the 298 baseline cases below. Actual feature proof includes core defaults-off, codec profiles, MCP/NAPI Metal pixels, CoreText and the current-engine SDK profile. The all-feature workspace check and strict checks pass. Python wheels and remaining isolated feature profiles are still required; no cross-platform or alternative-engine result is claimed.

### Rust assertion failures (78)

1. `tests::human_interact_does_not_bypass_form_submission_admission` — Running unittests src/lib.rs (rust/target/debug/deps/turbo_surf_mcp-eafc73731efcfb99)
2. `broadcast_channel_dispatches_native_targets_and_cloned_messages` — Running tests/api_contracts.rs (rust/target/debug/deps/api_contracts-9bf4d977fdc8a45a)
3. `canvas_media_presence_stubs_match_chrome_156_shape` — Running tests/api_contracts.rs (rust/target/debug/deps/api_contracts-9bf4d977fdc8a45a)
4. `storage_is_branded_inherited_and_coerces_named_keys` — Running tests/api_contracts.rs (rust/target/debug/deps/api_contracts-9bf4d977fdc8a45a)
5. `resource_loaders_clear_site_data_and_ancestor_partition_match_chrome155` — Running tests/cache_advanced.rs (rust/target/debug/deps/cache_advanced-f327c071701990e3)
6. `measure_text_matches_chrome156_with_a_host_measurer` — Running tests/canvas_measure_text.rs (rust/target/debug/deps/canvas_measure_text-6488e9d35d15479c)
7. `native_no_cors_embedder_policy_and_real_cookie_headers_match_chrome` — Running tests/coep_resources.rs (rust/target/debug/deps/coep_resources-78f788f7a9b9455b)
8. `native_embedder_redirect_policy_and_cookie_hops_match_chrome` — Running tests/coep_resources.rs (rust/target/debug/deps/coep_resources-78f788f7a9b9455b)
9. `srcdoc_fetch_inherits_embedder_policy_and_preserves_child_error_realm` — Running tests/coep_resources.rs (rust/target/debug/deps/coep_resources-78f788f7a9b9455b)
10. `dictionary_discovery_policies_and_nonblocking_lifecycle_match_chrome` — Running tests/compression_dictionary.rs (rust/target/debug/deps/compression_dictionary-45204cf17968d131)
11. `every_dictionary_link_settles_past_256_links` — Running tests/compression_dictionary_budgets.rs (rust/target/debug/deps/compression_dictionary_budgets-0040824ca7485a9e)
12. `computed_defaults_and_titles_through_a_surf_session_match_chrome156` — Running tests/computed_defaults.rs (rust/target/debug/deps/computed_defaults-b923ae5397e9bb68)
13. `source_pinned_chrome_reports_use_actual_private_transport` — Running tests/csp_reporting.rs (rust/target/debug/deps/csp_reporting-8e49433ebc0e7a00)
14. `stalled_reports_survive_frame_retirement_without_realm_roots_and_end_with_profile` — Running tests/csp_reporting.rs (rust/target/debug/deps/csp_reporting-8e49433ebc0e7a00)
15. `high_entropy_js_uses_the_same_profile_as_request_headers` — Running tests/detection_trace.rs (rust/target/debug/deps/detection_trace-551be6c0ae2ea51b)
16. `failed_native_insertions_retire_only_frames_detached_by_actual_conversion` — Running tests/dom_adoption_contracts.rs (rust/target/debug/deps/dom_adoption_contracts-261b659af4fae681)
17. `ordered_dynamic_classics_fetch_concurrently_and_scope_current_script_to_execution` — Running tests/dynamic_script_preparation.rs (rust/target/debug/deps/dynamic_script_preparation-e3f95d3acdfb25fc)
18. `retired_document_script_preparation_stays_inert_after_adoption` — Running tests/dynamic_script_preparation.rs (rust/target/debug/deps/dynamic_script_preparation-e3f95d3acdfb25fc)
19. `opaque_bodies_and_response_headers_stay_private_when_guest_intrinsics_change` — Running tests/fetch_headers.rs (rust/target/debug/deps/fetch_headers-2d6ddd8e413dc0db)
20. `native_fetch_headers_body_cancellation_and_binary_upload_match_chrome` — Running tests/fetch_streams.rs (rust/target/debug/deps/fetch_streams-c0380eb8169aeb7b)
21. `trusted_hover_and_focus_show_interest_targets_after_css_delays` — Running tests/focus_interest_input.rs (rust/target/debug/deps/focus_interest_input-776e2325f386d5ba)
22. `font_face_csp_font_src_matches_chrome156` — Running tests/font_face.rs (rust/target/debug/deps/font_face-803480bbfc089e31)
23. `font_face_loading_matches_chrome156` — Running tests/font_face.rs (rust/target/debug/deps/font_face-803480bbfc089e31)
24. `failed_replacement_initialization_releases_roots_and_same_host_can_retry` — Running tests/frame_initialization_rollback.rs (rust/target/debug/deps/frame_initialization_rollback-764a628e4cb39c7e)
25. `failed_later_frame_navigation_can_recover_on_the_same_host` — Running tests/frame_initialization_rollback.rs (rust/target/debug/deps/frame_initialization_rollback-764a628e4cb39c7e)
26. `parent_window_messages_survive_child_retirement_and_detached_hosts_reconnect` — Running tests/frame_lifetime_contracts.rs (rust/target/debug/deps/frame_lifetime_contracts-6c6973445c0a0c3b)
27. `initial_markup_frame_cannot_own_isolate_lifetime_crypto_metadata` — Running tests/frame_lifetime_contracts.rs (rust/target/debug/deps/frame_lifetime_contracts-6c6973445c0a0c3b)
28. `native_iframe_source_resolution_ignores_page_url_query_and_attribute_overrides` — Running tests/frame_ownership_contracts.rs (rust/target/debug/deps/frame_ownership_contracts-dfcc8c1c627142e5)
29. `inactive_parsed_document_has_no_iframe_browsing_context` — Running tests/frame_ownership_contracts.rs (rust/target/debug/deps/frame_ownership_contracts-dfcc8c1c627142e5)
30. `private_iframe_lifecycle_ignores_page_maps_queries_and_namespace_helpers` — Running tests/frame_ownership_contracts.rs (rust/target/debug/deps/frame_ownership_contracts-dfcc8c1c627142e5)
31. `implicit_foreign_document_adoption_retires_old_window_and_preserves_borrowed_accessors` — Running tests/frame_ownership_contracts.rs (rust/target/debug/deps/frame_ownership_contracts-dfcc8c1c627142e5)
32. `collecting_a_host_reply_does_not_reenter_document_hydration` — Running tests/host_bindings.rs (rust/target/debug/deps/host_bindings-aa725cb980bcb544)
33. `initial_module_host_dependency_yields_and_resumes_the_same_realm` — Running tests/host_bindings.rs (rust/target/debug/deps/host_bindings-aa725cb980bcb544)
34. `post_load_host_dependency_yields_before_repeating_hydration_passes` — Running tests/host_bindings.rs (rust/target/debug/deps/host_bindings-aa725cb980bcb544)
35. `request_cache_modes_validators_vary_and_referrer_match_chrome155` — Running tests/http_cache.rs (rust/target/debug/deps/http_cache-eb7fe3753ac03003)
36. `indexeddb_partitions_match_chrome156` — Running tests/indexeddb.rs (rust/target/debug/deps/indexeddb-0a7c7f3af246acea)
37. `enforced_csp_violation_tasks_match_chrome` — Running tests/isolation_policy.rs (rust/target/debug/deps/isolation_policy-e204e186c60d78b4)
38. `external_resource_reports_and_original_utf16_samples_match_chrome` — Running tests/isolation_policy.rs (rust/target/debug/deps/isolation_policy-e204e186c60d78b4)
39. `report_only_csp_violation_tasks_match_chrome` — Running tests/isolation_policy.rs (rust/target/debug/deps/isolation_policy-e204e186c60d78b4)
40. `shared_memory_queue_accounting_preserves_admission_and_growth` — Running tests/isolation_policy.rs (rust/target/debug/deps/isolation_policy-e204e186c60d78b4)
41. `csp_task_capacity_is_bounded_and_recovers_after_dispatch` — Running tests/isolation_policy.rs (rust/target/debug/deps/isolation_policy-e204e186c60d78b4)
42. `resources_use_captured_event_constructors_after_guest_replacement` — Running tests/lifecycle_security.rs (rust/target/debug/deps/lifecycle_security-56ee4e3113fff0c1)
43. `native_key_material_counts_against_queue_admission_before_buffer_transfer` — Running tests/message_transfer_contracts.rs (rust/target/debug/deps/message_transfer_contracts-fd98dcba1dd2e5f9)
44. `queued_key_delivery_survives_source_collection_and_keeps_real_permissions` — Running tests/message_transfer_contracts.rs (rust/target/debug/deps/message_transfer_contracts-fd98dcba1dd2e5f9)
45. `websocket_event_source_and_web_transport_match_chrome` — Running tests/network_transports.rs (rust/target/debug/deps/network_transports-de9fcf5dc187e165)
46. `overlong_guest_navigation_is_rejected_before_page_allocation_or_http` — Running tests/popup_host_contracts.rs (rust/target/debug/deps/popup_host_contracts-60c71c0ee5508890)
47. `target_reads_content_viewport_and_retirement_remain_independent` — Running tests/popup_host_contracts.rs (rust/target/debug/deps/popup_host_contracts-60c71c0ee5508890)
48. `missing_dom_signature_remains_an_error` — Running tests/quiescence_budget.rs (rust/target/debug/deps/quiescence_budget-19d683bb3fe2c063)
49. `dom_signature_exception_remains_an_error` — Running tests/quiescence_budget.rs (rust/target/debug/deps/quiescence_budget-19d683bb3fe2c063)
50. `native_document_request_and_redirect_referrer_policy_matches_chrome_headers` — Running tests/referrer_policy.rs (rust/target/debug/deps/referrer_policy-6788d792d196853e)
51. `frame_sandbox_origin_and_script_policy_use_native_context_state` — Running tests/render.rs (rust/target/debug/deps/render-df64c99e4b27f30c)
52. `render_tier_fetch_set_cookie_on_redirect_reaches_jar` — Running tests/render.rs (rust/target/debug/deps/render-df64c99e4b27f30c)
53. `page_fetch_uses_negotiated_hints_and_browser_owned_headers` — Running tests/request_context.rs (rust/target/debug/deps/request_context-564791d327d307bb)
54. `same_origin_frames_share_host_storage_and_keep_session_areas_separate` — Running tests/storage_contracts.rs (rust/target/debug/deps/storage_contracts-f0cb4e723909c510)
55. `storage_delivery_uses_private_intrinsics_and_receiving_area` — Running tests/storage_events.rs (rust/target/debug/deps/storage_events-e912c1ba4eb27f29)
56. `trusted_storage_delivery_does_not_read_polluted_object_prototypes` — Running tests/storage_events.rs (rust/target/debug/deps/storage_events-e912c1ba4eb27f29)
57. `storage_events_match_source_pinned_chrome` — Running tests/storage_events.rs (rust/target/debug/deps/storage_events-e912c1ba4eb27f29)
58. `storage_tasks_preserve_microtask_and_receiver_lifecycle` — Running tests/storage_events.rs (rust/target/debug/deps/storage_events-e912c1ba4eb27f29)
59. `native_storage_task_budget_failure_is_atomic_and_recovers_after_delivery` — Running tests/storage_events.rs (rust/target/debug/deps/storage_events-e912c1ba4eb27f29)
60. `accepted_response_notices_survive_inbox_overflow` — Running tests/storage_inbox.rs (rust/target/debug/deps/storage_inbox-1f26fe10fd30e0bb)
61. `response_clear_survives_a_full_multi_recipient_task_queue` — Running tests/storage_inbox.rs (rust/target/debug/deps/storage_inbox-1f26fe10fd30e0bb)
62. `storage_units_partitioning_and_access_match_source_pinned_chrome` — Running tests/storage_partition.rs (rust/target/debug/deps/storage_partition-61d4f6d355a178c6)
63. `storage_quota_and_receiver_edges_match_chrome` — Running tests/storage_quota.rs (rust/target/debug/deps/storage_quota-b520271db40f22d6)
64. `native_stream_transfers_match_independent_chrome_control` — Running tests/stream_transfers.rs (rust/target/debug/deps/stream_transfers-5f4c22ade31551fe)
65. `stream_cancel_context_matches_distinct_chrome_webidl_methods` — Running tests/stream_transfers.rs (rust/target/debug/deps/stream_transfers-5f4c22ade31551fe)
66. `trusted_transfer_steps_ignore_page_modified_port_and_stream_methods` — Running tests/stream_transfers.rs (rust/target/debug/deps/stream_transfers-5f4c22ade31551fe)
67. `source_pull_reentry_preserves_chrome_buffer_stream_port_transfer_phases` — Running tests/stream_transfers.rs (rust/target/debug/deps/stream_transfers-5f4c22ade31551fe)
68. `native_stream_transfer_adversarial_chrome_controls` — Running tests/stream_transfers.rs (rust/target/debug/deps/stream_transfers-5f4c22ade31551fe)
69. `native_stream_transfer_retirement_matches_chrome` — Running tests/stream_transfers.rs (rust/target/debug/deps/stream_transfers-5f4c22ade31551fe)
70. `enforced_script_src_blocks_string_timers_with_an_eval_violation` — Running tests/string_timers.rs (rust/target/debug/deps/string_timers-fb1f563414b6deda)
71. `report_only_script_src_reports_and_still_runs_string_timers` — Running tests/string_timers.rs (rust/target/debug/deps/string_timers-fb1f563414b6deda)
72. `string_timers_run_in_the_global_scope_when_eval_is_allowed` — Running tests/string_timers.rs (rust/target/debug/deps/string_timers-fb1f563414b6deda)
73. `serialization_and_conversion_errors_match_chrome` — Running tests/transfer_list_errors.rs (rust/target/debug/deps/transfer_list_errors-b15f3c28da4c9a68)
74. `observable_when_and_text_stream_match_chrome156` — Running tests/web_apis.rs (rust/target/debug/deps/web_apis-8e053a31615f974b)
75. `history_and_location_match_chrome156` — Running tests/window_interfaces.rs (rust/target/debug/deps/window_interfaces-26e372dfd8b52d88)
76. `shared_window_snapshots_use_fixed_byte_admission_and_release_after_delivery` — Running tests/window_message_budget.rs (rust/target/debug/deps/window_message_budget-904dc314061f6ab6)
77. `window_recursive_getters_obey_the_current_count_budget` — Running tests/window_message_budget.rs (rust/target/debug/deps/window_message_budget-904dc314061f6ab6)
78. `window_named_access_matches_chrome156` — Running tests/window_named_access.rs (rust/target/debug/deps/window_named_access-583d75f4b5370f0e)

### Rust crash-affected cases (13)

1. `live_layout_backend::recapture_tests::a_mutation_between_capture_and_layout_is_laid_out_fresh` — renderlib
2. `live_layout_backend::recapture_tests::a_source_removed_after_its_capture_is_reported` — renderlib
3. `live_layout_backend::recapture_tests::a_stale_capture_is_not_served_from_the_layout_cache` — renderlib
4. `live_layout_backend::recapture_tests::a_tree_that_keeps_changing_under_its_capture_is_reported` — renderlib
5. `live_layout_backend::startup_tests::cold_session_startup_keeps_current_thread_network_driven` — renderlib
6. `live_layout_budget_tests::errors_report_the_first_violation_in_composed_document_order` — renderlib
7. `live_layout_budget_tests::node_budget_counts_comments_doctypes_and_processing_instructions_before_filtering` — renderlib
8. `live_layout_budget_tests::node_budget_counts_text_and_elements_alike` — renderlib
9. `runtime::window_timer_task_tests::timer_nesting_is_per_window_and_survives_only_its_task_checkpoint` — renderlib
10. `blank_popup_inherits_frozen_base_without_changing_visible_location` — popup
11. `native_window_frame_accessors_survive_bootstrap_and_track_independent_children` — popup
12. `persistent_root_publishes_actual_capability_and_shared_actor_child` — popup
13. `producer_root_template_admits_only_native_contexts` — popup

### JavaScript failures (207)

1. `native inherited form resolution: about:blank popup uses its captured opener base` — `rust/playwright-shim/action-navigation-completion.test.mjs:188:1` (full-playwright-wrapper)
2. `frame automation identities never invoke authored WeakMap hooks` — `rust/playwright-shim/action-navigation-completion.test.mjs:362:1` (full-playwright-wrapper)
3. `a same-path replacement cannot forge an old frame identity through WeakMap hooks` — `rust/playwright-shim/action-navigation-completion.test.mjs:426:1` (full-playwright-wrapper)
4. `context animation presentation reaches native geometry and preserves WAAPI` — `rust/playwright-shim/animation-policy.test.mjs:13:1` (full-playwright-wrapper)
5. `browser newPage accepts an independent animation presentation choice` — `rust/playwright-shim/animation-policy.test.mjs:34:1` (full-playwright-wrapper)
6. `child console arguments own the child realm and preserve message ordering` — `rust/playwright-shim/console-events.test.mjs:58:1` (full-playwright-wrapper)
7. `draining console messages without listeners releases argument roots` — `rust/playwright-shim/console-events.test.mjs:80:1` (full-playwright-wrapper)
8. `Locator.evaluate uses indexed live nodes and persists page effects` — `rust/playwright-shim/eval-input.test.mjs:6:1` (full-playwright-wrapper)
9. `Locator.evaluateAll receives the complete matched collection and optional argument` — `rust/playwright-shim/eval-input.test.mjs:32:1` (full-playwright-wrapper)
10. `scrollIntoViewIfNeeded moves an offscreen target into the viewport` — `rust/playwright-shim/eval-input.test.mjs:68:1` (full-playwright-wrapper)
11. `hover enters newly hovered ancestors and preserves related targets on departure` — `rust/playwright-shim/eval-input.test.mjs:88:1` (full-playwright-wrapper)
12. `evaluate preserves special primitive values in nested results across page and frame realms` — `rust/playwright-shim/eval-input.test.mjs:109:1` (full-playwright-wrapper)
13. `keyboard tracks held modifiers and separates down/up events` — `rust/playwright-shim/eval-input.test.mjs:153:1` (full-playwright-wrapper)
14. `keyboard applies cancellable text editing and focused button activation defaults` — `rust/playwright-shim/eval-input.test.mjs:213:1` (full-playwright-wrapper)
15. `keyboard edits selections and Unicode text while respecting readonly controls` — `rust/playwright-shim/eval-input.test.mjs:260:1` (full-playwright-wrapper)
16. `frame evaluation awaits exposed host results without blocking the binding pump` — `rust/playwright-shim/eval-input.test.mjs:291:1` (full-playwright-wrapper)
17. `read-only typed collectors perform one evaluation and zero synchronization pulls` — `rust/playwright-shim/evaluate-sync.test.mjs:13:1` (full-playwright-wrapper)
18. `aliased DOM mutations, errors, cookies and child documents stay synchronized` — `rust/playwright-shim/evaluate-sync.test.mjs:37:1` (full-playwright-wrapper)
19. `handles preserve live identity, nested arguments, own properties and disposal` — `rust/playwright-shim/handles.test.mjs:11:1` (full-playwright-wrapper)
20. `handles enforce session and frame ownership and survive same-document mutations` — `rust/playwright-shim/handles.test.mjs:52:1` (full-playwright-wrapper)
21. `handle evaluations service host bindings while awaiting guest promises` — `rust/playwright-shim/handles.test.mjs:78:1` (full-playwright-wrapper)
22. `ElementHandle actions keep the retained node when microtasks reorder its siblings` — `rust/playwright-shim/handles.test.mjs:91:1` (full-playwright-wrapper)
23. `frame helper capabilities stay private when the guest replaces Function` — `rust/playwright-shim/handles.test.mjs:128:1` (full-playwright-wrapper)
24. `nested handle arguments use the target frame prototypes` — `rust/playwright-shim/handles.test.mjs:160:1` (full-playwright-wrapper)
25. `frame helper state ignores replaced guest intrinsics and inherited setters` — `rust/playwright-shim/handles.test.mjs:181:1` (full-playwright-wrapper)
26. `handle evaluation preserves sparse array argument lengths` — `rust/playwright-shim/handles.test.mjs:215:1` (full-playwright-wrapper)
27. `document replacement retires a paused route without poisoning the next request` — `rust/playwright-shim/host-routes.test.mjs:441:1` (full-playwright-wrapper)
28. `navigation deadlines bound a host route that never settles` — `rust/playwright-shim/host-routes.test.mjs:473:1` (full-playwright-wrapper)
29. `setContent on the retained native Document cannot reuse the earlier static seed` — `rust/playwright-shim/image-promotion.test.mjs:168:1` (full-playwright-wrapper)
30. `implicit Enter: first default submitter` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
31. `implicit Enter: disabled first default submitter` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
32. `implicit Enter: checkbox uses default submitter` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
33. `implicit Enter: radio uses default submitter` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
34. `implicit Enter: range uses default submitter` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
35. `implicit Enter: canceled keydown` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
36. `implicit Enter: invalid required input` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
37. `implicit Enter: submitter formnovalidate bypasses required input` — `rust/playwright-shim/implicit-submit-contract.test.mjs:56:3` (full-playwright-wrapper)
38. `keyboard completes independently and preserves eventual host value` — `rust/playwright-shim/keyboard-bindings.test.mjs:31:3` (full-playwright-wrapper)
39. `keyboard completes independently and preserves eventual host error` — `rust/playwright-shim/keyboard-bindings.test.mjs:31:3` (full-playwright-wrapper)
40. `held callbacks do not change key cancellation, modifiers, order or frame ownership` — `rust/playwright-shim/keyboard-bindings.test.mjs:85:1` (full-playwright-wrapper)
41. `background binding collection errors remain observable` — `rust/playwright-shim/keyboard-bindings.test.mjs:135:1` (full-playwright-wrapper)
42. `closing during unrelated host work does not forge callback completion` — `rust/playwright-shim/keyboard-bindings.test.mjs:152:1` (full-playwright-wrapper)
43. `keyboard emits Chromium legacy Enter and printable keypress fields in order` — `rust/playwright-shim/keyboard-keypress.test.mjs:16:1` (full-playwright-wrapper)
44. `legacy React keypress filtering admits Enter and printable characters` — `rust/playwright-shim/keyboard-keypress.test.mjs:38:1` (full-playwright-wrapper)
45. `keydown cancellation suppresses typing while preserving keyup` — `rust/playwright-shim/keyboard-keypress.test.mjs:58:3` (full-playwright-wrapper)
46. `keypress cancellation suppresses typing while preserving keyup` — `rust/playwright-shim/keyboard-keypress.test.mjs:58:3` (full-playwright-wrapper)
47. `command chords omit keypress and preserve select-all editing` — `rust/playwright-shim/keyboard-keypress.test.mjs:78:1` (full-playwright-wrapper)
48. `Shift+Enter preserves its printable legacy keypress` — `rust/playwright-shim/keyboard-keypress.test.mjs:96:1` (full-playwright-wrapper)
49. `typing preserves a live editor's character query until its timeout` — `rust/playwright-shim/keyboard-timers.test.mjs:5:1` (full-playwright-wrapper)
50. `live editor timers still run after their wall-clock deadline` — `rust/playwright-shim/keyboard-timers.test.mjs:30:1` (full-playwright-wrapper)
51. `page selector waits honor states, timeouts and later insertions` — `rust/playwright-shim/lifecycle.test.mjs:60:1` (full-playwright-wrapper)
52. `frame and nested-frame locator owners are their top-level Page` — `rust/playwright-shim/locator-page.test.mjs:41:1` (full-playwright-wrapper)
53. `the payroll date-helper owning-page keyboard primitive edits the selected field` — `rust/playwright-shim/locator-page.test.mjs:74:1` (full-playwright-wrapper)
54. `native workers match Chrome trusted storage tasks, UTF16, source exclusion and tab namespaces` — `rust/playwright-shim/native-peer-events.test.mjs:126:1` (full-playwright-wrapper)
55. `the addon reports its linked renderer engine without exposing diagnostics to guests` — `rust/playwright-shim/native-profile.test.mjs:24:1` (full-playwright-wrapper)
56. `Blob downloads preserve every byte in path, saveAs and streams` — `rust/playwright-shim/navigation.test.mjs:16:1` (full-playwright-wrapper)
57. `waitForURL keeps waiting for a later driver navigation within its timeout` — `rust/playwright-shim/navigation.test.mjs:214:1` (full-playwright-wrapper)
58. `waitForURL honors zero timeout and URL predicate/RegExp overloads` — `rust/playwright-shim/navigation.test.mjs:233:1` (full-playwright-wrapper)
59. `waitForURL expires at its deadline and stops when the page closes` — `rust/playwright-shim/navigation.test.mjs:253:1` (full-playwright-wrapper)
60. `a URL wait timeout leaves a pending host callback and sibling URL wait alive` — `rust/playwright-shim/navigation.test.mjs:268:1` (full-playwright-wrapper)
61. `locator press dispatches key events on its focused element` — `rust/playwright-shim/routing-regressions.test.mjs:13:1` (full-playwright-wrapper)
62. `XHR route abort emits an error and continue reaches the real response` — `rust/playwright-shim/routing-regressions.test.mjs:32:1` (full-playwright-wrapper)
63. `route fallback visits older handlers and unroute updates one live wrapper` — `rust/playwright-shim/routing-regressions.test.mjs:77:1` (full-playwright-wrapper)
64. `late native image completion cannot replace the new document's resident bytes` — `rust/playwright-shim/screenshot-document-assets.test.mjs:163:1` (full-playwright-wrapper)
65. `screenshot masks resolve scoped and indexed locators in the captured snapshot` — `rust/playwright-shim/screenshot-options.test.mjs:14:1` (full-playwright-wrapper)
66. `screenshot masks clip descendants and retain source indices after SVG` — `rust/playwright-shim/screenshot-options.test.mjs:44:1` (full-playwright-wrapper)
67. `screenshot masks and native capture share one snapshot across resource preparation` — `rust/playwright-shim/screenshot-options.test.mjs:72:1` (full-playwright-wrapper)
68. `full-page capture includes positioned overflow and viewport minimum` — `rust/playwright-shim/screenshot-options.test.mjs:96:1` (full-playwright-wrapper)
69. `omitBackground preserves transparent canvas and authored background pixels` — `rust/playwright-shim/screenshot-options.test.mjs:117:1` (full-playwright-wrapper)
70. `native screenshot device scale preserves CSS layout and scales clip pixels` — `rust/playwright-shim/screenshot-options.test.mjs:142:1` (full-playwright-wrapper)
71. `screenshot scale validates context/options and retains SVG CSS coordinates` — `rust/playwright-shim/screenshot-options.test.mjs:195:1` (full-playwright-wrapper)
72. `fractional DPR viewport dimensions and clipped CSS sizes match Playwright controls` — `rust/playwright-shim/screenshot-options.test.mjs:215:1` (full-playwright-wrapper)
73. `viewport screenshot selects contributing image bytes while preserving independent intrinsic sizes` — `rust/playwright-shim/screenshot-resources.test.mjs:9:1` (full-playwright-wrapper)
74. `resident image keys preserve URL strings without invoking object prototype setters` — `rust/playwright-shim/screenshot-resources.test.mjs:89:1` (full-playwright-wrapper)
75. `unchanged viewport captures reuse one prepared scene with identical pixels` — `rust/playwright-shim/screenshot-scene.test.mjs:10:1` (full-playwright-wrapper)
76. `scene reuse invalidates HTML, stylesheet, viewport and output-layout options and closes old state` — `rust/playwright-shim/screenshot-scene.test.mjs:38:1` (full-playwright-wrapper)
77. `owned scenes retain copied inputs and invalidate changed image bytes even within the same Buffer` — `rust/playwright-shim/screenshot-scene.test.mjs:65:1` (full-playwright-wrapper)
78. `setContent preserves the live realm and upgrades attached custom elements` — `rust/playwright-shim/set-content.test.mjs:16:1` (full-playwright-wrapper)
79. `setContent uses parser CSP resource ordering and load completion` — `rust/playwright-shim/set-content.test.mjs:104:1` (full-playwright-wrapper)
80. `setContent domcontentloaded returns before load resources and later completes` — `rust/playwright-shim/set-content.test.mjs:153:1` (full-playwright-wrapper)
81. `setContent preserves installed CSP and rejects load deadlines without replacing the realm` — `rust/playwright-shim/set-content.test.mjs:222:1` (full-playwright-wrapper)
82. `setContent clears old intrinsic metadata before same-URL image reloads` — `rust/playwright-shim/set-content.test.mjs:277:1` (full-playwright-wrapper)
83. `setContent retires old resources while activating replacement frames and resources` — `rust/playwright-shim/set-content.test.mjs:338:1` (full-playwright-wrapper)
84. `setContent activates imported srcdoc frames and retires replaced children` — `rust/playwright-shim/set-content.test.mjs:410:1` (full-playwright-wrapper)
85. `replacement parser preparation stays ordered beside custom-element dynamic scripts` — `rust/playwright-shim/set-content.test.mjs:435:1` (full-playwright-wrapper)
86. `toBeEmpty uses control values and descendant text, including live dirty values` — `rust/playwright-shim/small-behavior.test.mjs:5:1` (full-playwright-wrapper)
87. `isChecked validates native and ARIA types and reads live checked state` — `rust/playwright-shim/small-behavior.test.mjs:26:1` (full-playwright-wrapper)
88. `boundingBox returns live positioned geometry and null for hidden elements` — `rust/playwright-shim/small-behavior.test.mjs:50:1` (full-playwright-wrapper)
89. `exposeBinding reports the originating main frame` — `rust/playwright-shim/small-behavior.test.mjs:70:1` (full-playwright-wrapper)
90. `exposed functions settle guest promises with host values and errors` — `rust/playwright-shim/small-behavior.test.mjs:88:1` (full-playwright-wrapper)
91. `disabled reads and assertions honor MUI ARIA scope controls and live changes` — `rust/playwright-shim/small-behavior.test.mjs:130:1` (full-playwright-wrapper)
92. `uncaught live handler exceptions emit Error payloads` — `rust/playwright-shim/surface.test.mjs:24:1` (full-playwright-wrapper)
93. `Locator composition: count/first/last/nth/all` — `rust/playwright-shim/surface.test.mjs:199:1` (full-playwright-wrapper)
94. `Locator filter / and / or` — `rust/playwright-shim/surface.test.mjs:209:1` (full-playwright-wrapper)
95. `Locator text reads: textContent/innerText/innerHTML/allTextContents/allInnerTexts` — `rust/playwright-shim/surface.test.mjs:217:1` (full-playwright-wrapper)
96. `Locator accessors: attr/value/role/name/visible/checked/enabled/editable/empty/css` — `rust/playwright-shim/surface.test.mjs:227:1` (full-playwright-wrapper)
97. `Locator selectedValues + selectOption` — `rust/playwright-shim/surface.test.mjs:242:1` (full-playwright-wrapper)
98. `Locator actions: fill/clear/type/pressSequentially/check/uncheck/setChecked` — `rust/playwright-shim/surface.test.mjs:249:1` (full-playwright-wrapper)
99. `Locator.setInputFiles selects genuine Files through the input's FileList` — `rust/playwright-shim/surface.test.mjs:263:1` (full-playwright-wrapper)
100. `Locator no-op actions resolve quietly` — `rust/playwright-shim/surface.test.mjs:278:1` (full-playwright-wrapper)
101. `Locator.evaluate runs page JS for a CSS-backed locator` — `rust/playwright-shim/surface.test.mjs:289:1` (full-playwright-wrapper)
102. `Page getBy*: role/text/label/testid/placeholder/alt/title` — `rust/playwright-shim/surface.test.mjs:298:1` (full-playwright-wrapper)
103. `getByTestId honours a custom testIdAttribute` — `rust/playwright-shim/surface.test.mjs:313:1` (full-playwright-wrapper)
104. `LocatorAssertions: visibility / state matchers` — `rust/playwright-shim/surface.test.mjs:321:1` (full-playwright-wrapper)
105. `LocatorAssertions: text / value / count matchers` — `rust/playwright-shim/surface.test.mjs:333:1` (full-playwright-wrapper)
106. `LocatorAssertions: attribute / class / role / id / aria-name matchers` — `rust/playwright-shim/surface.test.mjs:344:1` (full-playwright-wrapper)
107. `LocatorAssertions: toHaveValues / toMatchAriaSnapshot` — `rust/playwright-shim/surface.test.mjs:355:1` (full-playwright-wrapper)
108. `LocatorAssertions: toHaveScreenshot throws` — `rust/playwright-shim/surface.test.mjs:362:1` (full-playwright-wrapper)
109. `Page selector shortcuts: innerText/innerHTML/textContent/getAttribute/inputValue/is*` — `rust/playwright-shim/surface.test.mjs:492:1` (full-playwright-wrapper)
110. `Page click submits a GET form to the server` — `rust/playwright-shim/surface.test.mjs:506:1` (full-playwright-wrapper)
111. `Page without iframes: mainFrame + frames() + frameLocator over host` — `rust/playwright-shim/surface.test.mjs:520:1` (full-playwright-wrapper)
112. `Page.frames / frame({name}) / frameLocator: iframe srcdoc` — `rust/playwright-shim/surface.test.mjs:531:1` (full-playwright-wrapper)
113. `frameLocator.locator().fill: iframe DOM reflects the write` — `rust/playwright-shim/surface.test.mjs:565:1` (full-playwright-wrapper)
114. `Page.frame by string name` — `rust/playwright-shim/surface.test.mjs:580:1` (full-playwright-wrapper)
115. `Page waiters resolve on the static DOM` — `rust/playwright-shim/surface.test.mjs:591:1` (full-playwright-wrapper)
116. `Page events: on/once/off load + close` — `rust/playwright-shim/surface.test.mjs:604:1` (full-playwright-wrapper)
117. `Page evaluate` — `rust/playwright-shim/surface.test.mjs:635:1` (full-playwright-wrapper)
118. `fixture { page } is injected and isolated` — `rust/playwright-shim/index.mjs:7081:5` (full-playwright-wrapper)
119. `test.extend injects custom fixtures` — `rust/playwright-shim/index.mjs:7081:5` (full-playwright-wrapper)
120. `page-override fixture receives + wraps the base page` — `rust/playwright-shim/index.mjs:7081:5` (full-playwright-wrapper)
121. `page.screenshot renders synthetic pixels` — `rust/playwright-shim/surface.test.mjs:838:1` (full-playwright-wrapper)
122. `honest throws: pixel / input / network-interception` — `rust/playwright-shim/surface.test.mjs:845:1` (full-playwright-wrapper)
123. `Page.evaluate returns typed values from a STATIC page too` — `rust/playwright-shim/surface.test.mjs:1009:1` (full-playwright-wrapper)
124. `locator.focus() + expect.toBeFocused() reflect document.activeElement in the live app` — `rust/playwright-shim/surface.test.mjs:1062:1` (full-playwright-wrapper)
125. `Locator.getByTestId scopes to the parent locator subtree` — `rust/playwright-shim/surface.test.mjs:1429:1` (full-playwright-wrapper)
126. `Locator.getByRole scopes to the parent locator subtree` — `rust/playwright-shim/surface.test.mjs:1443:1` (full-playwright-wrapper)
127. `successful locator assertions read each observed value once` — `rust/playwright-shim/surface.test.mjs:1837:1` (full-playwright-wrapper)
128. `visibility assertions request only the visibility predicate` — `rust/playwright-shim/surface.test.mjs:1885:1` (full-playwright-wrapper)
129. `getByTestId regex preserves flags, scope and repeated global matching` — `rust/playwright-shim/surface.test.mjs:1939:1` (full-playwright-wrapper)
130. `successive first last and nth select the existing singleton` — `rust/playwright-shim/surface.test.mjs:2043:1` (full-playwright-wrapper)
131. `selector counts avoid materializing match payloads` — `rust/playwright-shim/surface.test.mjs:2063:1` (full-playwright-wrapper)
132. `screenshot encodes JPEG type, inferred paths, and quality` — `rust/playwright-shim/surface.test.mjs:2080:1` (full-playwright-wrapper)
133. `Frame URLs resolve relative src before URL lookup` — `rust/playwright-shim/surface.test.mjs:2105:1` (full-playwright-wrapper)
134. `Frame registry preserves nested identity and detach events` — `rust/playwright-shim/surface.test.mjs:2115:1` (full-playwright-wrapper)
135. `FrameLocator.owner returns the indexed parent-scoped iframe locator` — `rust/playwright-shim/surface.test.mjs:2171:1` (full-playwright-wrapper)
136. `screenshot clip crops output without changing viewport layout` — `rust/playwright-shim/surface.test.mjs:2435:1` (full-playwright-wrapper)
137. `nested scope selectors evaluate against each matched parent` — `rust/playwright-shim/surface.test.mjs:2463:1` (full-playwright-wrapper)
138. `child reads honor indexed and filtered ancestors` — `rust/playwright-shim/surface.test.mjs:2479:1` (full-playwright-wrapper)
139. `fill rejects invalid number text before mutating the value` — `rust/playwright-shim/surface.test.mjs:2629:1` (full-playwright-wrapper)
140. `CSS and id assertions accept regex and retain negation` — `rust/playwright-shim/surface.test.mjs:2677:1` (full-playwright-wrapper)
141. `attribute expectations preserve absence and normalize existence options` — `rust/playwright-shim/surface.test.mjs:2695:1` (full-playwright-wrapper)
142. `boolean locator assertions honor expected-state options and negation` — `rust/playwright-shim/surface.test.mjs:2780:1` (full-playwright-wrapper)
143. `fill edits contenteditable text with input events and a collapsed caret` — `rust/playwright-shim/surface.test.mjs:2841:1` (full-playwright-wrapper)
144. `getBy parents retain their scope for CSS and getBy children` — `rust/playwright-shim/surface.test.mjs:2931:1` (full-playwright-wrapper)
145. `textContent reads preserve raw descendant DOM text` — `rust/playwright-shim/surface.test.mjs:3142:1` (full-playwright-wrapper)
146. `addStyleTag inserts content URL and path sheets into the live document` — `rust/playwright-shim/surface.test.mjs:3305:1` (full-playwright-wrapper)
147. `Context media preferences reach native matching and stay isolated` — `rust/playwright-shim/surface.test.mjs:3404:1` (full-playwright-wrapper)
148. `addScriptTag executes content URL and path scripts in the live realm` — `rust/playwright-shim/surface.test.mjs:3594:1` (full-playwright-wrapper)
149. `placeholder alt and title locators retain parent and indexed scope` — `rust/playwright-shim/surface.test.mjs:3627:1` (full-playwright-wrapper)
150. `role queries filter pressed state and accessibility-hidden ancestors` — `rust/playwright-shim/surface.test.mjs:3644:1` (full-playwright-wrapper)
151. `innerText reads rendered text and excludes hidden descendants` — `rust/playwright-shim/surface.test.mjs:3667:1` (full-playwright-wrapper)
152. `XPath engine prefixes and scoped parent locators retain their context` — `rust/playwright-shim/surface.test.mjs:4007:1` (full-playwright-wrapper)
153. `role locators resolve native headings table structure and image names` — `rust/playwright-shim/surface.test.mjs:4491:1` (full-playwright-wrapper)
154. `exact text label and role names survive static scoped and live resolution` — `rust/playwright-shim/surface.test.mjs:4512:1` (full-playwright-wrapper)
155. `accessible names resolve ordered ID references and associated native labels` — `rust/playwright-shim/surface.test.mjs:4539:1` (full-playwright-wrapper)
156. `static frame identity survives promotion to the live native registry` — `rust/playwright-shim/surface.test.mjs:4645:1` (full-playwright-wrapper)
157. `in-flight screenshot assets cannot repopulate a reloaded document cache` — `rust/playwright-shim/surface.test.mjs:4667:1` (full-playwright-wrapper)
158. `File paths infer upload MIME types and preserve explicit payload types` — `rust/playwright-shim/surface.test.mjs:4761:1` (full-playwright-wrapper)
159. `Single-file uploads reject invalid cardinality without replacing files` — `rust/playwright-shim/surface.test.mjs:4790:1` (full-playwright-wrapper)
160. `selectOption supports complete value, label, index and clearing selections` — `rust/playwright-shim/surface.test.mjs:4816:1` (full-playwright-wrapper)
161. `Class assertions compare complete token sets and ordered class lists` — `rust/playwright-shim/surface.test.mjs:4855:1` (full-playwright-wrapper)
162. `Single-target locator reads actions and negated assertions enforce strictness` — `rust/playwright-shim/surface.test.mjs:4884:1` (full-playwright-wrapper)
163. `Text array assertions compare ordered collections and subsequences` — `rust/playwright-shim/surface.test.mjs:4907:1` (full-playwright-wrapper)
164. `Locator unions and intersections honor indexed operands, order and live child scope` — `rust/playwright-shim/surface.test.mjs:4924:1` (full-playwright-wrapper)
165. `Extended visible CSS resolves the same live elements for reads and actions` — `rust/playwright-shim/surface.test.mjs:4947:1` (full-playwright-wrapper)
166. `Locator filters preserve relative predicates, stage order and live read action identity` — `rust/playwright-shim/surface.test.mjs:4987:1` (full-playwright-wrapper)
167. `Filtered locator plans retain nested scopes, indexed stages and frame identity` — `rust/playwright-shim/surface.test.mjs:5019:1` (full-playwright-wrapper)
168. `Page control shortcuts and selectedValues share the live control state after setContent` — `rust/playwright-shim/surface.test.mjs:5061:1` (full-playwright-wrapper)
169. `locator count + text` — `rust/playwright-shim/test.mjs:25:1` (full-playwright-wrapper)
170. `getByRole / getByText / getByLabel` — `rust/playwright-shim/test.mjs:34:1` (full-playwright-wrapper)
171. `expect matchers + .not` — `rust/playwright-shim/test.mjs:43:1` (full-playwright-wrapper)
172. `page views` — `rust/playwright-shim/test.mjs:53:1` (full-playwright-wrapper)
173. `frame locator interactions do not crash` — `rust/playwright-shim/test.mjs:65:1` (full-playwright-wrapper)
174. `child fragment navigation clears a hash without replacing its document` — `rust/playwright-shim/test.mjs:88:1` (full-playwright-wrapper)
175. `evaluate + render` — `rust/playwright-shim/test.mjs:125:1` (full-playwright-wrapper)
176. `actions: fill / check / selectOption mutate the DOM` — `rust/playwright-shim/test.mjs:139:1` (full-playwright-wrapper)
177. `click follows a link to a new page` — `rust/playwright-shim/test.mjs:152:1` (full-playwright-wrapper)
178. `click submits a GET form` — `rust/playwright-shim/test.mjs:169:1` (full-playwright-wrapper)
179. `locator accessors` — `rust/playwright-shim/test.mjs:189:1` (full-playwright-wrapper)
180. `expect matcher surface` — `rust/playwright-shim/test.mjs:202:1` (full-playwright-wrapper)
181. `locator-scoped actions` — `rust/playwright-shim/test.mjs:216:1` (full-playwright-wrapper)
182. `page.screenshot renders a synthetic PNG` — `rust/playwright-shim/test.mjs:227:1` (full-playwright-wrapper)
183. `honest-throws for pixel APIs` — `rust/playwright-shim/test.mjs:238:1` (full-playwright-wrapper)
184. `mock SPA hydrates through the shim` — `rust/playwright-shim/test.mjs:248:1` (full-playwright-wrapper)
185. `getByTestId resolves the configured attribute` — `rust/playwright-shim/test.mjs:291:1` (full-playwright-wrapper)
186. `locator composition` — `rust/playwright-shim/test.mjs:299:1` (full-playwright-wrapper)
187. `nested locator scopes via CSS concat` — `rust/playwright-shim/test.mjs:308:1` (full-playwright-wrapper)
188. `function-form page.evaluate with an argument` — `rust/playwright-shim/test.mjs:315:1` (full-playwright-wrapper)
189. `node_snapshot batches expect(locator) chain in one crossing` — `rust/playwright-shim/test.mjs:322:1` (full-playwright-wrapper)
190. `waiters resolve on the static DOM` — `rust/playwright-shim/test.mjs:365:1` (full-playwright-wrapper)
191. `event registry: on('load') fires` — `rust/playwright-shim/test.mjs:394:1` (full-playwright-wrapper)
192. `honest throws for network/input/pixel` — `rust/playwright-shim/test.mjs:402:1` (full-playwright-wrapper)
193. `@playwright/test fixture: { page } is injected` — `rust/playwright-shim/index.mjs:7081:5` (full-playwright-wrapper)
194. `locators + expect on the native engine` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
195. `native dialogs suspend the handler and use host accept or dismiss results` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
196. `no listener automatically dismisses dialogs without suppressing later script` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
197. `accepting a prompt uses empty replacement text when omitted or explicitly empty` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
198. `dialogs during initial document script execution reach the host before continuation` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
199. `closing a page releases a pending native dialog without an actor command deadlock` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
200. `a native dialog cannot be handled twice` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
201. `dialog arguments use Web IDL strings and preserve conversion exceptions` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
202. `sandboxed frames without allow-modals auto-dismiss without a host event` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
203. `allow-modals frames deliver dialogs to their actual Page` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
204. `dialog messages, defaults, and accepted prompt strings preserve original UTF-16` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
205. `pixel/real-browser APIs reject honestly instead of silently passing` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
206. `page.screenshot produces a synthetic PNG without a browser` — `rust/playwright-shim/index.mjs:7081:5` (full-e2e-unreached)
207. `native trace preserves trusted handler location activation before automatic retries` — `harness/detection/native.test.mjs:203:1` (full-detection)

## Integrated c953 regression discovery — all residuals remain in scope

This section records valid localhost-authorized discovery on frozen `c953e15c`. The earlier sandbox-denied run is invalid and receives no product pass/fail credit. Isolated fixed proofs do not replace a fresh integrated rerun. Original baseline cases above are never removed.

### Rust assertion failures observed in the discovery shard

1. `failed_later_frame_navigation_can_recover_on_the_same_host` — valid frozen discovery failure; see default-rust-regressions results/logs.
2. `failed_replacement_initialization_releases_roots_and_same_host_can_retry` — valid frozen discovery failure; see default-rust-regressions results/logs.
3. `external_resource_reports_and_original_utf16_samples_match_chrome` — valid frozen discovery failure; see default-rust-regressions results/logs.
4. `dom_signature_exception_remains_an_error` — valid frozen discovery failure; see default-rust-regressions results/logs.
5. `native_storage_task_budget_failure_is_atomic_and_recovers_after_delivery` — valid frozen discovery failure; see default-rust-regressions results/logs.
6. `storage_delivery_uses_private_intrinsics_and_receiving_area` — valid frozen discovery failure; see default-rust-regressions results/logs.
7. `storage_events_match_source_pinned_chrome` — valid frozen discovery failure; see default-rust-regressions results/logs.
8. `storage_tasks_preserve_microtask_and_receiver_lifecycle` — valid frozen discovery failure; see default-rust-regressions results/logs.
9. `trusted_storage_delivery_does_not_read_polluted_object_prototypes` — valid frozen discovery failure; see default-rust-regressions results/logs.
10. `accepted_response_notices_survive_inbox_overflow` — valid frozen discovery failure; see default-rust-regressions results/logs.
11. `response_clear_survives_a_full_multi_recipient_task_queue` — valid frozen discovery failure; see default-rust-regressions results/logs.
12. `unrelated_origin_notices_do_not_stall_matching_delivery` — valid frozen discovery failure; see default-rust-regressions results/logs.
13. `storage_units_partitioning_and_access_match_source_pinned_chrome` — valid frozen discovery failure; see default-rust-regressions results/logs.
14. `native_stream_transfer_retirement_matches_chrome` — valid frozen discovery failure; see default-rust-regressions results/logs.
15. `window_named_access_matches_chrome156` — valid frozen discovery failure; see default-rust-regressions results/logs.
16. `borrowed_cookie_getter_uses_the_frame_request_path` — valid frozen discovery failure; see default-rust-regressions results/logs.

### Timeout and crash obligations

- Quiescence target timed out after its first explicit DOM-signature failure; the missing-helper case timed out and the remaining budget case initially had no outcome. All three original cases remain mandatory. Candidate `c2380ba0` integrated as `a2f3896d`: fresh focused 10 pass, strict pass, single-seam OFF stalls and byte-exact restored original 3 pass. Full updated regression rerun still required.
- Four recapture cases abort in frozen Render lib: `a_mutation_between_capture_and_layout_is_laid_out_fresh`, `a_source_removed_after_its_capture_is_reported`, `a_stale_capture_is_not_served_from_the_layout_cache`, `a_tree_that_keeps_changing_under_its_capture_is_reported`. Test-hook owning-context fix `93b3ab8` integrated as `962152c0`; all four pass focused. Crash outcomes remain recorded, not erased.
- Crash-affected cases were recovered individually: baseline 91 outcomes are 72 pass / 14 fail / 1 timeout / 4 crashes, zero unreported. Required default 408 outcomes are 389 pass / 14 fail / 1 timeout / 4 crashes. Whole-bin positive totals are a different denominator.

### JavaScript discovery failures (18)

1. `initial module top-level awaits can complete host fulfillment and route.fetch` — `rust/playwright-shim/host-routes.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-86.log`.
2. `setContent on the retained native Document cannot reuse the earlier static seed` — `rust/playwright-shim/image-promotion.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-87.log`.
3. `a pending peer Promise receives a storage task without another peer RPC` — `rust/playwright-shim/native-peer-events.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-94.log`.
4. `shim page.evaluate awaits a storage Promise from another page` — `rust/playwright-shim/native-peer-events.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-94.log`.
5. `late native image completion cannot replace the new document's resident bytes` — `rust/playwright-shim/screenshot-document-assets.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-98.log`.
6. `resident image keys preserve URL strings without invoking object prototype setters` — `rust/playwright-shim/screenshot-resources.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-100.log`.
7. `setContent uses parser CSP resource ordering and load completion (#386)` — `rust/playwright-shim/set-content.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-102.log`.
8. `setContent preserves installed CSP and rejects load deadlines without replacing the realm (#386)` — `rust/playwright-shim/set-content.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-102.log`.
9. `setContent clears old intrinsic metadata before same-URL image reloads (#386)` — `rust/playwright-shim/set-content.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-102.log`.
10. `setContent retires old resources while activating replacement frames and resources (#386)` — `rust/playwright-shim/set-content.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-102.log`.
11. `replacement parser preparation stays ordered beside custom-element dynamic scripts (#386)` — `rust/playwright-shim/set-content.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-102.log`.
12. `addInitScript applies to every child frame (src, srcdoc, nested, dynamic, blob)` — `rust/playwright-shim/surface.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-104.log`.
13. `page.exposeBinding fires when called from inside an iframe` — `rust/playwright-shim/surface.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-104.log`.
14. `Frame URLs resolve relative src before URL lookup (#448)` — `rust/playwright-shim/surface.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-104.log`.
15. `Live frame registry tracks nested frames and actual navigation URLs (#447)` — `rust/playwright-shim/surface.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-104.log`.
16. `Frame reads reflect source documents and later live mutations (#445)` — `rust/playwright-shim/surface.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-104.log`.
17. `in-flight screenshot assets cannot repopulate a reloaded document cache (#489)` — `rust/playwright-shim/surface.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-104.log`.
18. `a navigation deadline does not close or replace its sibling popup` — `rust/playwright-shim/navigation-deadline.test.mjs`; retained log `/Users/grzegorz/github-flux/turbo-surf/.recovery/final-public-c953/js-regressions/job-115.log`.

### Active fix families and additional candidate controls

- [x] Frame rollback hook ownership: original two failures fixed by `eed5fcbb`, integrated `0336d449`; ON/OFF/ON reproduces the missing-receipt failures and restores both passes. Integrated rerun pending.
- [x] CSP report oracle: obsolete filter removed, full unchanged 19-row oracle passes; `f0792961` integrated `d0c36469`. Integrated rerun pending.
- [x] Recapture native callback crash fixture: owning-context hook corrected; integrated `962152c0`, focused four pass. Full library rerun pending.
- [x] Owned navigation timeout normalization: `05e0bb5` integrated `f3de2a22`; sibling/deadline/security controls pass. Separate initial module timeout remains open.
- [x] Quiescence selected timer progress/bounded churn: integrated `a2f3896d` with fresh-library and inverse proof. Earlier stale-library candidate run is invalid evidence, not a second code failure.
- [ ] Storage recipient delivery: original events 5, inbox 3 and new selected-peer/FIFO controls 3 pass isolated; original partition case still fails valid authorized run with Owner entry failed. Both JS storage failures and partition require proof; no global peer drain or FIFO waiver.
- [ ] Weak Promise-parent lifetime: fulfilled parents collected by GC lose known terminal state; original WindowNamed and profile duplicate fail. Actual pending/unseen parents must continue failing closed. Tests-first terminal-state fix and inverse pending.
- [ ] Iframe native fetch registration lacks captured parent ownership lease: original borrowed-cookie and related JS unknown-ownership failures; narrow private-parent boundary fix awaiting proof. No generic entered-context fallback.
- [ ] Retained stream retirement: original retirement oracle and Chrome-correct borrowed-method control pass isolated. Negative test requires actual sink-start barrier. GC control observed child alive before underlying close fired; genuine close barrier and same-budget GC test required before classifying/fixing a post-completion leak. Guard weakening is prohibited.
- [ ] Initial module host fulfillment / route.fetch timeout: actual callback hits zero and only module-page HTTP observed; selected-context embedder handoff fix under review. Actor-wide pending checks must not abandon Root work because an unrelated peer has a binding.

Evidence: `.recovery/final-public-c953/default-rust-regressions/results.json`, `default-required-case-coverage-restored.json`, `valid-js-failures.jsonl`, individual job logs, and isolated fix receipts. Every new candidate failure stays in this section and the detailed monotonic manifests until passing proof and integrated rerun. Full Rust/JS suites and coverage remain blocked.

## Frozen deb94 discovery and subsequent repairs

The complete frozen `deb94a60` campaign covered 843 required IDs exactly once: **833 pass, 10 fail, zero missing, required skips or crashes**. All 91 original Rust/crash obligations passed. The sealed receipt is `.recovery/final-known-next/FINAL-DISCOVERY-RECEIPT.json` (SHA `b7528198b3fd3bbd099985689d593ab34009fa2ec33709d5c90577a27b79998a`). These results are retained and are not replaced by isolated candidate results.

- Nine failures were the retained-document image-promotion, late image completion, resident-key, five set-content cases, and in-flight screenshot reload-cache case listed above. Logs remain in `.recovery/final-known-next/js-regressions/job-{87,98,100,102,104}.log`. Native diagnostics proved an unleased `op_load_image` from the mutation resource consumer. The isolated document-leased consumer repairs all nine original assertions; the first candidate's five complete files also pass with the exact official runner. A later passive-retirement correction still requires the complete five-file cohort on the final dependency graph.
- The tenth failure was `both collectors use a fixed Enter window for slow forms and delayed retries` in `harness/detection/navigation-observation.test.mjs`: a genuine terminal response arrived before the fixed deadline, but its committed document was still parsing, so its inline clock had not executed. Deterministic native tests prove both early complete and late parsing/pending frontiers without renewing the observation window. Test-only correction `6601e471`, integrated locally as `c15f2505`, preserves physical 600ms timer/wire checks and distinguishes authentic partial snapshots from executed-clock evidence. The amended full oracle and negative controls pass, as do both native frontier controls. The historical timing attribution remains an inference; no runtime execution is fabricated.
- New required control `resource_consumers_reject_fake_receivers_and_retired_documents_and_use_adopted_owner` first exposed an actual candidate regression: retirement's passive mutation consumer was incorrectly given a live-document lease. The isolated native retirement seam fixes that boundary; generic retired-document guards remain intact.
- The same control then exposed a producer binding defect: a child-created image adopted into Root loads the correct Root URL, but Root's `naturalWidth` getter returns 3 while the child getter returns 0. Exact Chrome returns 3 through both getters. Producer commit `625472a0` uses private per-image, creator-realm state and decode services. Four new controls and five existing image controls pass; a storage-only inverse reproduces the state split, and exact restored controls and strict checks pass. Public producer adoption and the unchanged Surf authority assertion are still pending.
- A candidate surface process without the official `--test-force-exit` flag was stopped after idle completion. This was a **runner-mode mismatch**, not a proven product regression. All raw stop/sample evidence remains in `.recovery/final-known-next/CANDIDATE-CLEANUP-OPEN.json`; the exact official argv subsequently exits cleanly with 204 passes and one intentional skip. No cleanup workaround was introduced.

Candidate authority phases remain hash-pinned in `.recovery/final-known-next/CANDIDATE-AUTHORITY-FAIL.json`. The monotonic V14 inventory contains **851 required IDs, 853 audit IDs and 144 jobs**, including four producer controls. It has no final integrated pass credit. The staged Surf consumer uses explicit private producer paths and is not public dependency proof. A fresh final public-graph regression run must pass every required case before full Rust/JS suites; coverage follows only after those suites pass.

## Public 551cb98e regression run — three Rust failures

The current public-graph discovery completed 69 default Rust jobs with **855 passed, 3 failed**, zero reported ignored tests or crashes. These whole-bin totals are distinct from the required-case count. All three failures remain in scope; no passing earlier epoch substitutes for a repaired integrated rerun.

| Status | Test | Observed failure | Owner / proof |
| --- | --- | --- | --- |
| Fixed on be74 | `isolation_policy::csp_task_capacity_is_bounded_and_recovers_after_dispatch` | Original capacity limit triggers correctly; subsequent recovery fails with `Owner entry failed` at line 1220. Root cause under native-only diagnosis; do not change the 4096 limit or assertions. | Surf async owner; `.recovery/final-known-v12/early-ready-regressions/job-23.log` (32 pass / 1 fail). |
| Fixed on be74 | `fetch_streams::timeout_abort_listener_survives_collection_until_delivery` | Expected timeout/abort delivery after collection is false at line 266. Timer/dispatch ownership diagnosis pending; no GC or time-budget waiver. | Native capability helper; `.recovery/final-known-v12/early-ready-regressions/job-14.log` (4 pass / 1 fail). |
| Fixed on be74 | `stream_transfers::native_stream_transfer_retirement_matches_chrome` | `Native async work has unclassified ownership` at line 99. Investigate this exact final-graph case, preserving the earlier retirement and security controls. | Stream owner; `.recovery/final-known-v12/remaining-default-regressions/job-37.log` (8 pass / 1 fail). |

The fetch-timeout failure was discovered as an adjacent test outside the prior 851 required IDs. Append it monotonically to the required manifest; it is not excluded merely because the old manifest omitted it. CSP capacity and stream retirement are already required. The corrected manifest therefore has 852 executable required IDs, with no duplicate stream entry. Preserve exact names, source/artifact hashes and original logs. Full Rust/JS suites and coverage remain blocked until every known required regression passes on one updated integrated build.

This ledger update is prepared in an isolated documentation worktree while the root's tracked `551cb98e` bytes remain frozen for active regression discovery. Integrate the update at the discovery boundary, then bind the next build to its new commit.

### Validated repairs after the sealed 551 discovery

The preceding failures remain historical evidence. CSP recovery is repaired by `cfeac1d7`: interrupted native owner entry no longer records a sticky admission failure while the VM is terminating. All 38 focused controls pass; reverting only the post-entry termination guard reproduces the original recovery error.

Fetch timeout ordering and lifetime are repaired by `155e527a`. Only selected documents' native timeouts prevent virtual timer acceleration; the official real-time backend remains intact. Its forwarding callback now belongs to the private infrastructure realm, so retired documents are collectible. All 13 focused controls pass, including the unchanged original timeout/GC case and the original retired-timeout GC assertion. Independent gate-only and forwarding-only inverses reproduce their respective ordering and collection failures; exact restoration passes.

Stream retirement is repaired by `da243375`. A deterministic pending-parent/timer-race control reproduced premature unknown-ownership rejection. Explicit completion waits can now make independently authenticated progress while a same-root collected-parent reaction remains uncertain; direct orphan waits, peer timers, unknown operations and inconsistent ownership still fail closed. The combined 29-case cohort passes, including the original stream target and CSP/fetch recovery controls; the mode-only inverse reproduces the race failure.

These three repairs have independent semantic reviews, strict checks and focused causal proofs. They do not retroactively turn the sealed failed run green. The next additive V18 inventory contains **877 executable required cases, 879 audit entries and 146 jobs**. Run all required cases on its new frozen source/public dependency build; only after 100% success and receipt review may full Rust/JS suites run, followed by coverage.

## Isolated asynchronous classic-script lifecycle audit (be74)

`asynchronous_classic_scripts_do_not_block_dcl_but_delay_load_with_native_events` remains open. The frozen full Rust run fails during `PageSession::open` with native async ownership unclassified, before authored order/event assertions. Its genuine DOMContentLoaded callback fetches `/ready` to release the held asynchronous script. Source audit identifies a missing ownership-entry candidate in native lifecycle dispatch; native registration metadata must establish causality before any production fix. Original assertions, golden fixture, and budgets remain unchanged. Shared runtime composition belongs to the lifecycle resource owner.

## Isolated actor non-pumping read audit (be74)

The sync and async `actor ... read enters runtime without pumping` controls fail because their child process exits with unclassified native async ownership. The child opens a real srcdoc frame before either read, so the failing phase is not yet established. A host-only marker copy preserves all actual calls, read assertions, pending-response handshake, and the original 15s child cap. No read behavior or fixture expectation is changed; the release source remains frozen.

Actor read phase refinement: both original reads return correct child text and leave timers/response consumers pending. Only the final await fails. Native metadata identifies the first read's sole fetch as unleased, because real target aliases use `read_target_engine_active`, whose direct engine call lacked the ownership wrapper already present on the primary path. A reviewed one-call wrapper proposal preserves all non-pumping semantics; controlled ON proof remains pending.

Actor read controlled acceptance: the one-call actual-target ownership wrapper passes both unchanged original sync/async controls and twelve held-IO read-queue/native guest-owner controls. Reads still leave timers and the response consumer pending, then the final await receives the actual response once. Render/NAPI all-target strict Clippy passes with zero warnings. Native recorder sources were fully removed before ON proof. The separate lifecycle dispatch fix passes its owner’s 25-case cohort; this actor component does not duplicate that runtime change. These are modular proofs, not a claim that the integrated full release gates passed.

## Full-suite discovery on frozen be74f8c7

All **877 required regressions across 146 jobs passed** on this source and public DOM `06c6f204`. Independent receipt review authorized full Rust/JS testing. The regression receipt remains sealed at `.recovery/final-known-v18/ALL146-REGRESSION-RECEIPT.json` (SHA `d1e5b02b1094b19025ef83f6ac28bb89ee8b98bda24fdcf3248b89fa040dcfa8`). Full-suite failures below do not erase that narrower result or count as successful full-suite completion.

| Status | Exact full-suite failure | Evidence and current diagnosis |
| --- | --- | --- |
| Fix merged; fresh rerun pending | `abort_signal_any::abort_signal_any_and_timeout_match_chrome156` | `tests/abort_signal_any.rs:88`: Chrome divergence `/timeoutEvents/2/1 = false`, meaning `combined.reason === timed.reason` is false. The timeout's infrastructure-owned raw reason is independently translated by each frontend, losing shared reason identity. Any repair must preserve source/dependent identity, guest error realms and retired-document collection without an infrastructure-to-guest strong root. Log: `.recovery/final-known-v18/full-gates/failure-abort_signal_any.log`. |
| Fix merged; fresh rerun pending | `geolocation::pending_watch_wakes_when_host_coordinates_change_without_polling_timers` | `tests/geolocation.rs:226`: “host configuration must wake the pending native operation”; full binary reports 9 pass / 1 fail. The scoped drain bypasses the legacy emulation-change notification subscription, so host changes do not wake the pending watcher. A narrow notification-only carryforward is source-reviewed; original wake-counter and peer-isolation runtime proofs remain required. Log: `.recovery/final-known-v18/full-gates/failure-geolocation.log`. |
| Fix merged; fresh rerun pending | `layout_query_cache::activating_an_animation_invalidates_a_previous_strict_certificate` | `tests/layout_query_cache.rs:71`: initial `PageSession::open` reports `hydration_diagnostics().budget_exceeded`, before certificate assertions; original 5000ms options and animation fixture remain unchanged. Log: `.recovery/final-known-v18/full-gates/failure-layout_query_cache.log` (3 pass / 2 fail). |
| Fix merged; fresh rerun pending | `layout_query_cache::query_relaxed_animation_admission_is_not_shared_with_other_reads` | Same initial budget-exceeded assertion at line 71, before cache-admission assertions. Investigate animation hydration/quiescence classification; no budget or assertion waiver. Same layout log. |
| Fix merged; fresh rerun pending | `lifecycle_resources::asynchronous_classic_scripts_do_not_block_dcl_but_delay_load_with_native_events` | `tests/lifecycle_resources.rs:345`: `PageSession::open` returns `Native async work has unclassified ownership`, before event/oracle assertions. Log: `.recovery/final-known-v18/full-gates/failure-lifecycle_resources.log` (2 pass / 2 fail). |
| Fix merged; fresh rerun pending | `lifecycle_resources::session_runs_deferred_scripts_at_interactive_and_waits_for_decoded_resources_after_dcl` | Same unknown-ownership error during open at line 123, before deferred-script/resource ordering assertions. Preserve the source-pinned probe and lifecycle limits; diagnose native operation/parent ownership. Same lifecycle log. |
| Fix merged; fresh rerun pending | `actor sync read enters runtime without pumping` | `rust/playwright-shim/actor-runtime.test.mjs:52`: the child exits 1 with `Native async work has unclassified ownership`. Host-only phase markers subsequently prove open and both no-pump reads succeed; failure is only the final `liveEvalEngineExpression('pendingRead')` after the response completes. Do not misclassify this as a failed read-entry/no-pumping assertion. Log: `.recovery/final-known-v18/full-gates/failure-js-actor-sync.log`. |
| Fix merged; fresh rerun pending | `actor async read enters runtime without pumping` | Same child-exit/unknown-ownership failure and confirmed final asynchronous completion phase; both original read assertions pass. Log: `.recovery/final-known-v18/full-gates/failure-js-actor-async.log`. |
| Fix merged; fresh rerun pending | `unimplemented compression names expose no usable constructor` | `rust/playwright-shim/e2e/private-infrastructure.spec.mjs:342` expects `CompressionStream` and `DecompressionStream` to be unconstructible for supported `gzip`; both are implemented by the current checksum-pinned official backend. Preserve this failed selector/ID and map it explicitly to the reviewed stronger replacement, `compression constructors roundtrip bytes without exposing the private realm`, which tests real gzip bytes and guest-owned streams/Promise/buffer/error constructor chains. Log: `.recovery/final-known-v18/full-gates/failure-js-compression-constructor.log`. |

The private additive V23 inventory preserves all prior required cases and adds all nine previously unselected failures: **886 executable required cases, 888 audit entries and 152 jobs**, before further accepted positive controls are appended. Full Rust completed 2003 pass / 6 fail, with 7 explicit existing ignores receiving no pass credit. All 177 Node files completed 1054 pass / 3 fail / 3 intentional skips / zero cancellations; both offline Python helper groups passed. The failed full-suite receipts are sealed and independently audited. No coverage run may start until the full Rust/JS gates pass on the final repaired source.

Validated post-discovery repairs have been additively integrated (the original private proof commits remain referenced here): geolocation `c8fea5a1` restores only the legacy host-change notification subscription (exact original counter-failure baseline, 21 unique ON controls and strict checks pass); lifecycle `f8e307fe` enters the actual document lease for native lifecycle dispatch (both exact UNKNOWN failures and 25 controls pass); layout `bd3c7c40` uses two bounded post-load turns without changing 5000ms budgets, animation policy or certificates (both failures and all 25 controls pass). Compression correction `e2b48c17` changes only the stale test contract: the stronger case and full seven-case privacy file pass on the unchanged native addon. Actor repair `b61d0782` passes both unchanged original read controls plus twelve adjacent controls and strict checks. Abort reason repair `34303d66` passes seventeen original/new identity and lifetime controls; independent reason-conversion and retired-owner-guard inverses reproduce their exact original and lifetime failures, and exact restoration passes. Preserve all historical failures above; isolated success is not final integrated full-suite credit.

Current integrated fix mapping: abort reason `34303d66`, geolocation `0bd2c1a8`, layout `2aa46d2f`, lifecycle `3bc15108`, actor reads `b61d0782`, compression test contract `3aa9ab57`. All nine recorded full-suite failures have reviewed repairs and focused proofs. A new fresh integrated build must execute all nine failure selectors (with the explicit compression rename) and every expanded required regression before the full Rust/JS suites are rerun. No prior-source proof substitutes for this run; coverage remains last.


## Nested timer ordering discovered by the full 145d suite

## Full workspace discovery: nested timer tail order

- Source: `145d15b2596dfdbf688974457d432dc7f4a6b27e` (four public DOM pins `06c6f204`, registry V8 `150.4.2`, Deno `.4`).
- Case: `turbo-surf-render::nested_timer_order::nested_timers_queue_behind_due_siblings_in_chrome_order`.
- Full run: session `47235`; target finished **0 passed / 1 failed / 0 ignored**, 3.89 s.
- Assertion: `rust/crates/turbo-surf-render/tests/nested_timer_order.rs:24`.
- Exact discrepancy: actual tail `n7,p30,n8`; unchanged pinned control expects `n7,n8,p30`. All earlier events match.
- Evidence: `.recovery/final-fixed-next/full-gates/failure-nested_timer_order.log`; full raw log `rust-workspace.log` in the same directory.
- Immutable baseline: `.recovery/final-fixed-next/artifacts/full145d-failed__nested_timer_order`, SHA-256 `2c50a2fa53ec9d8cd749785eab416656436058b3e5bab86f538f7f542367a217`; strict signature and independent CoW provenance in `failed-nested-timer-artifact.json`.
- Status: **OPEN**, root cause unproven; isolated source owner `locator_count`. No golden, ordering, timeout, nesting-clamp or budget waiver. No fix or passing future-graph proof claimed.
- Inventory: V26 preserves all prior 947 IDs and adds this actual failure: **948 eligible / 950 audit / 161 jobs**, independent additive source GO `e77aab64`.
- The prior 947-case regression PASS receipt remains sealed and is not rewritten. Current full run continues without fail-fast; coverage remains blocked.


Validated repair: `22bf0c1e` anchors clamped nested virtual timers to the running task deadline. The unchanged original Chrome order and deterministic host-pause control pass, together with 25 timer adjacency controls; the exact clamp-only baseline reproduces the same `n7,p30,n8` tail, and restored ON passes. RealTime semantics, nesting threshold, 4 ms clamp, FIFO order, and the original 5000 ms budget remain unchanged. Independent semantic review accepted the repair. The full 145d Rust run completed with 2009 pass / 1 fail / 7 explicit no-pass ignores; full JS completed with 1057 pass / 0 fail / 3 intentional no-pass skips. Those receipts remain sealed. Fresh integrated execution of all 965 required cases and both full suites is pending; prior focused proofs do not count as that execution.


## Full ee42 timer precision and rejection discovery

The full `ee42f80d` Rust run completed with 2009 pass, 2 failures, and 7 explicit no-pass ignores; its full JS run completed with 1057 pass, zero failures, and 3 intentional no-pass skips across 177 files, plus both offline Python helpers. Combined receipt `20353fe503a76606384a134f3d28d37c69c960060f2e8f8dfa17eda2d67ff5fb` is independently sealed as failed, not green.

- `runtime::window_timer_task_tests::timer_nesting_is_per_window_and_survives_only_its_task_checkpoint`: the first measured clamp sample was 3.8 ms against a 3.9 ms sampling threshold. At the user's explicit direction, `bf3d37aa` rounds samples to whole milliseconds before checking the unchanged nominal 4 ms clamp. Production timer code and ordering checks are unchanged. The existing unit and 27 related controls pass; independent review and strict/format/complexity checks pass. No new test ID was added. Original failure evidence: `.recovery/final-timer-next/full-gates/TIMER-NESTING-REGRESSION.md`.
- `unhandled_rejection::promise_rejection_events_match_chrome156`: OPEN. Native diagnostic reproduced 21 actual rows versus 22 expected; the top-level rejection for reason 42 arrived after constructor rows, and its attribute-handler row was absent after the fixture cleared that handler. Earlier stress-200 and frame rows match. Task ordering remains under investigation; no oracle, timer budget, or expectation has been changed. Evidence: `.recovery/final-timer-next/full-gates/failure-unhandled-rejection.log`; preserved failed binary SHA `ac103e942281528bf8ffd9bb4a9c4d54d21da7a34fb76a14cefa5f0d7da53475`. Both selectors already belong to the required regression inventory.


## Bugs discovered by meaningful coverage contracts

These are observable behavior failures, not requests to exercise arbitrary branches. The frozen staged `0674431` covered run remains independently sealed at 2011 pass / zero failures / seven explicit no-pass ignores; its coverage floors remain a separate failing gate.

- **Forged interaction navigation admission (fixed in reviewed component `395076d0cff8a41755adb89b63afc5fde415c0d5`; final integration pending).** `tests::coverage_human_interaction_ignores_a_forged_navigation_hint` demonstrated that guest-authored `data-hi-location` caused `human_interact` to fetch `/forged` and report navigation without an actual Location or form commit. The tests-first baseline was 2 pass / 1 genuine failure. Removing only the obsolete hint-follow fallback preserves authenticated native navigation; the three new controls and all 118 MCP library tests pass, including real forms, Location, CSP, POST and cookie behavior. Strict/format/complexity checks pass. Evidence: `.worktrees/mcp-coverage-admission/.recovery/mcp-coverage-admission/baseline-native.log` and `FINAL-FUNCTIONAL-RECEIPT.json`. Its separate official MCP measurement passes 1934/1939 lines (99.742%, unchanged 99.6% floor), 131 tests; this is MCP-only coverage, not a workspace result.

- **Root-to-foreign-window Location rejected before navigation (fixed in reviewed component `3869b84980d935848e4f5ee72fb5cbdd2b06dc0b`; final integration pending).** `cross_origin_popup_location_writes_resolve_against_the_genuine_source` reached the genuine live source and target, but the host searched only frame-specific URL slots absent on the initial root document. The fix reuses the existing private captured document-base resolver without relaxing ownership or sandbox checks. `cross_origin_popup_navigation_uses_captured_base_without_guest_getters` additionally proves real `<base>` resolution and zero calls to poisoned public URL/baseURI getters. Actual registered proof is **21 pass**, not the earlier source-attribute count of 22: the default-disabled sandbox harness registered zero and receives no credit. Strict checks pass. Evidence: `.worktrees/coverage-actor-diag-067/.recovery/actor-diag/cross-metadata.log` and `.recovery/actor-on/final-source-receipt-corrected.json` in that worktree.

- **Window.closed invalid receiver silently accepted (OPEN, turbo-dom producer coordination).** `opaque_iframe_window_descriptors_preserve_access_and_receiver_checks` found the public getter returns `undefined` for `{}` instead of throwing TypeError. The exact Chrome control reports live child `false`, invalid object TypeError, null receiver adjusted to the global (`false`), and removed child `true`. Actual oracle version is Chrome 158.0.8127.18; it is not labeled Chrome 156. The native expectation remains unchanged. Evidence: `.worktrees/coverage-proxy-location-067/.recovery/proxy-location/closed-oracle/{chrome.mjs,result.json}` and the preserved expanded coverage test log. The fix belongs in the external turbo-dom binding, with no Surf vendoring; adjacent receiver boundaries require producer review.


## Full combined 97285 Rust and JavaScript results

The clean combined staged source `97285df75c5bf30809b5fd7025460a6c69be3667` uses four public DOM pins `2e195dac6e56eb94863b0c27421b5c7e2580277d` and the exact audited Deno .5 archives. Its covered Rust run completed **321/321 executables, 2069 pass, zero failures, seven explicit no-pass ignores**. Workspace line coverage is **33252/35303 (94.1903%)**, passing the unchanged 93% aggregate floor. All measured crate floors pass except Render: **24980/26997 (92.5288%)**, requiring **668 additional covered lines** to reach 95%. NAPI/Python remain excluded from line measurement; their actual binding-unit accounting is separate, not invented line coverage. The covered receipt is `39eb96b6b0cf822be9a8f34f7b5b0ce318fe9d84675fa335b0df79a1761d954f`.

The subsequent full JavaScript run completed all **177 files: 1055 pass, two failures, three intentional no-pass skips, zero cancellations**; detection was 274 pass / zero failures, and both offline Python helper tails completed. Same-run host V8 coverage was captured in 384 raw files; aggregation is pending here, so no percentage is claimed. This host coverage does not measure Rust/native or guest-page JavaScript. The two failures remain OPEN and are required regression selectors:

- `history-groups.test.mjs::guest back/forward/go cross document groups while same-group traversal remains live`: line 73 observed `/two` instead of `/spa`. Exact isolated host-phase diagnostic passed once, which is non-reproduction rather than a fix; unchanged cohort reproduction continues.
- `e2e/catalog.spec.mjs::product list shows every product`: line 16 timed out waiting for a product to be visible (5117 ms). An isolated rerun passed; original ordering/concurrency reproduction continues, with no assertion or timeout waiver.

Raw full logs live under `.recovery/final-coverage-next/js-after-covered-97285df7/full-gates/`; the immutable source/addon/trace and both failure records are pinned by `new-js-failure-ledger.json` (SHA `618779eb`, `.recovery/final-regression-driver-v33/`). Tracking adds both selectors monotonically; it is not an extra regression execution gate. Fix diagnostics take priority over the prepared 22 coverage controls.


### Additional history failure during unchanged reproduction

`history-groups.test.mjs::restored groups retain duplicate entry identity and replacement/forward semantics` is OPEN: an unchanged eight-process cohort with the original `NODE_V8_COVERAGE` environment produced 87 pass / one failure; this case timed out waiting for `load` at the unchanged 5000 ms limit. The original cross-group back/forward case passed in that cohort, so a shared cause is **not established**. Exact evidence: `.worktrees/history-groups-97285/.recovery/history-groups/covered-cohort-3.log` (session 15657). Source and immutable addon remain identical to the failed full 97285 run. Low-overhead buffered host event, waiter-registration and navigation-identity diagnostics are next; no assertion or timeout has been changed.
### 2026-11-18: catalog product-list visibility (full JS 97285)

- Case: `rust/playwright-shim/e2e/catalog.spec.mjs::product list shows every product`, line 16.
- Actual unchanged full-suite failure: `expected element to be visible`, 5117.721917ms; frozen source `97285df75c5bf30809b5fd7025460a6c69be3667`, fresh signed native addon.
- Receipt: `.recovery/final-coverage-next/js-after-covered-97285df7/full-gates/test:e2e-1.log`, line 33 onward.
- Status: cause unproven; exact single-spec reproduction queued behind active native detection. No selector, timeout, or visibility assertion waiver.
- Fixture is static server-rendered `<ul data-testid="product-list">` with three real `<li>` children. Investigation must distinguish native geometry/visibility from live-locator identity before assigning a producer bug.
- Follow-up: exact immutable single spec passed 4/4; original dropin/auth/catalog prefix passed 9/9 plus five repeats (45/45); complete original e2e order passed 55/0F/2S without coverage, then unchanged frozen source with original recorded environment and fresh `NODE_V8_COVERAGE` passed 55/0F/2S. Three bounded covered diagnostic full-order repetitions each passed 55/0F/2S. Across these runs the catalog first case passed 12 attempts; original failure remains open/unexplained, not marked fixed.
- Diagnostic limitation: expect retry swallows non-strict errors; original generic visibility timeout does not prove zero geometry. Isolated catch-only diagnostics retain/rethrow first and last `isVisible()` errors and collect DOM/ancestor style/rects on final failure. No production workaround or assertion change.

### 2026-11-18: locator retry hides persistent underlying read exceptions

- Proven independently of the intermittent catalog case: `BaseExpect._retry` discards non-strict read exceptions and `_ok` replaces them with a generic assertion failure.
- New `assertion-retry-diagnostics.test.mjs` reproduces the missing error cause: red 4 passed / 1 failed; persistent read error's exact identity was lost. Fix retains the latest erroneous observation as assertion cause/message and clears it after every valid observation, including false.
- Green 5/5 controls: transient error→true recovers; persistent errors preserve exact latest cause; transient error→valid false exhausts without stale cause; ordinary false keeps existing diagnostic; strict errors escape immediately. Retry timing, timeout, predicate and negation behavior are unchanged.
- This diagnostics fix does not establish or resolve the original catalog failure's root cause.


### New module-deadline control awaiting classification

`navigation_deadline_module_tests::completing_a_real_module_source_releases_its_descendant_budget` timed out at its original source deadline (`navigation_deadline_module_tests.rs:269`, 5.55 s) during the new 22-control behavior validation, session 55978. The preceding five Location contracts and module-expiry control passed; 15 disjoint controls were initially unrun after the stop-on-failure boundary. An earlier localhost bind permission denial was setup-only and is recorded separately. The new timeout is OPEN pending source-author classification as a real product issue or an incorrect fixture premise. No source deadline, assertion, or expectation has been increased or waived; this does not block integration of the independently proven history URL repair.


### History URL race repaired; remaining failures stay distinct

Reviewed fix `6f5f3aa2` (component `e4c41b4f93abe9403176799bdacbb5db12715da5`) prevents an awaited legacy request-context snapshot from replacing the authoritative native-history URL. Buffered reproduction proved native journal/document identity had correctly advanced while a delayed old snapshot rewrote the shim cache. A deterministic real-Page regression holds delivery of a genuine old snapshot, completes real navigation, then releases the snapshot: unchanged production failed `/one` versus `/two`; the one-line native-history guard passes. Disabling only that guard reproduces the exact failure; byte-exact restoration passes. All 36 history/authority adjacencies and 96 covered-environment repeated history cases pass. Final receipt: `.worktrees/history-groups-97285/.recovery/history-groups/FINAL-RECEIPT.json` (`aaf51ebd959c0ed1611d771795789ad3c63e2d4f5754e5906321af293d6cd82f`). The original stale-URL failure is repaired; the separate earlier `restored groups...` load timeout and original catalog visibility failure remain OPEN rather than assumed equivalent.

The new coverage control `runtime_document_contract_tests` watchdog case also remains OPEN pending source-premise classification: session 61085 observed `is_execution_terminating()` false at line 53 after a 0.46 s run. This is a newly authored control, not a proven production defect; no budget/assertion was changed. Module-deadline timeout classification remains OPEN separately. Disjoint remaining controls continue without fail-fast. The locator retry diagnostic fix (`70e1d5d`) passes 22 related expectation/locator tests and preserves the last real read error; it improves diagnosis but does not establish the original catalog cause.


## Integrated JavaScript validation after the history repair

The full current official JavaScript run on pushed source `6e75849fac2f8a5e43e6c84aa318aa9f164f74d7` completed **178 files, 1063 pass, zero failures, three intentional no-pass skips, zero cancellations**, plus both offline Python helper tails. It includes all five retry-diagnostic controls and the new deterministic history regression. The original history stale-URL case passes after its causally proven repair. The original catalog visibility case and the separately observed history load-timeout case also pass in this full run, but their original causes remain unproven; a passing observation does not establish those as independently repaired. Evidence: `.recovery/final-coverage-next/js-root-6e75849/driver-retry.log` and its `full-gates/` results. The run explicitly composes current Root JavaScript with the preserved `97285` native component; it does not claim byte-equivalent Root Rust sources. Fresh same-run host coverage is measured: product JavaScript 10317/11708 lines (88.1192347%), 3776/4335 branches (87.1049596%), and 1232/1409 functions (87.4378992%); developer harness coverage is reported separately. The receipt is `a31c91f92071b71b6245ed6abc82fbcccb03941a6fd7c8d7d1ef4b2982890fa7`, with all 384 raw files stable. This measures Node-hosted JavaScript only, and no numeric JS floor is configured.

All 22 next Render contracts now pass after reviewed, source-backed fixture corrections: the completion fixture uses the public RealTime mode required by its non-advancing poll protocol (the expiry fixture remains Virtual); the watchdog witness uses `Watchdog.exceeded` and the actual termination error because Core clears the raw VM flag while creating that error. Exact module pair passes, then the unfiltered whole LIB plus four integration targets completes **323 registered pass, zero failures, zero ignores/filters** (session 7558). The separately repeated prerequisite pair is not counted as two additional unique tests. Production behavior, original budgets, and coverage floors are unchanged; fresh Render-only line measurement remains pending.


Producer issue backfills: [turbo-dom #514](https://github.com/miaskiewicz/turbo-dom/issues/514) records the already repaired adopted-image creator-realm state defect (fix 625472a, public descendant 06c6); [turbo-dom #515](https://github.com/miaskiewicz/turbo-dom/issues/515) records the already repaired Window getter receiver defect (public fix 2e195dac). These tickets were filed after the fixes and explicitly preserve that chronology. Future producer defects require a detailed issue before a fix request.

Producer confirmation: both #514 and #515 are closed as completed and assigned to the v0.8.4 milestone (#6). Public `release/0.8.4` head `2e195dac` contains `06c6`; its exact CI run [44202084318](https://github.com/miaskiewicz/turbo-dom/actions/runs/44202084318) passed all 26 jobs.


## Isolated registered-operation coverage controls: nine pending failures

The new source-only coverage batch on `c5ead783` ran all 17 controls independently (session 36000): eight passed and nine failed, including two fatal isolate-disposal crashes. These tests are isolated and uncommitted; they do not invalidate or replace the previously sealed full-suite green evidence. The production/fixture cause of each failure is pending exact original-author classification. No expectation, timeout, authority check, coverage floor, or production behavior is waived. A unit-return null/undefined ABI premise and V8 scope/destruction ordering are hypotheses only until verified against the actual registered operation and caller.

- `runtime::registered_fetch_prepare_tests::registered_fetch_prepare_rejects_lookalikes_without_consuming_request_identity` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_fetch_prepare_rejects_lookalikes_without_consuming_request_identity.log`.
- `runtime::registered_fetch_prepare_tests::registered_fetch_prepare_rejects_retired_document_and_accepts_real_replacement` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_fetch_prepare_rejects_retired_document_and_accepts_real_replacement.log`.
- `runtime::runtime_registered_document_lifecycle_tests::registered_frame_helper_rejects_foreign_documents_without_mutation` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_frame_helper_rejects_foreign_documents_without_mutation.log`.
- `runtime::runtime_registered_document_lifecycle_tests::registered_retirement_requires_the_owner_and_revokes_child_services` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_retirement_requires_the_owner_and_revokes_child_services.log`.
- `runtime::runtime_registered_geometry_tests::invalid_and_retired_rendering_targets_preserve_documents_and_live_frame_progress` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/invalid_and_retired_rendering_targets_preserve_documents_and_live_frame_progress.log`.
- `runtime::registered_resource_deadline_tests::registered_frame_capabilities_bind_actual_host_window_and_reject_stale_completion` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_frame_capabilities_bind_actual_host_window_and_reject_stale_completion.log`.
- `runtime::registered_resource_deadline_tests::registered_timer_document_guards_preserve_actual_queues_and_reject_retired_owner` — OPEN; fatal isolate-disposal crash, exit `-5`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_timer_document_guards_preserve_actual_queues_and_reject_retired_owner.log`.
- `runtime::registered_resource_deadline_tests::registered_module_metadata_rejects_invalid_and_retired_owners_without_resurrection` — OPEN; assertion or runtime failure, exit `101`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_module_metadata_rejects_invalid_and_retired_owners_without_resurrection.log`.
- `runtime::registered_resource_deadline_tests::registered_resource_consumers_reject_wrong_and_retired_documents_then_execute_healthy_owner` — OPEN; fatal isolate-disposal crash, exit `-5`. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/registered_resource_consumers_reject_wrong_and_retired_documents_then_execute_healthy_owner.log`.

Authoritative batch source/artifact and per-case results: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/results.json` (source manifest `4972390668e577593e08f8aee3056abd3571ca174a2985d268d5cc167e0094d7`). Every real production defect requires a failing regression and causal repair; a producer defect requires a detailed GitHub issue before a fix request.


### Confirmed native-helper security exception registration defect

`runtime::runtime_registered_document_lifecycle_tests::registered_frame_helper_rejects_foreign_documents_without_mutation` now has a confirmed Surf production defect, distinct from the repaired test premises in the same batch. After identity and healthy-root controls pass, the actual registered helper's foreign-document denial exposes `undefined` rather than the required typed SecurityError/message. The native helper returns a custom DOMException error class through Core, but Surf lacks the matching canonical per-realm error builder. The exact expected error remains unchanged. A minimal Surf registration repair must use the captured canonical browser-env DOMException, with no copied binding, guest Core exposure, or broader authority fallback; public error reproduction, red/green/inverse and related security controls are required before acceptance. Evidence: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/results-retry-nine.json` (session 89851). Retry results are seven pass / two failures; the full 17-control state is 15 pass / two unresolved, with both prior isolate-disposal crashes gone after source-proven fixture lifetime corrections. The separate module Root-expression budget failure remains unclassified.


### Confirmed URL shim intrinsic-capture defect

The focused public SecurityError contract exposed a separate Surf production bug: replacing the guest global `String` caused two calls during real `history.pushState`, while result inspection caused no additional calls. The captured call stacks identify the Surf URL shim's parse (`String(input)`) and constructor (`String(base)`) paths in `runtime.rs` around lines 12723/12734. The security denial, realm branding and healthy recovery otherwise pass. This is not a turbo-dom defect and the expected zero poisoned-constructor calls is retained. A separate URL/URLSearchParams intrinsic-capture TDD repair must use the existing trusted `__String` capture; it must not be bundled with the one-name SecurityError registration fix. Root's readonly immutable-NAPI diagnostic established the cause; the previous Rust diagnostic-only proposal is no longer needed for this known cause.
### 2026-11-18: URL conversions call replaced global String

- Confirmed Surf defect from the public native probe: a poisoned global `String` is called during URL parsing/constructor base conversion (runtime.rs12723/12734), including history URL updates. This is Surf's bootstrap implementation, not a producer defect.
- Modular fix uses the existing pre-authored-script `__String` capture only inside the URL/URLSearchParams block, including iterable pair mapping and record/value/method conversions. User object coercions and their exceptions remain active; no unrelated intrinsic hardening is included.
- New `url_string_intrinsic` public integration controls run in actual root and child realms: clean-versus-poisoned URL construction/mutations/searchParams identity/static parsing, sequence and record/proxy parameter inputs, own `toString`/string-hint conversions, sentinel exception propagation, unchanged state after failed mutations, zero poisoned-global calls, and healthy subsequent String calls.
- Tests were authored before the production patch. Root executed both baseline controls RED: root and child each observed one poisoned-global String call in both URL and parameter exercises. Both fixed controls then passed all exact zero-call/coercion/proxy/state assertions (Root ON receipt `20261118T051128Z-dd19ae2314af-a3d6abb1/ROOT_ON`, 2/2 URL controls). Preserved isolated OFF/ON source archives also hold SecurityError registration and the strong public security control unchanged: only the 16 URL-local captures differ, while the same root/two-child control changes from two poisoned-global calls to zero. No additional inverse rebuild is needed for this bound causal proof. Final combined native controls pass 21/21. Render/NAPI all-target Clippy with denied warnings, formatting, and CC15 gates pass (central strict receipt `20261118T052917Z-strict-final-48299db0/results.json`). Expanded regression inventory includes both selectors in `tests/url_string_intrinsic.rs`.
### 2026-11-18: missing native SecurityError builder throws undefined

- Fixed Surf-owned production bug: `op_frame_host_helper` returns `DOMExceptionSecurityError` for an actual foreign document, but the existing captured main/E builder list omitted `SecurityError`. Deno's unregistered custom-error builder returns `undefined`, discarding the original exception type and message.
- The minimal fix adds one name to the existing canonical builder list. Once-only main/E constructor capture, child collection, per-realm DOMException registration and private-boundary rehoming are unchanged; there is no new registry, guest core exposure or producer binding change.
- Actual OFF proof: immutable `ROOT_BASELINE` in attempt `20261118T050819Z-66e016c8407f-682cdf0b` fails both original foreign-helper and focused typed-native controls; the original rejection is exact `undefined` after valid caller, opaque-token, unchanged-document and Root-health witnesses.
- Actual ON proof: immutable `ROOT_ON` in attempt `20261118T051128Z-dd19ae2314af-a3d6abb1` passes 5/5 with the separately owned URL intrinsic fix applied. Focused controls retain exact canonical E prototype/name/message/code18 and each public Root/child realm's canonical exception under constructor poisoning, history nonmutation/recovery and child replacement health.
- Focused fixture corrections preserved the contract: canonical E identity is independently witnessed by the existing SyntaxError builder rather than assuming an own global constructor property; URL-omitted history recovery preserves real srcdoc URLs instead of rewriting against an inherited Root base.
- The zero-poison control exposed a separate Surf URL/URLSearchParams ambient-String bug, owned and fixed in a separate commit; it is not included in this issue's change. Its fix must be integrated first. Public history adjacency does not claim a Surf-native guard hit if the producer rejects earlier.
- Final restored proof: `ROOT_FINAL` in attempt `20261118T051419Z-8f05bd26ef0a-a752b454` passes all 21 cases (original 17, focused security 2, URL 2), with no crashes. Strict attempt `20261118T052917Z-strict-final-48299db0` passes Render/NAPI all-target clippy with warnings denied, formatting and complexity 15.
- Receipt root: `.worktrees/coverage-registered-native-c5/.recovery/registered-native/attempts/`. Earlier discovery aliases were reused by the retry runner; only the unique attempt receipts above and retained snapshots are authoritative immutable evidence. No full-workspace or coverage-gain claim is implied by these targeted results.
### Registered native contracts: fixture corrections and final verification

The 17 genuine registered-operation controls use actual PageSession documents, RuntimeCore functions and native state, with valid controls, rejected identities/retirement and healthy recovery. The first attempt produced eight passes, seven assertion failures and two fixture disposal crashes. Exact resolved Core/Deno ABI evidence corrected unmarked Rust unit results to null; scopes and retained handles now end before consuming PageSession::close. Inline modules do not emit load: Chrome 159 confirmed body execution without load, so the completion fixture now resolves from the actual module body in default Virtual mode with its original 5000 ms budget. No production scheduler, authority or deadline guard was relaxed.

After the separately proven SecurityError registration and URL intrinsic fixes, Root's immutable final attempt completed all 19 LIB contracts plus two URL integration contracts: 21 pass, zero failure or crash. Render/NAPI all-target Clippy with denied warnings, formatting and complexity limit 15 pass. Navigation slice-reference lint fixes are mechanical and preserve assertions. Coverage has not yet been measured for this packet; helper hits are not production coverage gains.

Evidence preservation limitation: nine failed raw logs from the first 17-case attempt were overwritten by a retry. Their original hashes and excerpts survive, but complete original failed raw logs are not claimed recoverable. The initial source/artifact manifests and binary survive in the attempt1 archive; eight surviving pass logs and all subsequent retry/final attempts have separate immutable snapshots. Every later attempt snapshots actual source bytes, artifacts and unique output paths before execution.


## Confirmed adjacent URLSearchParams constructor capture defect (OPEN)

After the String-capture repair, a readonly public-NAPI reproduction replaced guest-global `URLSearchParams` with a throwing constructor and constructed an otherwise valid `URL`. The replacement was invoked once and the URL construction threw. Source inspection identifies three internal `new URLSearchParams` references in Surf's URL fallback block still reading the mutable guest global. This is a separate Surf defect, not a turbo-dom producer issue. A narrow separate TDD fix must capture the original constructor inside that existing block, preserve ordinary public binding mutability and Root/child realm behavior, and retain exact strong poison-call assertions. RuntimeCompat owns the isolated repair; it is not part of the active `1fd865f` full JavaScript or targeted coverage epoch. This ledger was created in an isolated documentation worktree to preserve that active source freeze and must be merged only after the run ends.


## New normal-public full-run navigation observation failure (OPEN)

On exact source `1fd865f`, fresh normal-public native artifacts, the full JavaScript run 62925 observed `navigation-observation.test.mjs::both collectors use a fixed Enter window for slow forms and delayed retries` failing at line 609: `assertFinalDocument` line 424 (`retry-fast-common/native`) expected true and observed false after 86.989 s. The cause is unproven; the fixed observation window, assertion and thresholds remain unchanged. Exact raw log: `.recovery/final-coverage-next/js-public-1fd865f5/full-gates/test:detection-1.log:1123`; original capture: `/var/folders/d0/zxfcybz151j_zkd609664rq80000gn/T/detection-navigation-observation-ZuSB91`. The current 178-file run continues without fail-fast and cannot be called green. The earlier 1063-pass JavaScript run is historical evidence only. Source/capture chronology classification and a targeted failing-case-first repair take priority over coverage work; no further full rerun is credited before this case passes. This entry is isolated until the active source epoch ends.


### Navigation observation validator contradiction repaired

Reviewed one-file test-contract fix `6003c343` (component `72160cea321770230d48a878ed79f0140b3c5bda`) propagates the phase returned by the existing strict native retry validator to the common final-document check. The retained failure already satisfied the authenticated partial-terminal contract (actual guest/response URL, exact pending HTML/status, no collector errors, both timer witnesses and terminal-before-deadline); the outer validator had then incorrectly rejected its verified `loading` phase. A full-layer pure TDD regression fails before the fix and passes after, while invalid wire, both clocks, URL/body/errors/status, generic loading and Chrome cases remain rejected. The exact original failed title passed in Root session 47374: one pass, zero failures/skips, 79.738 s, using the same immutable trace and Chrome 156 path, with no observation-window, timeout or assertion weakening. Evidence: `.recovery/final-coverage-next/ROOT_FAILED_ORACLE/{PLAN.json,FINAL.json,test.log}`. The earlier full 178-file run remains recorded as 1062 pass / one failure; this targeted green proof is not relabeled as a new full run. No engine behavior changed.
### 2026-11-18: URL construction reads a page-replaced URLSearchParams constructor

- Confirmed separately by Root's read-only public NAPI probe on integrated `1fd865f5`: replacing global URLSearchParams with a throwing constructor makes valid `new URL(...)` call that constructor and throw. The earlier captured-String fix is present and unrelated. This is Surf's URL bootstrap, not a producer issue.
- Separate modular fix captures the canonical parameter constructor before authored scripts inside the URL-definition block, then uses that private reference in URL construction, component/search updates, and href assignment. The public global binding remains mutable and callable by explicit application code.
- New public `url_search_params_constructor_intrinsic` root/child controls cover throwing, Proxy, and getter replacements; require zero internal guest-constructor reads/calls, original parameter brand/prototype and retained identity, live query updates, static parse/canParse, authored coercion/sentinel atomicity, and a positive explicit `new globalThis.URLSearchParams` call into each replacement.
- Tests were authored before the source patch. Root executed both unchanged-baseline controls RED (valid URL invokes the public replacement once), then both exact fixed controls GREEN across all throwing/Proxy/getter, identity, coercion, rollback, and explicit public-constructor assertions. Archived before/after runtime bytes differ only by the reviewed private alias and three sites; tests remain byte-identical (`5f26ac1b…`), providing the isolated causal contrast without an additional OFF rebuild. Render/NAPI all-target Clippy with denied warnings, formatting, and CC15 all pass; the new test also passes its CC15 check (central receipt `20261118T072237Z-strict-bdcbea37/results.json`). This packet was prepared without changing the earlier frozen `1fd` coverage/JavaScript graph.


## Current validated checklist — public source 954b0385

- [x] URL String intrinsic capture: integrated `7defb26`, component `3c5af275`; Root/child public TDD and poison/recovery controls pass.
- [x] Canonical native SecurityError registration: integrated `294f1c5`, component `d1237b7b`; exact native denial, typed per-realm errors, poisoned-constructor and replacement controls pass.
- [x] URLSearchParams internal constructor capture: integrated `954b0385234dae8611721cd1818dccf57c8eae5a`, component `68fb7e54`; identical Root/child tests failed with internal guest calls before the one-alias/three-site repair and pass afterward, while explicit public replacement remains mutable. Strict checks passed.
- [x] Authenticated partial navigation validator: integrated `6003c343`, component `72160cea`; full-layer negative controls, retained failure replay and exact original failed title pass without changing the fixed observation window or engine.
- [x] Fresh normal-public full JavaScript on `954b0385`: **178 files, 1064 pass, zero failures, three intentional no-pass skips, zero cancellations**, plus both offline Python helper gates. Native trace/addon were freshly built from main 954b source; tests ran from an immutable byte-identical owned checkout. All 1913 owned tracked hashes stayed unchanged. Receipt `.recovery/final-coverage-next/js-public-954b0385/FINAL-OWNED-FULL-JS-HOST-COVERAGE-RECEIPT.json` SHA `e1237108` pins the complete evidence. All previously failing JavaScript selectors pass in this current run. The history stale-URL and navigation-validator causes are repaired; the catalog and separate load-timeout causes were never established, so their current passing observations are not invented causal fixes. Historical failed full-run receipts, including `f2e051be`, remain unchanged.
- [x] Same-run **host JavaScript** coverage: product 10314/11708 lines (**88.0936%**), 3764/4327 branches (86.9887%), 1232/1409 functions (87.4379%); 384 raw files, receipt `e269c254`. Developer coverage is separately 7350/52219 lines (14.0753%). No numeric JavaScript coverage floor is configured; these figures do not measure Rust/native or guest-page code.
- [ ] Rust line-coverage release gate: latest authoritative targeted/mapped owner-scope union is **Render 25712/27624 (93.0785%), 531 covered lines short of 95%**. Workspace 33984/35930 (94.5839%) passes 93%, and the other five measured crate floors pass. Production-hit progress is **118** since the 97285 baseline (77 + 41); newly eligible test-helper hits are kept separate and receive no credit toward the requested production-coverage buffer. This is an explicit mixed-epoch, content-mapped union, not a newly executed whole-workspace run. The targeted 8883/41100 execution used staged sources byte-equivalent to the published archives; corrected receipt `5ef3e594` preserves that label and the earlier wrong-label receipt. Current full JavaScript uses the ordinary public registry.
- [ ] Remaining meaningful reviewed coverage queue: all 20 controls ran in an isolated 954b checkout: 17 passed and three failed, as tracked below. The failures are under investigation; no new coverage gain or full gate is claimed. Floors and production behavior remain unchanged.

### Experimental reviewed Render queue: three failures under classification

The isolated test-only queue on `954b0385234dae8611721cd1818dccf57c8eae5a` compiled after a mechanical raw-string delimiter correction that preserved its expected JSON bytes. Native attempt `7761` ran all 20 reviewed new cases independently: **17 passed, three failed, zero crashes and zero unrun**. Original source bytes, signed binaries, actual registration lists and every raw result remain in `.worktrees/render-reviewed-queue-954/.recovery/reviewed-queue-attempts/20261118T083210Z-7cc3cebb/`, with the exact case plan and results in `NATIVE20/`. These experimental results are not a merged coverage/floor claim.

- `csp_resource_sample_trims_long_padding_and_limits_utf16_code_units`: the blocked eval emitted an empty sample, whereas the new fixture expected 40 UTF-16 units after trimming 260 leading spaces. A subsequent nonce-admitted, UTF-8 localhost Chrome **159.0.8154.6** oracle confirms the distinction: eval truncates before trimming and has an empty sample; dynamic inline resource sampling trims before truncating and has the intended 40-unit sample. This is a fixture/oracle mismatch, not a production defect. A corrected two-path contract is pending source review and fresh execution; the original failure remains immutable.
- `explicit_viewport_options_set_actual_window_and_root_dimensions`: actual `[640,480,640,51]`, expected `[640,480,640,480]`. The fixture lacks a doctype, but a subsequent exact Chrome 159 control still reports HTML client/rectangle height 480 in BackCompat, while the standards control has client height 480 and HTML rectangle height 50. A standards-only fixture change would therefore hide the original mismatch. Surf document-mode forwarding versus producer layout ownership is under source investigation; the original assertion remains unchanged and no producer fix request is justified before attribution and a detailed issue.
- `runtime::runtime_queued_classic_interruption_tests::collector_watchdog_rejects_real_queued_classic_and_retires_only_its_child`: the actual child context lacked `FrameRetired` after the real collector watchdog; the fixture expected retirement. The finite queued-script control passed. Source/caller and native Promise/interruption metadata must distinguish a fixture premise from a real lifecycle defect before any change. Budgets and assertions are unchanged.

The already sealed `954b0385` full JavaScript result remains a separate historical proof: 178 files, 1064 passed, zero failures, three intentional no-pass skips and both offline tails passed. This experimental test-only queue does not claim a new full JavaScript run, repaired engine defects, or new measured coverage. New producer defects require a detailed GitHub issue before a producer fix request.
### Queued child-script interruption survives the TryCatch lifetime boundary

A real queued child classic script performed its admitted mutation and then ran indefinitely under the existing public 2000 ms collector budget. The new unchanged regression observed that its child context was not retired. Native-only diagnostics captured `guarded=false`, `outcome=Completed`, `terminating=false`, but `failure_interrupted=true`: the inner TryCatch had retained the genuine native termination witness after the transient isolate flag cleared. The outer execution wrapper checked only that transient flag or a frame-deadline outcome, so the job fell into the later reject/restore/reterminate fallback and skipped the existing deferred frame cleanup. No guest error string or property controls this typed native flag.

The repair adds that persisted interruption witness to the existing interruption predicate. It preserves document identity, weak lifetime checks, queue ownership, deadline/cancellation handling and budgets; it does not introduce a new cancellation policy. Test-only diagnostics were removed. Original regression expectations remain exact: the affected child is retired, its waiter rejects with the interruption error, admitted markup and anonymous currentScript state remain observable, and the same Root and peer stay healthy. The finite-script contrast and four existing ordinary-error/realm/error-event/native-authority controls also pass.

Causal evidence is immutable under `.worktrees/render-reviewed-queue-954/.recovery/reviewed-queue-attempts/`: the original 20-case epoch `20261118T083210Z-7cc3cebb/NATIVE20` records the retirement failure; `20261118T084745Z-47d2977c/NATIVE2` records the native flag mismatch; diagnostic-free ON `20261118T085950Z-c46da679/NATIVE6` completes six independent cases with zero failures or crashes. The exact production patch is `ad48aeedc7c98ba987dbbe580383e959f7c51bccf2fa17426c965bc11ca01f67`; focused source remains `4cd55469c3c614a4f7801ce2b106772fe140406b07f858375d553865874c2242`. The separate CSP fixture correction and producer viewport issue #516 are excluded from this fix. Fresh post-fix JavaScript/addon and coverage results remain required; the earlier 954 full JavaScript green and measured 531-line coverage gap are historical evidence, not new-fix credit.


## Coverage expansion — 90% host JavaScript and another Rust round

- [ ] Enforce at least **90% independently for host JavaScript lines, branches and functions** in local tooling and CI. Preserve the existing 43-file product inventory, include unloaded product files, and fail on missing inputs or incomplete execution; do not count native Rust or guest-page JavaScript as host coverage. Baseline on `954b0385`: 10314/11708 lines (88.0936%), 3764/4327 branches (86.9887%), 1232/1409 functions (87.4379%). A fresh full run on `157a60b6` is in progress; no new result is claimed here.
- [ ] Add meaningful JavaScript behavior tests in separate worktrees: launcher and NAPI loading; selector and keyboard defaults; JSHandle/ElementHandle APIs; other locator/assertion/page APIs. Tooling owns official test registration and the coverage gate. Any discovered defect requires a reproducer and causal fix before integration.
- [ ] Run the already compiled accepted 17 Rust controls after the current JavaScript execution lane releases, then measure targeted additive Render coverage. Add another supported-call-path coverage round for history and resource/async lifecycles, avoiding duplicate controls and artificial runtime states.
- [ ] Retain the current **95% Render floor**. At the current denominator, 25712/27624 covered lines needs **255 additional hits for 94%** or **531 for 95%**; new eligible lines can change those gaps. The suggested 94% fallback has not changed configuration. Count newly covered production lines separately from test helpers and report measured gains.

### Prepared independent Render contract packet; viewport remains pending

Seventeen test-only contracts are separated from the production classic-interruption repair and its two focused regressions: cross-realm pipe/iterator cancellation, Blob/CSP resource admission, native/public history validation and reentrancy, hidden Range/positioned scroll, actual private host-route transport, form CSP, cached driver errors and retired Location wire denial. Their independent case outcomes are preserved in the original `083210Z-7cc3cebb/NATIVE20` epoch; the CSP fixture's browser-proven eval/inline correction additionally passes in `084745Z-47d2977c/NATIVE2`. This is historical source-bound proof across epochs, not a new complete run on the integrated graph or measured coverage credit.

The original viewport assertion remains unchanged in its frozen reproduction and producer issue [turbo-dom #516](https://github.com/miaskiewicz/turbo-dom/issues/516). It is deliberately separate from this packet until the producer fix/proof exists; no standards-only replacement, ignored test, or expected-value waiver is used. The two successful layout contracts are retained without the pending viewport function. The separate classic fix passed its original retirement/waiter/Root-peer checks and ordinary-error controls; it is not duplicated here. Fresh validation/coverage will bind the eventual integrated source.

The separated packet was subsequently validated on owned `157a60b6`: Native attempt `20753` ran all 17 exact cases with zero failures or crashes. The unfiltered targeted instrumented run `18126` then passed 340 LIB tests and 16 integration tests (356 total), with source hashes unchanged. The official content-mapped owner merge now measures Render **25940/27778 (93.3833%)**, **450 hits short of the unchanged 95% floor**; workspace **34212/36084 (94.8121%)** and the other five crate floors pass. This mixed-epoch additive result retains every old eligible line and adds 154 eligible lines. Its production progress is **74 previously uncovered production lines plus four newly eligible classic-fix production lines**; the 150 new test-helper hits are reported separately and do not count toward the production goal. Cumulative production progress since 97285 is 196. The immutable receipt is `.recovery/coverage-additive-157a60b6/merged-accepted17/FINAL-TARGETED-ADDITIVE-ACCEPTED17-RECEIPT.json`; this is not a newly executed whole-workspace run. The viewport producer issue remains pending.

Affected Render/NAPI all-target Clippy with `-D warnings`, workspace formatting and the 11 changed Rust files’ CC≤15 checks all pass on the same source. Strict attempt `20261118T100613Z-06490945` preserves the 428-file source snapshot and successful results.

### Page dispatchEvent silently skipped live DOM delivery

The public Page shortcut had an empty implementation while Locator.dispatchEvent already dispatched into the retained native document. A genuine public regression first dispatches through Locator as a working control, then requires Page to deliver the target MouseEvent with its button/clientX, untrusted flag, disabled bubbling and noncancelable behavior. The unchanged baseline failed because only the Locator event was observed (0 passed, 1 failed); no fixture timeout or assertion was relaxed. Missing-target rejection and a healthy later read remain part of the regression.

The proposed repair forwards the selector, event type, init and options to the existing Locator method. It preserves that method's settlement and error behavior; it does not claim a new missing-target wait/timeout capability. Baseline log: `.worktrees/js-public-api-coverage/.recovery/js-public-api-coverage/dispatch-red-1/test.log`; source baseline SHA256 `7982354a5fa126b545d32d6edb6fdc21d465ca6786676b0cbb7df5eaef5d2869`, regression SHA256 `a573b0e2fc7517c5d76fe7bb5f5dba11472de67e7bde9cd6b646aca24313a470`, immutable native addon SHA256 `4a2185778d0f0de712c3cf2a697c799bf92dd59f9f6d4fdf14df26cf16930b38`. The unchanged regression passes with the delegate enabled (1 passed, 0 failed), and the existing #372 target-ownership and #371 initializer/propagation controls also pass (2 passed, 0 failed). Green logs are under `.worktrees/js-public-api-coverage/.recovery/js-public-api-coverage/dispatch-green-1/`: `new-test.log` and `related-tests-localhost.log`. The initial related-test attempt was blocked by sandbox localhost `listen EPERM`; its separate log is retained and the authorized localhost rerun is the actual product proof. This is a Surf-owned JavaScript defect; broader full-suite and coverage results remain pending.

### Locator innerHTML returned the matched outer element

A genuine frame public API contract expected `frame.innerHTML('#note')` to return `First note`, but the unchanged shim returned `<p id="note">First note</p>`. The forwarding Frame method is correct; Locator.innerHTML returned the query record's outer `html` for ordinary CSS queries, whereas extended queries already read the live element's innerHTML. The failing expectation is preserved; no assertion or timeout is weakened. Two other new public page/locator contracts passed in the same finite run. Raw log: `.worktrees/js-public-api-coverage/.recovery/js-public-api-coverage/frame-run-1/test.log`.

A focused Root/frame markup, outerHTML contrast, missing-element, mutation and peer-isolation control is being added before the proposed narrow repair reuses the existing genuine DOM element read for both locator forms. The original frame contract and related existing text/markup reads must pass after the fix. This is a Surf-owned shim defect. The independent focused Root regression also failed unchanged (0 passed, 1 failed), returning the outer section instead of its children. Reusing the existing prepared DOM read path makes that focused regression and the unchanged three frame/page contracts pass (4 passed, 0 failed), including the original failing frame assertion. The existing Locator text/markup control passes as well (1 passed, 0 failed) under the official `--test-force-exit` runner flag. Its first invocation passed its assertion but retained existing fixture handles; that log was preserved before stopping only the identified test processes and rerunning with the standard flag. Causal logs are in `.worktrees/js-public-api-coverage/.recovery/js-public-api-coverage/inner-html-red-1/test.log` and `inner-html-green-1/{new-tests.log,related-read-finite.log}`. Broader full-suite and final coverage validation remain pending.


### Quirks viewport geometry: producer fix adopted for validation

The original BackCompat control expected `[640,480,640,480]` but measured `[640,480,640,51]` on the previous producer pin. Chrome 159 and the WHATWG quirks root/body sizing rules confirmed the expected viewport floor. Producer issue [turbo-dom #516](https://github.com/miaskiewicz/turbo-dom/issues/516) was filed before requesting its fix; the correction now exists in producer commit `fb6957adcb6597426e36ca0c740fcdab872f343c`. Surf adopts all four DOM Git crates coherently at that revision, with published versions 0.8.4 and native V8 150.4.2 unchanged. The lockfile changes only those four Git source records. No producer implementation is copied into Surf.

The original unchanged `explicit_viewport_options_set_actual_window_and_root_dimensions` regression and the existing hidden-Range and positioned-scroll contrasts all pass with the new pin. Immutable source, signed binaries and all three raw results are in `.worktrees/viewport-producer-fb695/.recovery/viewport-attempts/20261118T102906Z-8bd7bf1a/NATIVE-20261118T103134Z-a20293/`. The original test function bytes remain SHA `ca8f3d4c351b1456efcab359e8b4c7e15a66544572a4a530eceaede19f051c74`. Two additional historical layout binaries list zero tests under this graph and receive no passing-test credit. Fresh trace/addon, complete JavaScript and coverage proofs must use the new producer graph; earlier 157a artifacts and coverage remain historical rather than being relabeled.

Affected Render/NAPI all-target Clippy with `-D warnings`, workspace formatting and the changed test file’s CC≤15 checks pass on this producer graph. Strict attempt `20261118T103215Z-bf11ae45` records the unchanged compiled source hashes and successful exit. Fresh full JavaScript and coverage remain separate requirements.


### Current complete JavaScript gate: all three 90% floors pass

Owned source `88d08ce9aa1d81b7ca78749160e3ed6395e195f5` completed the full official run `17745` with fresh producer-fb695 trace/addon artifacts, pinned Chrome 156 and restored sidecar dependencies. All 192 official test files were registered, plus the local-addon control file: **811 shim, 55 E2E and 275 detection tests pass (1141 official), with nine addon controls passing separately**. There are zero failures/cancellations and exactly the three intentional fixture skips; detection has zero skips. Both offline Python helper commands pass. Every scoped product file loaded and source hashes stayed unchanged.

Measured **host JavaScript** coverage on the unchanged 43-file scope is **10871/11708 lines (92.8510%), 4209/4666 branches (90.2057%) and 1328/1455 functions (91.2715%)**. All three unrounded 90% floors pass. This does not measure Rust/native or serialized guest code. The gate rejects unexpected skips, missing suites/files, unloaded product inventory, source changes and test failures. The immutable output is `.worktrees/js90-fixture-fixed-fb695/.recovery/full-js90-attempts/20261118T110214Z-b303679e/`; `END.json` records exit zero, unchanged source and full-log SHA `7ea2106c57f16de656030c8212d360b5cb9aa1e66b172c811de93144d39e1a4c`.

The three earlier output assertions were caused by the tooling's inherited `NODE_OPTIONS` reporter setting. The repaired gate passes an explicit outer reporter argument and removes only its private opt-in before launching tests, leaving nested process behavior and original assertions unchanged. A subsequent temporary addon-fixture failure was a missing copy of the new real wrapper helper; adding that dependency preserves every original assertion. The complete finite bundle (all nine addon controls, original three surface regressions, thirteen tooling controls) passes before this full run. Earlier failed and incomplete epochs remain historical; they are not relabeled as this complete proof.

The separate Rust gate remains open: its last authoritative measurement is Render **25940/27778 (93.3833%), 450 covered lines short of the unchanged 95% floor**. New passing Rust contracts have not yet received measured coverage credit. No floor, scope or complexity limit was lowered.

### User-authorized Render coverage target: 94%

On 2026-11-18 the user explicitly changed the Render coverage target from 95% to 94%. Only `turbo-surf-render` in `rust/coverage-floors.json` changes; the workspace 93% floor and every other crate floor remain unchanged. This instruction overrides the repository default against lowering a floor for this specific target change.

The latest independently sealed result (`7c0eccc5`, round-five additive owner merge) is 26028/27804 eligible lines, or 93.61243%. The new threshold requires 26136 covered lines, so **108 additional hits remain necessary**. No test, source exclusion, eligible-line denominator, raw report or measured count changes, and no coverage pass is claimed. Production, retained legacy-service and test-helper hit classifications remain separate.


### Requested Rust behavior areas: executed controls

All five requested areas have concrete passing controls. These checks establish the listed behavior, not 100% coverage of an area. The round-five owner-merge receipt `7c0eccc5` records 365 instrumented passes and the still-failing Render result 26028/27804 (93.61243%): **108 hits remain for the user-requested 94% target**.

| Completed area | Exact controls | Source under `rust/crates/turbo-surf-render/` | Raw proof |
|---|---|---|---|
| [x] Deadline expiry, replacement and recovery | `trusted_setup_expiry_rejects_reentry_without_terminating_the_root`; `obsolete_driver_guard_cannot_clear_its_replacement_lease` | `src/navigation_deadline_coverage_contracts.rs` | A, cases 8 and 7 |
| [x] Popup read, script and setContent watchdogs | `a_target_engine_read_watchdog_preserves_reached_state_and_other_pages`; `a_target_authored_script_watchdog_rejects_without_poisoning_next_evaluation`; `content_script_timeout_keeps_the_root_live_and_accepts_a_later_replacement`; `polling_an_actor_after_root_close_services_its_actual_surviving_child` | `tests/coverage_popup_target_budgets.rs` | A, cases 1, 0, 2 and 3 |
| [x] Geometry brands, SVG and stale-capture recovery | `native_geometry_brands_require_actual_html_and_input_receivers`; `svg_text_ranges_report_missing_native_geometry_without_poisoning_html_ranges`; `document_hit_test_recaptures_a_source_mutated_after_its_capture`; `inline_text_scrolling_uses_placed_runs_and_hidden_text_does_not_move_the_port` | `tests/coverage_geometry_boundaries.rs`; `src/live_layout_backend.rs` test module | A, cases 14, 15, 4 and 13 |
| [x] Geolocation observers and background recovery | `observer_detection_tracks_real_watch_registration_clear_and_retirement`; `real_background_watchdog_preserves_reached_dom_and_allows_a_later_turn` | `src/context_geolocation.rs` test module; `tests/coverage_background_recovery.rs` | A, cases 9 and 12 |
| [x] History state budget and traversal timeout | `actor_history_storage_budget_rejects_peer_growth_and_releases_replaced_state`; `driver_traversal_timeout_preserves_selection_and_allows_a_healthy_retry` | `tests/coverage_history_followup_contracts.rs` | B, cases 3 and 4 |

A is the immutable 16-case receipt `.worktrees/coverage-round5-ready10/.recovery/round5-attempts/20261118T111125Z-a3d65039/NATIVE16-20261118T111451Z-49ab97/FINAL.json`: all 16 pass, with per-case raw log hashes. B is `.worktrees/coverage-public-next5/.recovery/next5-attempts/20261118T105520Z-01fbc819/NATIVE-20261118T111052Z-0fa12b/FINAL.json`: all five pass, including both history controls. A case number is its zero-based raw-log prefix, not a new execution count. Affected Clippy, formatting and CC15 pass in strict attempt `20261118T111616Z-96dbcd23`; the instrumented re-execution and official owner merge are sealed at `.recovery/coverage-additive-round5-b725767b/merged-round5/FINAL-ROUND5-ADDITIVE-RECEIPT.json`.

The observer predicate is an existing retained legacy-collector service tested against real registration, clear and retirement states. Its 11 newly hit lines are classified separately from 51 newly hit supported-production lines; 26 newly eligible test-helper hits receive no production-buffer credit. No fake authority, exclusions, budget increases or revised assertions establish these completed checks.


### Latest Render measurement: registered host controls and source normalization

The three new controls passed unchanged, with affected strict checks green. The targeted warm measurement `68744` then passed **351 unique LIB tests, zero failures, ignores or filtered cases**. The independent official additive receipt `.recovery/coverage-additive-host3-2a974fd2/merged-host3/FINAL-HOST3-ADDITIVE-RECEIPT.json` (SHA `e7a301fb3ede643b533eab29b571410f6b5d638cfbfdfea2b591876ceedbb299`) measures **Render 26121/27878 = 93.69754%, 85 additional hits short of the user-authorized 94% floor**. Workspace coverage is 34393/36184 = 95.05030%; other five owner floors retain their separately scoped passing evidence. No eligible old line was dropped. Gain is 19 previously uncovered production lines and 74 newly eligible test-helper lines; helper hits are not production-goal credit. The native text callback contract proves argument forwarding, not SDK font accuracy. Old profiles remain separately archived and hash-verified; `--skip-clean` preserves normal Cargo freshness and instrumentation. This is targeted, mixed-epoch additive coverage, not a newly executed full workspace run.

Immutable source, signed artifacts, actual case lists and all five raw logs are at `.worktrees/coverage-next-cohort5/.recovery/cohort5-attempts/20261118T121011Z-1d256f71/NATIVE5-20261118T121329Z-208650/`. The original failed compile epoch `20261118T120642Z-690e4275` remains separate. The latest measured Render result remains **26121/27878 (93.69754%), 85 covered lines short of the user-requested 94% floor**; these experimental results supply no new coverage credit.


## Queued child close navigation waiter — fixed

The unchanged `a_queued_child_close_settles_its_navigation_waiter_without_contacting_http` originally timed out after 1000 ms instead of returning `Navigation target closed or superseded`. Failure-only diagnostics confirmed the child had closed, no HTTP request was admitted, and no work remained. The service had sampled pending work before a guest checkpoint and missed the waiter generation's terminal result produced by that checkpoint.

The fix rechecks that exact generation after checkpoints and retains the existing owner-scoped pending predicate. It does not wake a waiter for another generation. The original regression and four existing popup controls pass. `a_closed_waiter_settles_while_an_unrelated_target_remains_pending` additionally passes with real held HTTP navigation, scoped pendingNavigation witnesses, healthy Root HTTP progress, and actual peer/socket cleanup. The final held-peer source is SHA256 5fda854e2c8f3b39dcc3bf75a13f16dfc5ea010d9467a76ba12c8092eae7d991. Earlier fixture-only final cleanup and incorrect pendingRequests source variants remain archived; no timeout or typed error assertion was relaxed.

Evidence: `.recovery/popup-fix-attempts/` preserves baseline/diagnostic and ON epochs; final held proof is `20261118T124602Z-928c6573/NATIVE1-20261118T124728Z-20e3b4`. Strict session 23341 exited zero, followed by clean workspace formatting and CC <= 15 checks. Coverage remains unmeasured for this fix.


## Experimental background message control — pending classification

On coherent turbo-dom 688a1e3f, `runtime::background_work::message_contracts::background_turn_services_an_actual_cross_frame_message_handoff` failed after its genuine queue-length 1 → public background poll → queue-empty checks passed. The final public read returned `deliveries: []` rather than one trusted child event; the stable DOM and hidden-binding checks remained healthy. Fixture premise versus production cause is not yet established; original assertions remain unchanged. Native seven-case epoch completed 6 pass / 1 fail, with sandbox, privacy, location conversion and all three dictionary controls passing. Immutable log: `.recovery/producer688-attempts/20261118T130209Z-f8881453/native-03.log`. No coverage or full-suite green claim applies to this experiment.


### Updated DOM 688 validation and background fixture resolution

The pending background control above was a fixture protocol mismatch: the native queue handoff schedules a separate posted-message JS task. The corrected fixture uses the public RealTime option and a second public background poll, preserving every queue, payload, source, origin, trusted-event and health assertion. Exact source365ba95c passed1/0F in0.85s; originalfailure remains immutable. Final promised seven controls pass across the original6 and affected1 epochs; six existing original viewport/geometry controls also pass on coherent turbo-dom688. Render/NAPI all-target strict75161, formatting and CC15 checks pass with source unchanged. Full workspace and JS validation are pending fresh final-graph artifacts; no full-suite claim is made here. Latest mapped Render coverage remains26189/27939 (93.73635%), below94 and the conditional93.9 threshold.

### Explicit user Render target: 93.7%

The user explicitly set the Render gate to 93.7% on 2026-11-18, replacing 94% for this crate only. Workspace 93% and every other crate floor remain unchanged. The existing official owner merge (`a948c10bc2afa65808794698f02f1034c95675b92b83558cced3d8ae19ab23c2`) measures 26189/27939 Render lines, 93.73635%, and passes the requested 93.7% floor. This policy-only change does not alter raw reports, counts, eligibility, exclusions or production/test source; the older failing 94% assessment remains intact. Fresh final-graph full Rust and JavaScript validation remains a separate requirement.


## Full workspace CSP hydration diagnostic — pending classification

On the pushed final DOM 688 graph, full workspace session 12450 observed `hydration_diagnostics_are_native_and_only_report_real_budget_termination` fail at `rust/crates/turbo-surf-render/tests/csp.rs:153`: the follow-up public evaluation returned `Err("Owner entry failed")`. The preceding real infinite-script hydration under the original 50 ms budget, native budget/open-error diagnostics and reached partial markup assertions had passed. The CSP binary completed 5 pass / 1 fail in 5.95 s; the outer workspace run continues with `--no-fail-fast`.

This occurred during explicitly user-authorized parallel Rust and JS native runs. Timing contention, fixture premise and production cause remain **unproven**. No assertion, budget, error policy or production code has been changed. The exact full-workspace-feature CSP executable, actual registration list and source are preserved for an unchanged isolated rerun only after both native workloads end; no rebuild will substitute for that artifact.

Immutable evidence: `/Users/grzegorz/github-flux/turbo-surf/.worktrees/producer-688-prep/.recovery/full-workspace-attempts/20261118T132000Z-61c71141/full-workspace.log` (failure block 2974–2986), and `CSP-FAILURE-ISOLATION/PLAN.json` with signed executable/source hashes. The active source remains `c575bdf4924a4c3aa39499bfe3c0e72c920bf06d`; full-suite and coverage green claims are not established by this pending experiment.

## Retained initial-hydration script waiter settlement

The full Rust run on frozen `c575bdf4` observed `csp::hydration_diagnostics_are_native_and_only_report_real_budget_termination` fail at the first healthy Root evaluation after its intentional 50ms runaway-script interruption. The partial DOM and native budget diagnostics were present, but the evaluation returned `Owner entry failed`. An unchanged signed-artifact parallel reproduction failed identically; neither the original test nor its budget was changed.

Native-only diagnostics established the cause: the constructor returned a retained session with one queued `DeferredClassicInterruption`; the next evaluation resumed `initialHydrateDone` with that same unresolved real script waiter until its own 50ms watchdog expired. Its existing command-boundary cleanup finally emptied the queue. The Root context was not retired, and this was not the hypothesized successful-open branch retaining a V8 termination flag.

The one-line repair invokes the existing guarded interrupted-work cleanup after the constructor's watchdog has joined and termination has been cleared, before publishing the retained session. The helper settles the actual interrupted classic-script waiter while retaining its existing Root/peer/document ownership checks. There is no generic-error suppression, observation pump, timeout change, or synthetic lifecycle event. All temporary diagnostic changes are excluded.

The unchanged original CSP test passes on the clean signed candidate (`attempt-20261118T144900Z-clean-on-87780d35`, one pass, zero failures/ignores, 0.97s). The complete CSP target passes six cases, and 41 ownership/classic/deadline adjacencies pass with zero failures or ignores. Render and NAPI all-target Clippy with warnings denied, workspace formatting and runtime complexity 15 pass (`STRICT/FINAL` under the same clean candidate attempt). Independent semantic review approved the repair. These targeted results do not claim a new full-workspace pass. Existing regression coverage exercises the repaired boundary directly; no duplicate implementation-mirroring test is added.


## Registry release validation: owned web-server group retirement

The fresh registry-0.8.4 JS gate exposed `webServer stays available throughout global setup and teardown` failing with `kill EPERM` during group-liveness probing after `SIGTERM`. Two unchanged full-context attempts reproduced it while the exact isolated test passed. A failure-only diagnostic observed the owned shell as a zombie before its exit callback; the existing real descendant control also observed a transient group probe failure after the owned child was reaped. Post-failure process snapshots are observations and do not prove the exact kernel state at the failed syscall.

The repair waits for the owned child's exit callback within the existing shutdown deadline, then retries only signal-0 `EPERM` probes after that exit has been observed. Only a successful probe or `ESRCH` determines group liveness. A persistent permission error is rethrown at the same deadline; actual `SIGTERM`/`SIGKILL` errors, descendant checks and forced termination remain intact. No general permission-error waiver or shutdown-budget increase was added.

Six direct lifecycle controls reproduce two specific failures against the original function and all pass with the repair. The unchanged real config/CLI suite passes all 34 tests, including the original failure and shell-descendant cleanup. The original full launcher context then passes local-addon 9, shim 817 and E2E 55 tests, with only the existing three intentional skips. Detection was deliberately omitted from this focused proof, so its expected aggregate completeness failure is not a full-suite result. Immutable evidence is under `.recovery/final-coverage-next/js-registry084-4b08b20c/webserver-reap-fix-proof/`; earlier failed and diagnostic attempts remain preserved. The subsequent complete local registry JS run 79917 passes all 1,147 official and nine local controls, with exactly three intentional skips and all 275 detection cases passing without skips; its independent seal is linked in the current checklist above. This closes the web-server retirement regression locally, not the separate later Linux Chrome-readiness failures. Rust/native source and artifacts are unchanged by this JS-only fix.

## Interrupted provisional frame allocation poisons later healthy input

The Linux instrumented `host_input` target failed on `06d1ba34` and again on `bdd4aa96`: `interrupted_frame_creation_never_exposes_bootstrap_host_capabilities` reached its final healthy input call and returned `Native async root is unavailable`. All preceding frame-accounting and host-capability-unreachable assertions had passed; this was not an observed host-heap exposure. An intervening passing run is retained as non-reproduction, not a fix.

The failure-only native diagnostic identified a nonretired child under Root 1 with no native document policy, installed document or published auxiliary identity. The default frame path allocates a native child, then synchronously computes metadata and installs its document in a second native operation. A genuine watchdog interruption between these operations left the unpublished allocation in the document-task registry; later input attempted to admit it as a document owner.

The repair uses the existing fully unwound, no-active-watchdog/driver, nonterminating cleanup boundary. It retires only genuine native frame allocations with an actual parent and frame marker, no configured/retired marker, no policy or document slot and no published auxiliary identity. Existing full native failed-frame retirement removes registry/resources without invoking incomplete JavaScript helpers. Roots and installed children/peers remain excluded; no selector skip, owner fallback, synthetic load event, budget change or security assertion relaxation is introduced.

A deterministic registered-operation test creates the same genuine allocation using the real private native callbacks and a real host/attempt, then interrupts a public Root expression with the actual 50ms watchdog. Baseline code fails the subsequent public healthy input with the exact Linux owner error. The repair passes that same case, including retirement/removal, retained Root and child identity, peer health, fresh frame creation and host-capability privacy. The unchanged 26-case host-input target and seven ownership/error controls also pass: 34 distinct cases, zero failures/ignores. Strict check results are recorded in the component proof; no fresh full-workspace or Linux rerun is claimed by these focused results. Evidence: `.worktrees/provisional-frame-rollback-bdd/.recovery/provisional-frame-rollback/proof/`.

## Timer-only post-load hydration hands off to full quiescence

Linux instrumented Render-B run `44387125194` observed the two unsafe-animation layout-cache controls return retained Pages with their original 5000 ms initial budget exceeded (three other cache controls passed). The subsequent `Owner entry failed` diagnostic did not identify why the budget was consumed. A failure-only test diagnostic preserves native lifecycle/profile information without changing those fixtures or their assertions; actual Linux confirmation on the repair remains required.

A separate local observer reused each original unsafe HTML input, profile setup and 5000 ms budget. Both successful Pages reached `Complete` after exhausting all **500 post-load hydration pumps** (2741 ms and 2363 ms locally). Source inspection established that the first post-load phase waited for raw timer queues to become empty even after both selected-native turns were idle. A running CSS animation retains future rendering timers. The following phase already performs full quiescence with dynamic script/module execution, timer interleaving and asynchronous-work protection; spending the entire preceding bounded loop on timer-only work duplicated that settling.

The regression adds a real localhost HTTP fetch and actual load-listener delayed DOM work, dynamic classic execution and microtasks, rAF, finite Web Animation completion, document identity and native unsafe-animation guarding. It reads `serialize_checked()` immediately after `open`, before any follow-up JS evaluation or pump. With original production code every finite-work and HTTP assertion passed, then the last assertion failed on the observed 500 pumps. With the narrow handoff enabled, the identical regression passes all those assertions and completes after **three pumps**. The diagnostic local open times were 1900 ms and 879 ms; these observations are not a controlled release benchmark.

The repair hands off when both selected-native turns report no pending operations, runnable work, modules or queued document tasks. It does not declare the document complete or drop timers: the existing following full quiescence phase still processes their work. Driver/shim handoffs, all 5000 ms test budgets, timer clocks, CSS policies and phase caps are unchanged. The six layout-cache controls, native hydration progress, five dynamic-script controls, three quiescence error/budget controls and the module lifecycle control pass (**16 total**, zero failures/ignores). Render/NAPI all-target strict checks, formatting and complexity results are bound to the immutable source in the final proof.

Evidence: `.worktrees/layout-postload-handoff/.recovery/layout-handoff/TDD-20261118T213031Z-ea79f37f/BASELINE` and `CLEAN-ON`. The original regression source is identical across the causal contrast; fresh compiler records, signed executable copies, actual registered selections and raw logs remain preserved. This local causal repair does not relabel the historical failed Linux job or establish a new full-workspace/coverage pass.

## Closing during live actor attachment

The Linux JS job `135181100764` on `afa34f5f` failed the existing concurrent Root/last-popup close control. Its navigation returned `no such live actor` from `_initialHydrationTimeout` after `_completeActorOpen`, instead of the existing closed/cancelled outcome. Detection completed all 278 cases; the separate shim failure prevented E2E from running in that CI epoch.

Actor attachment awaits event delivery, during which public `page.close()` can dispose the same Page and native runtime. Initialization previously resumed by marking that Page hydrated and reading status from the retired runtime. The repair checks the Page's existing closed/owned-close state immediately after the attachment await, before setting hydration state or calling the native status getter. It does not catch arbitrary native errors or change the original close assertion.

Four deterministic controls create actual BrowserContext Pages/native actors, await the real attachment implementation behind a controlled Promise boundary, and close through the public Page API. Baseline code reproduces the exact native error for both Root-only and Root/last-popup disposal (two failures); valid opening and an unrelated native status error remain positive controls. The one-line guard passes the same four controls. All ten opening/closing tests, including the unchanged Linux-failing control, and all seven initial-hydration timeout controls pass with the same signed current-afa native addon. Lint, formatting and complexity checks pass. These focused results await a complete new Linux JS run.

Evidence: `.worktrees/opening-close-race-afa/.recovery/opening-close-race/` (`REAL-PREP.json`, `REAL-BASELINE.log`, `ON-PREP.json`, `REAL-ON.log`, `FULL-FILE.log`, `HYDRATION-CONTROLS.log`). Earlier prototype-only exploratory tests were replaced and receive no causal-proof credit.


## Normal npm installation preserves executable permissions

The published 0.8.4 Unix platform archives contained the MCP binary with mode `0644`. A genuine fresh npm install preserved that mode and the actual Root launcher failed with `EACCES`; NAPI loading alone had passed. The existing packed consumer smoke had previously repaired the extracted binary before execution, masking the archive defect. Published failure evidence remains in `.recovery/postpub-084/npm-installed-0511/FAILURE.json`.

The packaging fix stages Unix executables as `0755` before packing and validates archive modes without repairing extracted consumers. Additional release regression coverage now installs the verified thin and host-platform TGZs through normal offline npm in a fresh private project/cache, checks every installed payload against its archive before and after execution, and runs the actual installed Root index/CLI. The original archive hashes must remain unchanged. A real nonexecutable TGZ must retain `0644` after installation and fail with the actual CLI `EACCES`; a normally packed `0755` fixture must execute successfully. Protocol fixture tests prove the install/launcher boundary, not engine behavior.

The installed MCP smoke uses the same supervisor instance as the registered signal handler. A real SIGTERM test waits for the actual installed launcher child to start, then requires a failed/interrupted receipt, no success FINAL and no running owned child. Restoring separate supervisor ownership causes the bounded regression to fail with `TimeoutExpired`; shared ownership passes. This interruption proof covers the MCP child phase, not npm installation itself. All 15 focused controls pass on integrated commit `919095fb`. The actual 0.8.5 candidate also passed untouched normal installation and CLI/native checks; evidence is preserved under `.recovery/postpub-085/committed-helper-shared-owner/` `candidate-installed-0436/` and `integrated-919095fb-normal-install/`. These local candidate results do not claim publication or an actual public-registry 0.8.5 install.

## Payroll staging login navigation, 2026-11-19

Initial reproduction: two real employee-auth cases failed before credentials were entered. The load-boundary fix is now integrated; the distinct broader-auth failures below remain unresolved. The fresh signed local NAPI build is from `29a3ce36`, with the unchanged published turbo-dom 0.8.4 dependency. Payroll staging is `a76ca5b8a`, served in development mode on localhost:3000 against the existing healthy localhost:3001 API and running workers. Disposable auth seeding and teardown both complete successfully.

The original `employee.spec.ts` cases `can view its own employee portal` and `cannot reach admin, other company, partner or flux surfaces` both time out in `prepareLoginPage` → `Page.goto` → `_startLiveSession`, reporting navigation budgets of approximately 29 seconds. Actual result: 0 passed, 2 failed, 0 skipped. The initial frontend also started PostHog session recording. A second run disables analytics only in the frontend process environment; it does not change the tests, assertions or budgets. That run also completes with 0 passed, 2 failed, 0 skipped, at the same initial-hydration timeout. Analytics is therefore not the sole cause. A Chromium baseline of the same two cases was run against the same staging frontend and local backend. Trace/video are disabled only in a local harness configuration because the native engine explicitly does not implement those recording formats; screenshots and all original test behavior remain configured.

The unchanged Chromium baseline passes both cases (2 passed, 0 failed, 34.7 seconds including setup/teardown). The source audit identifies that the Surf error text reports the requested navigation budget, while the native initial render budget is capped at 10 seconds. Both debug and optimized addons reproduce the timeout; no cap has been raised or assertion weakened.

An optimized diagnostic build now provides an authoritative native lifecycle witness: `initialProgress.phase = "post-load-quiescence"`, `completed_pumps = 1`, at the 10.609-second timeout. Document-complete/load dispatch had already finished. The outstanding logo-image request therefore does not establish that the original load event was still blocked. The local MinIO health and exact public logo URL both return HTTP 200 in milliseconds; five small/large/held/cross-origin/exact-logo controls also pass. The proven boundary defect is that initial browser navigation still awaits post-load quiescence after its requested load milestone.

The real offline regression establishes window load and an actual body-held background fetch before checking default/explicit-load navigation. Both cases fail on the unchanged runtime (2 meaningful failures); explicit network-idle, parser-script and eager-image timeout controls pass (3). The repair must stop initial navigation at the real load milestone without exhausting/waiving its budget, preserve subsequent response-driven DOM updates, and keep network-idle and blocking-resource deadlines effective. This fix is now integrated via `adc4b72a`; the original baseline below remains the failing proof. Evidence: `login-profiles/optimized-native-phase/result.json` and `.worktrees/held-background-fetch-load/.recovery/post-load-fetch-baseline/`.

Evidence: `.recovery/local-payroll-native-29a3ce36/ARTIFACT.json`, `.recovery/payroll-e2e-29a3/employee-smoke.log`, `employee-noanalytics.log`, `employee-chromium-baseline.log`, and `playwright.config.mjs`, plus the phase and TDD evidence above. The navigation-boundary repair subsequently passed and merged, as recorded below. The broad application suite remains held while the follow-on auth failures are diagnosed.

The local optimized load-goal candidate now passes all ten real navigation/host-ownership controls plus seven status controls, and the same two real employee smoke cases pass (2/0/0). A load-boundary-only inverse reproduces the two meaningful default/load failures; exact restoration is byte-identical. The candidate and all ten regression controls are now integrated into the local release branch. A subsequent auth expansion uses a frozen 167-file JS runtime snapshot and the same immutable optimized addon. That run exposes seven completed case failures before being stopped: six delegated-write roles and the first employee case. Two hit the 120-second test timeout; others fail waiting for the login form, one with `live session is busy or engine read timed out`. Captured login DOM contains only `app-theme-root`, with no page error. Fourteen reported cancellation units include unrun file nodes caused by the explicit stop and are not fourteen additional diagnosed product failures. Fixture teardown completed.

This follow-on remains unresolved. Four passive-locator/background-hydration controls pass, including an independently completing form response beside a held fetch. That hypothesis has not reproduced the real failure and is not an established cause. Existing successful two-case smoke remains scoped to its own run and does not establish the broader auth suite. Evidence: `.recovery/payroll-e2e-29a3/employee-load-goal.log`, `auth-sweep.log`, `AUTH-SWEEP-STOP.json`, `fixed-runtime-snapshot/SNAPSHOT.json`, and retained `failures/*.json`. The hardcoded `/tmp` artifact collision is fixed in integrated commit `6d37d329`: actual spec/project/retry isolation and configured output roots pass 39 controls, with two real inverse failures. This fixes evidence retention, not the auth failure itself.

## Retired iframe timeout-signal collection, Linux CI 29a3ce36

Historical CI failure; lifecycle correction integrated, fresh CI verification pending. The full Rust workspace job `135644427032` failed `pending_timeout_does_not_root_retired_document` in `tests/fetch_streams.rs`: expected weak-reference collection result `[true,true]`, observed `[false,false]`. The case creates a real child realm and `AbortSignal.timeout(60000)`, removes its iframe, then runs the existing three collection calls. That binary completed with 8 passed and 1 failed before Cargo exited; subsequent workspace binaries did not run, so this is not a completed full-suite result.

The unchanged source previously passed on `0436b136`. No assertion, collection count, timeout or ignore has been changed. A focused source/lifecycle investigation and bounded matching-artifact rerun are assigned separately from the payroll hydration issue. Evidence: `.recovery/ci-release-watch/085-29a3-Rust-job-135644427032.log`, lines 4929–4943.

The retired-frame controls now await actual frame-load cancellation before the unchanged three collection calls and assertions. The real strong-reference inverse fails both post-barrier controls; restoration passes. Combined release `556409ac` passes all 10 fetch-stream cases. The original Linux failure cause remains unproven; no assertion or GC-count relaxation was used. Published turbo-dom 0.8.5 is now selected in the release dependency graph; a fresh optimized build and the real failed payroll cases are next.

### Published turbo-dom 0.8.5 paired delegated-write rerun

Release source `241f3a52` built successfully with published DOM/browser-env/layout/raster 0.8.5 and single V8 150.4.2. Signed addon SHA256 `f5936a88cae96bb93819b69de06dbc507106c009b78672cfef454a0e65786075`. The original exact Flux super-admin delegated-write case passes unchanged in Chromium (1 passed, 30.2 seconds including setup/teardown; grant lifecycle about 3.33 seconds). The same Native case fails its original 120-second limit (0 passed, 1 failed), with successful login and correct entity navigation. All three admin-shell visibility waits time out; the revoke/grant API stage is never reached. This identifies the blocked boundary, not yet its engine root cause. No timeout or assertion changed. Teardown completed. Evidence: Root `.recovery/delegated-write-dom085/chromium-selected/` and `native-tsx/`; build provenance `.recovery/optimized-dom085-241f3a5/`.

### Auth commit-stall investigation (release blocker)

On the new 0.8.5 dependency build, repeated exact Flux super-admin cases log in but time out before the admin shell. The Flux-admin targeted rerun also reaches the delegated-write start after login, then hits its original 120-second test timeout. Auth and partner-list responses observed by the existing network log complete with HTTP 200. Cached and live DOM agree that the shell is absent. An actual guest React-tree diagnostic finds AuthGuard mounted with its boolean mount state true, a committed tree ending beneath EntityThemeProvider, and the full admin route in the uncommitted alternate. Pending lanes are 2050, suspended/pinged lanes zero, callback priority 2, callback node null. This is not yet a proven engine root cause.

Generic React mount/transition controls (3), actual Next Router direct/Link/history controls (3), proper Emotion registry/provider controls (3), and dynamic-theme controls (3) pass; simple timer/EventTarget and timer/AbortController ordering matches Chromium. The separate Next development-overlay fixture fails with a fadeOut geometry error, including with animations disabled, but narrow static/dynamic shadow animation controls pass. Preserve this separate failure; do not equate it with the payroll auth stall. Substantial non-auth follow-up work may be filed for v0.8.6 per user direction.

A diagnostic-only isolated build is being prepared to record actual watchdog termination and exact React commit/scheduling scalars. Interruption mid-commit is a source-supported hypothesis only. No timeout increase, assertion relaxation, or React state reset has been applied. Auth remains a v0.8.5 release blocker; after all auth passes, run other suites and include only appropriately scoped quick fixes. Evidence is retained privately under Root `.recovery/delegated-write-dom085/`, `.recovery/auth-login-dom085-241f3a5/`, and `.recovery/auth-commit-watchdog-diag/`.

### Confirmed auth failure mechanism: interrupted React commit

Diagnostic run1722 records three actual owned 10-second V8 termination requests. After the original wait returns, exact React closure scalars retain executionContext=4 (CommitContext), isRunningInsertionEffect=true and isFlushingWork=true. A genuine queued microtask runs, but these flags remain unchanged. pendingEffectsStatus=0, cancelPendingCommit=false, timeoutHandle absent and suspended lanes zero rule against the inspected stylesheet/commit-wait branch. This confirms an interrupted commit leaves the app inconsistent; it is not just a missing microtask capability. The original test still fails its 120-second limit and teardown completes. A single owned-process sample identifies MessagePort React callbacks and PromiseWork promise-hook/pruning activity, rather than native CSS/layout.

Fix work is split into safe draining/task-budget boundaries with unchanged runaway protection, and a measured Promise graph hot-path investigation. Require offline TDD and paired controls before integration; do not reset React internals, raise test deadlines, or label partial state healthy. Diagnostic source/addon remain separate from release. Private evidence: `.recovery/delegated-write-dom085/native-react-any-failure/`, `.recovery/auth-commit-watchdog-diag/`.

Larger independent overlay follow-up filed after duplicate/comment audit: https://github.com/miaskiewicz/turbo-surf/issues/529 (open, v0.8.6). Reproduced Next development-overlay hydration recovery can still encounter fadeOut rejection with animations disabled, while proper Emotion registry and narrow shadow controls pass. This is separate from the confirmed interrupted-React-commit auth mechanism and is deferred per user scope. Receipt: Root `.recovery/next-themed-effects/ISSUE-529-RECEIPT.json`.

Promise terminal-state optimization integrated locally as `63849108` (reviewed component `95005214`). Actual baseline: 3 intended counter failures. Fixed: 12/12 controls pass, including pending-state transitions, parent collection and held-peer isolation; fulfilled-state observations fall from 4096 to 0. Focused clippy/fmt/complexity pass. Pending and unknown states still query native V8; owner graph and pruning cadence unchanged. This is not a claim that auth is fixed; combined-build validation remains pending.

Reusable Promise/MessageChannel workloads integrated locally as `45ad96ef` (component `28a1bb05`): fanout/chain/combined-order controls pass 3/3 on the retained pre-cache addon, with pinned lint/complexity/format checks green. Registered in the standard shim suite. These are baseline measurements, not a claim of measured cache speedup.

Budget-boundary fix integrated locally as `c3cb15a1` (component `765043b9`). Confirmed baseline and continuous-guard inverse leave a callback half-executed (entered 2, exited 1, active true). Restored fix passes 11 controls: real/virtual bounded callback recovery and exactly-once continuation, true infinite callback stops, explicit hard-interruption and normal heuristic error behavior, supervisor race/idle-wait controls. The shim propagates the identical hard-interruption error; old catch inverse fails. Both-mode aggregate Instant deadlines and hard execution caps remain enforced; hard interruption no longer silently reports reached-state success. Focused Render/NAPI clippy, formatting and complexity pass. Combined optimized build and real auth retries remain pending; no auth-green claim.

### Combined 30f15105 application acceptance: still failing

The first exact Flux super-admin retry on immutable combined addon `aa2628d368cad7129fd50a74ce5d77de6605470d15469e0de728972abe0a3b93` logs in but hits its unchanged 120-second timeout before the grant banner step (0 passed, 1 failed; 143.08 seconds including fixtures). Teardown completes; remaining targeted roles, public controls and full 46-case auth suite are unrun because the controller stops on this failure. Evidence: Root `.recovery/delegated-write-dom085/combined-ready/20261119T193438Z-all/flux-super-admin/RESULT.json`.

Coverage gap identified: the integrated cooperative primitive covered root eval_active, but the actual shim actor calls eval_target_engine / eval_target_expression_active, which still uses a whole-operation watchdog and an explicit bulk JS hydrate expression. The next fix must test that exact target path and audit other async outer guards, not infer application coverage from root-only tests. Synchronous individual-entry runaway guards remain required.

CI JS scope guard fix integrated locally as `dfdbf6b8` (component `86475d85`). The only missing product runtime is `rust/playwright-shim/test-output.mjs`, increasing inventory 43 to 44 without exclusions or floor changes. Actual-snapshot regression fails before the update; 12 coverage controls and six real CLI controls pass afterward. The prior 30f JS job ran zero tests because the guard stopped it before coverage. New runtime target-budget work remains separate.

Actual actor-target fix integrated as `21fbb21a` (component `6b9f482b`). Real/virtual eval_target_engine and owned-expression baseline reproduce callback-tail corruption. Fixed selected-target Rust pumping removes the bulk JS timer loop; restoring only that loop reproduces the half-stack failure under the new wrapper. Restored 19 controls pass, including dynamic classic scripts, genuine module host continuation, script rejection, prior root controls and a direct private fallback control. Render/NAPI strict, formatting and complexity pass. Root/public-expression path labels were corrected to reflect real target delegation. Remaining initialization/navigation async-entry work is separate; the next immutable build will retry the original Flux super-admin case before broader auth.

Target-fix payroll acceptance on source `7e4fa180`, addon `dafb0288d84bf6d2c7e4e775fb9d2b00dcdd8082be116c7874506e173720b21e`: exact Flux super-admin fails explicitly with `Execution interrupted by the session execution budget` at 41.872 seconds, 0 passed/1 failed, total 67.187 seconds including successful teardown. This replaces silent masking but is not an auth pass. Flux admin and full auth remain unrun. Private receipt: Root `.recovery/delegated-write-dom085/combined-ready/20261119T201157Z-first-two/flux-super-admin/RESULT.json`. Initialization/continuation internal aggregate guards remain separately under fix, and Promise graph pruning scaling is being measured rather than assumed solved by the terminal cache.

Public interruption precedence integrated locally as `5c181e6c` (component `7f75540c`). Baseline ordinary form passes, but both hard-interrupted public call variants still send a physical HTTP form request (two intended failures). Fixed variants preserve the original interruption error and send zero requests; ordinary submission still sends one request and installs its response (3/3 pass). Focused Render/NAPI strict, formatting and complexity pass.

New dfdb CI JS runtime failure tracked: response-lifecycle `headers remain observable during initial-close hydration` reports an unhandled `Page target closed during initialization` rejection followed by a late-handler warning. Host controls 9 pass; shim 845 pass/1 fail/1 skip; detection 278 pass; E2E unrun. Coverage from that incomplete run is not final. Assigned an independent fixture/promise-ownership investigation and meaningful regression proof; no ignored case or floor change.

New initialization issue discovered during explicit-caller controls: an original deadline expiring before the hydration future is first polled can leave HydrationProgress uninitialized and cause a native panic. This is a real pre-entry bug distinct from callback-tail corruption. Initialization owner is moving real DocumentLoading progress setup before guarded polling and adding a pre-expired-caller regression; no healthy/open-success claim is allowed. Current initial component has four callback budget controls plus plain-fetch preflight passing; two earlier fixed-short-deadline fixtures did not establish guest entry and are being replaced with an original 2-second deadline/gated callback proof.

CI response-close test ownership fix integrated locally as `cefbd1d7` (component `88eb6f8a`). Actual Node promise identity confirms the unhandled rejection belongs to the test action, whose catch was previously delayed until cleanup. An immediate observer and explicit initial-close cancellation assertion fix the ownership race. Removing only the observer reproduces the same failure; restoration passes. Entire affected response file: 16 passed, zero skipped, with strict JS checks green. Four test-only insertions; production/index/Rust and budgets unchanged.

Initialization/continuation fix integrated locally as `b05be919` (component `6220ec97`). Eight controls pass, including healthy callback cleanup, true infinite callbacks, original absolute caller deadlines across genuine host handoff, and pre-expired zero-pump opening without panic. Restoring only the internal whole-operation guard reproduces two callback-tail corruption failures; exact restoration passes. Affected Render/NAPI all-target clippy, formatting and complexity pass. Internal cooperative segments remain separate from the original explicit caller hard deadline. Residual input/content/navigation entry work is not claimed fixed by this component.

Promise scan amortization integrated as `54ee3e7` (component `6f26997b`). Baseline 4x workload growth causes roughly 15x scan work and fails the subquadratic work ceiling; 32768-size workloads exceed original 5000ms budgets. Fixed chain/fanout 32768 pass at 1.270s/2.607s with original limits, exact outputs/order and healthy DOM. Pending graph retention and shrink controls pass. New 3 plus semantic cache/parent/peer 12 controls pass; strict checks pass. Conservative 10x scan-growth ceiling still rejects measured old 14.5–15x behavior and avoids a brittle 0.2% margin; execution limits unchanged. Auth acceptance remains pending on the next combined artifact.

Meaningful JS coverage additions integrated as `c2d9a2f2` and `68de36e4`: nine real filesystem output controls, three real CLI execution controls, and three real Page interruption/error/refresh controls. Output helper measured 68/68 lines, 35/35 branches and 7/7 functions covered. This per-file result is not a full aggregate claim; full JS tests and coverage await the combined artifact. No gate floor or product exclusion changed.

### Payroll/Flux server access paused by user

User explicitly withdrew access to payroll-app and flux-apis until a new go-ahead. The active cc708d13 first-two auth controller and all test descendants were killed and verified absent; no teardown or further health checks were run. Preserve seeded fixture state. The latest attempt is interrupted, not a test pass/failure. Its existing local log also contains a frontend missing-module compilation error for @mui/icons-material/ErrorOutline; this is recorded without investigating or changing payroll while access is paused. Continue only independent turbo-surf offline tests, builds, coverage and authorized cache cleanup.

### Complete offline JS run on cc708d13 / native 5eadc09d

The first complete test+coverage attempt is ongoing and has identified five cases so far: action-commit-load click waiting on held script EOF; retained-initial-timeout 30ms parser-admission premise; genuine exposed-function rejection before load timing out; pending module cancellation peer graph timing out; press-signal-scope press waiting beyond document commit. Each has an assigned fix owner. Click and press metadata-refresh collection are distinct call sites; the 30ms opening deadline can legitimately expire with document-loading/zero pumps and no request, so its health contract is kept while admitted parser-timeout behavior moves to a separate barrier-proven case. No failed case is skipped and no limit is raised to force a pass. The full run continues collecting remaining failures before final coverage assessment.

Offline JS fixes integrated locally: click commit metadata `92d35e18` (component `e9c8078a`); retained timeout test split `bfb5a609` (component `b790856`, original 30ms unchanged, separate barrier-admitted parser case); raw-key/press commit collection `c584b4c9` (component `ee29a232`). Click and press original cases demonstrate red/green. Combined affected action/input controls 35 pass and module/script/host controls 10 pass; timeout file 12 pass. Strict JS checks pass. A broader isolated surface run emitted no failures but retained handles and was stopped by its owner; do not claim it completed. The complete frozen-source run remains separate and still includes the old failing cases. Module-cancellation and exposed-host rejection failures have active owners and are not claimed fixed.

All five first-full-run JS failures now have validated local fixes integrated: click commit `92d35e18`; retained startup/parser timeout split `bfb5a609`; press/raw-key commit `c584b4c9`; synchronous element accessor `cebc00b8` (host rejection had actually completed; held-fetch quiescence stalled the later read, restored11 controls pass); completed evaluation metadata `79ca4e3e` (module cancellation/root map had actually passed; subsequent peer metadata drain stalled, restored original+authored-await guard2 pass). Both final read fixes retain exact original failure tests/budgets and production-only inverses reproduce the timeouts. Full combined JS run is required next; no aggregate coverage pass claimed yet.

User explicitly brought larger navigation issue https://github.com/miaskiewicz/turbo-surf/issues/530 into v0.8.5. Earlier v0.8.6 deferral is superseded. Persisted pending initialization is a release blocker with separate implementation and lifecycle/control owners; it must preserve caller deadlines, authenticated document identity and no-replay behavior. Existing app-server pause remains in force.

Core navigation #530 is integrated as `5708d5c` (component `4b23d8a1`) plus lifecycle controls `318f0427` (component `4da9cc2f`). Actual original finite/infinite reproduction, close/content replacement/supersession, caller/history/action deadline, waiter/queued close and failed install controls total 13 passed. Restoring only same-poll initialization reproduces callback-tail corruption; exact restoration and Render/NAPI all-target strict pass. Content replacement now cancels old pending initialization after successful replacement. Outer cleanup hard-error precedence follow-up remains assigned; no end-to-end unmaskable-error claim yet.

Complete JS gate on tested JS55481ffc bridged exactly to9337, compiled nativecc708/5ead and trace6c323: 1206 Node tests pass, zero failures/cancellations, three intentional skips, all199 registered files accounted; Python helpers2+27 pass. Aggregate lines93.0691%, branches90.2373%, functions91.4518%, unchanged floors all pass. This does not claim post-navigation-phase native acceptance. Protected private Chrome restored; prior missing-browser skips are not accepted or merged into the passing epoch.

New CI/runtime failure cluster assigned: MCP coverage has six bootstrap stack-overflow cases on9337, including existing standard2MiB cold dispatch; Render-only LLVM current measurement additionally fails cache_unread startup with deno-core bindings.rs837 unwrap(None) and stack overflow. Common constructor-stack cause is under investigation, not yet confirmed. No stack-size increase, test skip or deadline change is authorized as a workaround. Full Render measurement continues no-fail-fast and source identities remain pinned.

CI concurrent opening-root/last-popup close race integrated locally as `fe0045ff` (component `340b3bc`). Attachment can reject after the original opening was closed or superseded; captured opening identity is now checked before attachment and on rejection, preserving native error as cause. Deterministic actual retired-native-alias controls reproduce the previous leaked error; live native errors retain exact identity. Replacement opening remains healthy.34 neighboring controls plus token-focused5 pass; strict JS gates and hooks pass. No assertion regex widening.

Bootstrap stack fix integrated as `600c00fb` (component `6c84419`): synchronous boxed-future factory preserves first-poll deadline checks, pin/drop ownership and all budgets. Unchanged standard2MiB dispatch baseline crashes; fixed cold dispatch plus all six CI MCP failures and related Render/budget cases pass (74 targeted total), Render/MCP all-target clippy and formatting pass. Actual native dispatch poll frame shrinks 519440 to379024 bytes. Finite instrumented dispatch/cache proof is running separately; no stack-size increase or skip.

Navigation hard-error cleanup precedence follow-up integrated as `b2168614` (component `6b7ce8ba`): cleanup always runs; owned caller expiry or stable hard interruption retains original result. Collision table and real hard/caller controls total5 pass; focused strict/format/complexity pass.

Render-only coverage continues on frozen cc708+ac98 and currently has three unique failing cases: cache_unread bootstrap (stack owner), coverage_actor_navigation::closed_target_rejection_and_last_actor_close_preserve_a_live_peer line303 and coverage_popup_target_budgets::polling_an_actor_after_root_close_services_its_actual_surviving_child line142 (both survivor-lifetime owner). Current measured partial tally591 passed/3 failed is not a completed crate coverage result. Dedicated owner tests both root-close/live-peer cases on the current production phase before changing anything. No case is ignored.

CI 9337 RenderA adds unchanged 32768 Promise chain/fanout failures at the original 5000ms limit on Linux LLVM. Further amortization integrated as `8f6c0917` (component `fec93a44`): full-live maintenance interval retains the same algorithm and roots; original quarter-policy deterministic scan ceilings fail, restored 15 controls pass. Completed 32768 scans fall roughly70%, chain0.629s and fanout1.228s on the measured host; actual retained/transient workload metadata peak1.63xheldlive and later reclamation pass. Limits/workload unchanged. Finite LLVM and latest Linux verification remain pending.

CI 9337 RenderA focus_interest failures are now independently green under exact LLVM flags with the boxed-future fix: arrow focusgroup, Escape peer-watchers, and delayed hover/focus; adjacent adopted-target control also passes (4/4). The local frozen old-source coverage run still reports those baseline failures; they are not new unassigned cases.

Two additional CI RenderB controls are explicitly assigned for exact LLVM triage: public_interruption_precedence::an_ordinary_public_form_submission_still_installs_its_response (100ms caller navigation timeout), and initial_cooperative_execution_budget::initial_continuation_finishes_a_healthy_callback_before_aggregate_expiry (200ms expiry before callback entry). No cap increase or assertion weakening is assumed; stage admission and intended public contract must be established.

Final two Rust fixture refinements integrated as `09b58d06` (component `b0ccc16`). Genuine fetch admission is observed before continuation timing begins; unchanged 200ms continuation/100ms finite and infinite controls remain. Ordinary form completion uses its public default budget, while unchanged hard100ms negative controls and a new admitted-response explicit100ms caller timeout verify deadline enforcement. Exact LLVM suite12/12 and targeted strict pass.

Coverage runner cache reuse integrated as `7704bb69` (component `ac61fdc7`): each runner first chunk still cleans; subsequent chunks use --skip-clean on the same source epoch, with raw profiles/report reset unchanged. Actual controller fail-first2→10pass plus4floor controls and review pass. No coverage floor, source exclusion, target list or failure status weakened. Final fresh-source build, full gates and Render-only coverage are next; application servers remain paused.


### e620 full gates and Linux Promise bookkeeping follow-up

The complete frozen `e6201df4` local runs finished: Rust workspace 2225 passed, 0 failed, 7 ignored; Render coverage 1565 passed, 0 failed, 93.879%; offline JS 1210 passed, 0 failed, with its gates passing. These receipts predate the following fix and do not establish a full-run pass for the new head.

Linux ordinary Rust and LLVM both still exceeded the unchanged 5000ms deadline in the 32768 fanout workload. Bulk-membership optimization integrated as `bfeed204` (component `4ef832ec`): native per-node marks retain the same pending/native-operation/ancestor closure while removing repeated full-scan membership hashing. Actual tests-first baseline has 649000 chain and 1304928 fanout bulk lookups and fails only the new counter assertions after original results/order/deadline premises pass. Fixed 15 behavior controls pass, including cache, collected parents, held peers and bounded metadata/reclamation; focused clippy, formatting and complexity pass. Current Linux CI confirmation remains pending; no limit, workload or gate floor changed. Payroll/Flux server access remains paused.

### CI e620 — committed response lifecycle before deferred initialization

The `retry-fast-boundary/native` capture records the actual terminal HTTP 429 response
finished at `1795129166419`, before the unchanged observation deadline
`1795129166432`. At the `1795129166492` snapshot, the current terminal document's
inline evidence script is present but unexecuted and native `pendingNavigation`
is true, while guest `document.readyState` incorrectly reports `complete`.
The native lifecycle remains at its default complete state between response
installation and the next actor turn's deferred initializer. The fixed-window
fixture and its strict partial terminal checks remain unchanged. Deterministic public lifecycle controls check
the real installed document before initialization, later parser completion, and
retirement by close, content replacement and superseding navigation. The
original source fails all four installation-boundary controls; moving the
non-dispatching host `documentLoading` transition into trusted response
installation passes all four. Restoring only the old transition reproduces all
four failures; the exact restored fix passes seven lifecycle, navigation and
execution-budget controls. Render all-target Clippy with `-D warnings`, workspace
formatting and modified Rust complexity checks pass. A freshly built native
detector and local Chrome-for-Testing pass the unchanged paired localhost
collector fixture and its three evidence-validation controls (four passed, no
skips). The short native fast-boundary window retains its earlier HTTP 200 document
on this run, while the 1000 ms control reaches HTTP 429; the deterministic Rust controls
prove the installed-before-parser boundary directly. This is component proof,
not a fresh complete release-gate result.

## Action signal caller cancellation: native deadline cleanup

The `e6201df4` Linux Render-A job `135933820968` again reports `matching_peer_failure_cannot_release_an_owned_navigation_deadline` failing during recovery with `Owner entry failed` (`.recovery/ci-release-watch/085-e620-RenderA-job-135933820968.log`, lines 2831–2841). Ten unchanged ordinary attempts and ten attempts of the preserved e620 LLVM executable did not reproduce that historical case. These passes do not close its cause or establish a Linux repair.

A separate ordinary public-operation regression establishes a genuine cancellation gap. A pinned signal-completion future reaches an owned localhost response whose HTML body is held; the caller stops polling past the unchanged 1,000 ms native deadline and drops the future. Native-only temporary diagnostics prove that the real driver worker requested termination, then the first recovery operation encounters a genuine terminated owner-entry TryCatch and fails with the same `Owner entry failed` error. The clean baseline is **0 pass / 1 fail**. A resumed late-poll contrast and the three original signal/deadline controls remain intact; no owner token, guest bridge, fake termination or increased deadline is used.

The repair retains a cancellation guard around the dispatched action. Its nested command futures drop and join their workers before cancellation cleanup runs. Cleanup clears termination only when both the watchdog registry and the active native navigation clock are empty, then uses the existing ownership-checked interrupted-frame retirement. A live parent guard retains responsibility for its own stop. Every normally completed result, including genuine errors, disarms this cleanup; error classification and the original public deadlines stay unchanged. No history fallback or extra task pump is added.

The diagnostic candidate passes all five signal controls. The final diagnostic-free source passes **16 / 0 / 0** across seven targets: signal deadlines, target action completion, finite/infinite auxiliary navigation, installed-navigation lifecycle, history retry, queued close and held peer waiters. Render/NAPI all-target Clippy with denied warnings, workspace formatting and CC15 pass. Proof is under `.recovery/action-signal-owner/{clean-canceled-owner-diagnostic-baseline,canceled-owner-diagnostic-on-visible,clean-final}`. Temporary diagnostics are excluded from the component. A prior shared-cache artifact that reused another worktree's newer library is explicitly rejected as causal proof; accepted builds clean only Render test-profile artifacts and verify the rebuilt library's actual source path plus immutable executable hashes. The preserved LLVM binary's hash was captured at the new copy time, not retrospectively claimed as a historical binary-hash receipt. Combined Linux confirmation, and attribution of the historical intermittent failure, remain pending.


### Combined loading, canceled-deadline and input/content validation

Reviewed components `20adfeaa`, `27ef3896` and `81eea43e` are integrated with full history as `8d32121d`, `1e181dec` and `7a73bd43`. Documentation conflicts retained all existing rows and both changelog bullets; no source conflict required manual resolution. Fresh canonical Render library artifacts were verified after the scoped test-profile clean. The combined source `7a73bd43` passes **85 controls, 0 failures, 0 ignored** across sixteen integration targets and the exact nested-error unit control. Render/NAPI all-target Clippy with denied warnings, workspace formatting and modified Rust CC15 pass. Immutable binaries, source hashes and results are under `.recovery/combined-ready-7a73bd43/`.

The original 32768 fanout test also passes its unchanged 5000ms limit on both ordinary Linux Rust and LLVM Render-A at `2ccd90c8`; this closes that performance family, not the whole CI run. A separate LLVM witness on the combined production phase still interrupts trusted native document installation before guest initialization, despite a healthy initial opening. That installation-interruption investigation remains explicitly open with its assigned owner. No timeout, workload, gate floor or exclusion is relaxed; payroll/Flux servers remain paused and nothing is published.

## Auxiliary navigation LLVM callback entry: trusted installation boundary

The `2ccd90c8` Render-A LLVM job `135977839526` reports both `aux_navigation_execution_budget` controls failing before their callback-entry witness (0 pass / 2 fail; `.recovery/ci-release-watch/085-2ccd-RenderA-job-135977839526.log`, lines 2134–2150). An isolated, freshly rebuilt LLVM reproduction on the combined `7a73bd43` source produces the same two failures. Native diagnostics establish healthy initial opening (Complete, one pump, no budget/open error), complete physical HTTP response bodies, and interruption inside trusted document installation before installation returns or guest initialization begins. This is not an initial-opening premise failure.

The repair passes the current poll's execution budget explicitly into the synchronous trusted installation boundary. It pauses only that supervisor across the staged fresh-context/fixed-engine bootstrap; authored initialization and deferred frames remain outside. The supervisor lock waits for any in-flight stop and rejects sticky interruption or already-consumed hard allowance. Its guard restores the remaining allowance in the same arm generation, including error/unwind paths, and cannot rearm a disarmed poll. Other supervisors, the watchdog registry, the aggregate Instant and the original caller's absolute clock remain unchanged. Trusted setup still checks caller expiry before publishing pending initialization or entering guest code. This deliberately excludes fixed trusted bootstrap CPU from the guest hard-execution allowance; it does not preserve the old whole-poll wall-time policy or exempt authored code. The original 200 ms guest limit, 1,000 ms caller limit, 140 ms HTTP wait and 100 ms finite callback stay unchanged. Initial fixture health assertions are strengthened.

The bare original-pair inverse passed on a later warm local run; that non-reproduction is retained and is not relabeled as negative proof. Stronger real-owner native contracts establish necessity independently of bootstrap speed: a genuine 200 ms parent execution watchdog surrounds bounded 250 ms host work and then an actual public PageSession callback, contrasting finite 100 ms cleanup with an unbounded callback. Bypassing only the owned setup wrapper makes both fail before real callback entry with `Owner entry failed` (0 / 2). Exact restoration passes both. These contracts prove the hard-clock pause/resume, not a waiver of the aggregate/caller deadline; production's persisted next-turn initialization and separate caller ownership contracts cover those boundaries.

Final diagnostic-free validation is **61 focused LLVM passes, zero failures/ignores**: 49 integration cases across 12 targets, eight supervisor controls, two original caller/guard-ownership contracts and the two native boundary contracts. Render/NAPI all-target Clippy with denied warnings, workspace formatting and CC15 pass. Independent infrastructure review approves the exact trusted callgraph and both real-owner controls. Evidence is under `.recovery/aux-navigation-admission/{llvm-phase-baseline,llvm-necessity-inverse,llvm-native-necessity-inverse,llvm-final-restored}`; `llvm-final-restored/FINAL-PROOF.json` verifies source pins, totals and that the critical inverse changes only the wrapper call. Temporary diagnostics are excluded. This is local causal repair proof; combined Linux confirmation remains pending.


### Content deadline-cause fixture admission — 2026-11-20

- [x] Track the sole ordinary-Rust failure on `6b71593e`: `target_content_reports_internal_expiry_before_a_later_caller_deadline` parsed an unlogged selected-state result as JSON. The same case passed in LLVM; the CI log does not establish its actual returned string.
- [x] Disprove the fixture premise with a real response withheld beyond the unchanged internal 200 ms budget and before the later 1000 ms caller deadline. The operation truthfully returns `setContent timeout after 200ms`; the script has not arrived, so its `turn` global is `undefined`. Reading that state as the old fixture did reproduces the JSON error. Request admission alone does not prove callback entry.
- [x] Make both internal-versus-later-caller controls hold the actual script response, require the exact earlier soft timeout and absence of authored script execution, then release the response and require the retained script to execute once and finish its real `finally` with the original 200 ms session budget. Preserve the other ten finite-tail, infinite-callback and explicit-caller controls unchanged. State-read failures now include the actual returned value.
- [x] All twelve focused controls pass in both serial and four-thread execution. Restoring only the premature callback-state read reproduces both held-response failures (`undefined` is not JSON); exact final source restoration is verified. This is a fixture-contract correction, not proof that the historical CI failure had that exact transport timing, and not a new whole-workspace or Linux CI result. Affected Render all-target Clippy with denied warnings, workspace formatting, modified-file CC15 and whitespace checks pass. No production code, timeout, floor or ignore changed.


### Recurring Linux fanout: authenticated Promise owner discovery

At `682e8ac9`, ordinary Linux Rust passes while LLVM Render-A again exceeds the unchanged 5000ms limit for the original 32768 fanout workload (7,273,322 us; `.recovery/ci-release-watch/085-682e8ac9-RenderA-job-136239196774.log`). The existing membership optimization is present: 652458 scan candidates and zero bulk membership lookups. This recurrence is not hidden by the earlier green receipts.

Every hook was still rediscovering ownership through the captured async-context getter even when the native Promise already had an immutable authenticated owner. Actual baseline fanout records 524318 discoveries for 131083 native Init events; the new work-count assertion fails after the original results, task ordering and deadline premises pass. The repair handles only non-Init hooks with no parent, matching the genuine native Promise private identity and weak Promise object and requiring an existing Some owner. Init, unknown, missing, pruned, tracking-error and unmatched-identity paths retain the original discovery logic. No unknown owner is cached as Root, no strong Promise/context root is added, and selection still revalidates native document leases and retirement. Native state observation, reaction flags and maintenance are preserved once per hook.

Final ordinary validation passes 27 controls; LLVM validation passes those 27 plus the same three workloads with two test threads. Fanout discovery calls fall to 131083, exactly its Init count, with 393235 authenticated reuse hits. The unchanged 32768 LLVM fanout completes in 0.864s serially and 1.815s concurrently on the measured host; these are contextual local measurements, not a guarantee of current Linux CI performance. Genuine unknown-infrastructure, pruned identity, foreign errors, retired-root/peer lifetime, parent collection and module isolation controls pass. Render/NAPI all-target Clippy with denied warnings, workspace formatting and modified Rust CC15 pass. Proof is under `.worktrees/promise-owner-discovery/.recovery/owner-discovery/{baseline,final-on,llvm,strict}`. Current Linux confirmation and refreshed affected release artifacts remain pending; no execution limit, workload, assertion ceiling or coverage floor is raised.
