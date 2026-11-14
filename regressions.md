# Release regressions

Target: `release/v0.8.4`. Updated 2026-11-14.

Original batch: **34 test cases (11 Rust, 23 shim)**. **27 have component proof and their fixes are merged; 7 remain open.** This is not a fresh combined-suite result. Existing intentional ignores/skips are unchanged.

All additional bugs found during this work are explicitly in scope and tracked below until fixed and validated. No deferrals. Payroll/backend/frontend tests and service changes remain paused until a new express go-ahead. No tag or publication has been authorized.

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
| [ ] | Rust | `debugger_inventory_captures_real_eval_function_and_child_sources` | Render/module: Original ON → guest-registration-only OFF missing-eval RED → exact restore GREEN; strict/CC/fmt pass. Actual archive consumer also passes inventory + module + extension checks and strict; component `2aeb763` awaits registry-resolved adoption before merge; the five approved dependency publications and checksum checks completed successfully |
| [x] | shim | `commit reload hydrates the new document only when requested` | `81423b6d`; targeted proof complete, combined rerun pending |
| [ ] | shim | `commit history document replacements retain on-demand execution` | Navigation/history: Driver promotion implementation; native proof pending |
| [x] | shim | `aliased DOM mutations, errors, cookies and child documents stay synchronized (#297)` | `e75cc661`; targeted proof complete, combined rerun pending |
| [ ] | shim | `real Location navigation and same-URL reload still fetch and execute (#319)` | Navigation/history: Bounded correction plus full native history ledger/ordering now required |
| [ ] | shim | `initial module top-level awaits can complete host fulfillment and route.fetch` | Module owner: Actual audited-archive consumer passes native 5, Node 4, original host TLA case, inspector 1 and extension identity 5 on default V8; strict/fmt/CC also pass and Node controls are registered in official test:shim. User explicitly approved the five immutable dependency publications; all five approved dependencies are published and public registry/download checksums match the independently reviewed payloads. Registry-resolved adoption is in progress. No Surf tag/release authorized |
| [x] | shim | `background binding collection errors remain observable` | `f1fb5c63`; targeted proof complete, combined rerun pending |
| [x] | shim | `native tabs and navigation match the source-pinned Chrome profile control` | `ff0a767`; targeted proof complete, combined rerun pending |
| [x] | shim | `HTTP attachments emit binary downloads without replacing the live document` | `e51c6f21`; targeted proof complete, combined rerun pending |
| [ ] | shim | `headers remain observable during initial-close hydration` | Opening-close owner: Fixing native opening-close races; original not green |
| [x] | shim | `unrouting every handler survives a fresh realm and later route reactivation` | `ff0a767`; targeted proof complete, combined rerun pending |
| [x] | shim | `a missed web-first assertion hydrates default navigation while commit stays static` | `98c8a8b8`; targeted proof complete, combined rerun pending |
| [x] | shim | `request metadata and client hints survive hydration and stay context scoped` | `8f511cfa`; targeted proof complete, combined rerun pending |
| [x] | shim | `no-content navigation retains the active document and history (#428)` | `efdc3f0d`; targeted proof complete, combined rerun pending |
| [x] | shim | `direct navigation referrers are explicit and reload retains them (#426)` | `8f511cfa`; targeted proof complete, combined rerun pending |
| [x] | shim | `history navigation honors networkidle hydration (#425)` | `d1b61ed7`; targeted proof complete, combined rerun pending |
| [ ] | shim | `navigation deadlines also cancel slow hydration (#427)` | Deadline owner: Native controls 5/5 pass, but full original still FAILS: second reload times out correctly yet closes Page (line 3239). Initial timeout0 recovery passes. Typed native probe isolates Environment stage after identity commit: animation-policy installation returns false while execution is not terminating; actual native Closed event follows. Investigating policy/bootstrap ownership, no green credit |
| [x] | shim | `viewport resize updates live layout media and only its own page (#369)` | `864b3343`; targeted proof complete, combined rerun pending |
| [ ] | shim | `same-URL form navigation invalidates retained screenshot image bytes (#489)` | Action/navigation owner: Full original rerun fails replacement document request count 1 vs 2 (surface line 4315; before screenshot/image assertions); investigating form completion with GET/shadow defaults, cache fixes remain merged |
| [x] | shim | `frame locator typing uses canonical child selection and child event constructors` | `0a451d99`; targeted proof complete, combined rerun pending |
| [x] | shim | `Browser credentials answer Basic challenges and stay origin scoped (#394)` | `90ca133b`; targeted proof complete, combined rerun pending |
| [x] | shim | `static frame identity survives promotion to the live native registry (#447)` | `0a451d99`; targeted proof complete, combined rerun pending |
| [x] | shim | `in-flight screenshot assets cannot repopulate a reloaded document cache (#489)` | `dcdbbdf7`; targeted proof complete, combined rerun pending |
| [x] | shim | `frame locator interactions do not crash (#280)` | `205a5556`; targeted proof complete, combined rerun pending |

## Additional findings — required before completion

- [x] **Locator.count stale document** — merged `94e4915b`. Original count 0→1, fix-only OFF reproduces 0, exact restore passes six targeted/read-only controls; root merged-source routing and locator-read files pass 10/10 (3.62s) on signed ba119 artifact. Reads current live DOM without evaluate, guest execution or full serialization; final combined rerun pending.
- [x] **Locator.all stale document** — merged `e00e36e7`. Regression and fix-only inverse reproduce stale 0; restored live enumeration passes, including returned/indexed locators and static-disabled controls. Root merged-source routing/read files pass 10/10 (3.51s); final combined rerun pending.
- [x] **Platform-value ownership** — merged `659a1931`: Rust 6/6, platform/subclass 7/7 and infrastructure 7/7 pass. Allocation-only OFF produces five expected failures with DOMPoint controls green; byte-exact restore 14/14 green. Render strict/format/CC/hooks pass. Final combined build/gates pending.
- [ ] **Broader native history gap** — navigation owner. Verified original `2abd5438`/`c8d4` loses `/spa` after pushState → cross-document navigation → back; Chrome retains it. Implement authenticated native document/group/state history with structured-clone state, transactional mutation and correct guest/driver traversal. User explicitly requires this; do not defer. Review of the unmerged candidate also requires actor-wide retained-byte budget revalidation after serialization getters can mutate a sibling target; add a genuine cross-target getter mutation control before acceptance.

## Additional confirmed failures and investigations

- [ ] **Classic-script response visibility during opening close** — confirmed script-fetch drops the core response observer: server flushes headers but facade waits for EOF. Lifecycle owner is implementing bounded forwarding with actual cancellation tests, preserving CORS/CSP/SRI and the header assertion.
- [x] **Child-frame click navigation (#280) older-addon failure** — whole surface replay on newer platform addon passes the original. Old E118 failure was missing accepted native fixes; final composed build must still rerun it.
- [ ] **Shadow-link default navigation** — full newer platform-addon cohort reproduces `/` instead of `/folder/next` (`shadow-locators.test.mjs:248`). Actual read-only diagnostic confirms server fetched the new link document and native Location/document.URL changed, but Page.url remained old: action completion/mirror lag. Targeted interaction completion fix is source-reviewed; native acceptance pending.
- [ ] **Page.click GET form completion** — new cohort failure: returned content is the old form rather than the server response containing `q=rust` (`surface.test.mjs:515`). Actual read-only diagnostic confirms valid form action/document URL and accepted native pending navigation, but no replacement HTTP request: form work is unscheduled on the actual guest target. Targeted interaction completion fix is source-reviewed; native acceptance pending. Review of the unmerged candidate found global navigation draining could also admit sibling queued navigation; fix requires target-scoped completion plus an actual sibling pending-navigation control before acceptance (part of this fix, not another baseline failure). Adjacent original single-slot form queue may discard another target’s submission; require an actual multi-target RED/control before classifying it as a distinct confirmed bug.

- [x] **NAPI all-target strict dead-code errors** — merged `680ee757`: restored missing crate-root `live_image_assets` re-export. Test-first E0425 RED → export fix GREEN; focused task-construction unit and NAPI all-target strict pass. No lint suppression or removed capability.

- [ ] **Detection trace retry, same-URL POST and delayed-consent controls** — detector completed 273 total: 258 passed, 3 failed, 12 existing feature skips, 0 cancellations (62.43s). Three native controls fail pending-Promise completion using the existing `rust/target/debug/examples/detection_trace` executable, which is independent of the pinned addon. Capture exact binary/source provenance and rebuild the example on the final graph before classification; any reproduced current-source defect remains in scope.

## Current tally

Accounting: 34 original test cases plus 11 additional tracked cases/findings (the three detector controls counted separately). This is not a count of unique GitHub issues.

- Merged with component proof: **31** (27 original + count, all, platform ownership, NAPI export).
- Locally proven, not yet merged: **2** (module and inspector dependency adoption).
- Awaiting fresh-binary classification/verification: **3** detector controls.
- Still requiring implementation or a passing fix: **9**.
- Total pending: **14**. Total merged/done at component level: **31**. Total tracked: **45**.
- Final combined gates and coverage remain pending for the release as a whole.

## Completion gates

User sequencing requirement: **fix all known failures first**, with targeted and adjacent regression checks as each fix lands. Only then freeze the combined release build and run the complete sweep; coverage follows green full gates.

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
