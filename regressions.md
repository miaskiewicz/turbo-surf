# Release regressions

Target: `release/v0.8.4`. Updated 2026-11-15.

Original batch: **34 test cases (11 Rust, 23 shim)**. **30 have component proof and their fixes are merged; 4 remain open.** This is not a fresh combined-suite result. Existing intentional ignores/skips are unchanged.

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
| [x] | Rust | `debugger_inventory_captures_real_eval_function_and_child_sources` | Merged `8049cbc3`; original and critical inverse/restored pass, actual registry consumer inventory/module/extension/Node controls and strict pass; final full gates pending |
| [x] | shim | `commit reload hydrates the new document only when requested` | `81423b6d`; targeted proof complete, combined rerun pending |
| [ ] | shim | `commit history document replacements retain on-demand execution` | History candidate original 13-case cohort includes this case and passes. Broader source safety/expanded acceptance/inverse/strict requirements remain before the modular history merge |
| [x] | shim | `aliased DOM mutations, errors, cookies and child documents stay synchronized (#297)` | `e75cc661`; targeted proof complete, combined rerun pending |
| [ ] | shim | `real Location navigation and same-URL reload still fetch and execute (#319)` | History candidate original cohort 13/13 passes on 7f; expanded 18 has 16 passes and 2 failures. Awaited Navigation task progress deadlocks and initial-blank native load-state bookkeeping need fixes; interruption ACK and full acceptance still gate merge |
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
| [ ] | shim | `navigation deadlines also cancel slow hydration (#427)` | Producer #513 API pinned at 7f via `9b91244f`. Combined 7f candidate whole original now passes 1/1 (2.535s): both 30ms timeouts preserve the same actual native Page and both timeout0 retries load 180ms authored work. API 5/5 and clock 5/5 pass on the earlier artifact. A pre-merge safety control then found root init running in the infrastructure realm; the explicit actual-root context fix and guard are now in fresh artifact 544620d1. The entire original/API/warm/iframe packet must rerun on that artifact, with no carried final credit. Critical inverses/restoration and strict still gate merge |
| [x] | shim | `viewport resize updates live layout media and only its own page (#369)` | `864b3343`; targeted proof complete, combined rerun pending |
| [ ] | shim | `same-URL form navigation invalidates retained screenshot image bytes (#489)` | Action/navigation owner: Full original rerun fails replacement document request count 1 vs 2 (surface line 4315; before screenshot/image assertions); latest commit-only/background candidate succeeds quickly with real POST and replacement image HTTP; only exact image-request count now fails (3 vs 2) because static-to-live promotion redundantly fetches the old image. Image helper implementing bounded current-logical-document resource handoff; pixel and request assertions remain mandatory. Earlier cache fixes remain merged |
| [x] | shim | `frame locator typing uses canonical child selection and child event constructors` | `0a451d99`; targeted proof complete, combined rerun pending |
| [x] | shim | `Browser credentials answer Basic challenges and stay origin scoped (#394)` | `90ca133b`; targeted proof complete, combined rerun pending |
| [x] | shim | `static frame identity survives promotion to the live native registry (#447)` | `0a451d99`; targeted proof complete, combined rerun pending |
| [x] | shim | `in-flight screenshot assets cannot repopulate a reloaded document cache (#489)` | `dcdbbdf7`; targeted proof complete, combined rerun pending |
| [x] | shim | `frame locator interactions do not crash (#280)` | `205a5556`; targeted proof complete, combined rerun pending |

## Additional findings — required before completion

- [x] **Locator.count stale document** — merged `94e4915b`. Original count 0→1, fix-only OFF reproduces 0, exact restore passes six targeted/read-only controls; root merged-source routing and locator-read files pass 10/10 (3.62s) on signed ba119 artifact. Reads current live DOM without evaluate, guest execution or full serialization; final combined rerun pending.
- [x] **Locator.all stale document** — merged `e00e36e7`. Regression and fix-only inverse reproduce stale 0; restored live enumeration passes, including returned/indexed locators and static-disabled controls. Root merged-source routing/read files pass 10/10 (3.51s); final combined rerun pending.
- [x] **Platform-value ownership** — merged `659a1931`: Rust 6/6, platform/subclass 7/7 and infrastructure 7/7 pass. Allocation-only OFF produces five expected failures with DOMPoint controls green; byte-exact restore 14/14 green. Render strict/format/CC/hooks pass. Final combined build/gates pending.
- [ ] **Broader native history gap** — navigation owner. Verified original `2abd5438`/`c8d4` loses `/spa` after pushState → cross-document navigation → back; Chrome retains it. Authenticated native document/group/state history is being implemented. Source review confirms fixes for actor-budget recheck after getters, strict true owned-activation receipts, safe captured methods on private null-prototype arrays, and preserving stored undefined versus null. All 31 original/parity/policy/alias controls now pass on fresh `ca4c` (0 failures/skips/cancellations, 26.856s); formerly failed awaited Navigation task progress and initial-blank load bookkeeping both pass after scoped fixes. Native commit/private-mirror interruption requires an immediate protected ACK before guest callbacks. All corresponding controls remain required. Chromium 158 reentrant oracle confirms popstate.event.state stays the selected destination even when currententrychange pushes a newer history.state; preserve the selected state snapshot. Do not defer.

## Additional confirmed failures and investigations

- [x] **Normal iframe parent-load completion** — merged `92f5901c`. Official positive and held/error-peer controls pass; timer-only OFF reproduces the exact 2000ms load TimeoutError, byte-exact restoration passes. Strict, fmt, complexity and hooks pass. Deadline safety remains separate.

- [x] **Native JSON collectors preserve authored getter errors** — merged `ec46bdf2`. New root/child and unchanged original host_input.rs:239 all pass (3/3); serializer-only OFF reproduces all three specific generic-error failures, byte-exact restore passes. All-target Render/NAPI strict, fmt and CC pass. Script-navigation admission remains separate/open.
- [x] **Script navigation captures admission activation** — merged `f79d9a97`. Accepted admission metadata and trusted activation use the actual native guest Document. Original #398 plus six quota/activation/peer/HTTP controls pass; selector-only OFF reproduces three failures, exact restore and strict pass.

Baseline classification: the Enter, frame-top form and programmatic-submit cases all reproduce on byte-exact old main `6b583d98` shim plus verified registry addon `bdcebe65` (3 intended failures, no skips/cancellations, 3.442s). These are preexisting gaps, not regressions introduced by the new action candidate. All remain in scope.

- [ ] **Enter-key form completion** — baseline control confirms no replacement document (1 vs 2). Input helper is implementing real implicit-submit semantics and whole-Locator.press signal scope; raw keyboard/mouse/touch must not acquire navigation waits. Ordinary evaluate/read semantics remain unchanged.
- [ ] **Frame form targeting top-level completion** — fresh coalesced frame `_top` form control confirms no replacement document (1 vs 2). Action owner assigned; preserve actual initiator/destination authority.
- [ ] **Programmatic form background scheduling** — `Page.evaluate(form.submit)` followed by ordinary `waitForURL` produces only initial GET and times out at the original 1000ms. Action owner assigned native background scheduling; do not fix by making ordinary evaluate/read calls implicitly wait or pump unrelated work.

- [x] **Classic-script response visibility during opening close** — merged `8c62b9dc`. Independent four-control ON passes; observer-forwarding-only OFF yields two explicit missing-header assertion failures; exact restored fresh build passes all four. Pure Rust 2 and all-target Render/NAPI strict, formatting, CC and hooks pass. CORS/CSP/SRI and body/header assertions remain intact. Opening-close acknowledgment race is separate and remains open.
- [x] **Child-frame click navigation (#280) older-addon failure** — whole surface replay on newer platform addon passes the original. Old E118 failure was missing accepted native fixes; final composed build must still rerun it.
- [ ] **Shadow-link default navigation** — full newer platform-addon cohort reproduces `/` instead of `/folder/next` (`shadow-locators.test.mjs:248`). Actual read-only diagnostic confirms server fetched the new link document and native Location/document.URL changed, but Page.url remained old: action completion/mirror lag. Fresh coalesced original now passes; critical inverse/restoration and strict acceptance still pending before merge.
- [ ] **Page.click GET form completion** — new cohort failure: returned content is the old form rather than the server response containing `q=rust` (`surface.test.mjs:515`). Actual read-only diagnostic confirms valid form action/document URL and accepted native pending navigation, but no replacement HTTP request: form work is unscheduled on the actual guest target. Fresh coalesced original now passes; critical inverse/restoration and strict acceptance still pending before merge. Review of the unmerged candidate found global navigation draining could also admit sibling queued navigation; fix requires target-scoped completion plus an actual sibling pending-navigation control before acceptance (part of this fix, not another baseline failure). Adjacent original single-slot form queue may discard another target’s submission; require an actual multi-target RED/control before classifying it as a distinct confirmed bug.

- [x] **NAPI all-target strict dead-code errors** — merged `680ee757`: restored missing crate-root `live_image_assets` re-export. Test-first E0425 RED → export fix GREEN; focused task-construction unit and NAPI all-target strict pass. No lint suppression or removed capability.

- [x] **Detection same-URL POST control** — fresh current `4e1634a6` source with immutable MCP example `6403299b` passes the original in 7.35s. Old pending-Promise symptom is gone. Verified clear without an additional source fix; not a new merge.
- [x] **Detection consent native input** — merged `1f7d97f1`. Actual guest dispatch with same-owner cancellation, retired-skip and captured wait primitives. Original consent passes; selector-only OFF reproduces the same Illegal invocation, byte-exact restore passes. Three new input/cancel/poison guards pass and all-target Render/NAPI strict passes. Whole host-input cohort has 24 passes and two independent tracked failures; no existing tests skipped.
- [ ] **Detection retry / MCP root navigation progress** — input steps now succeed with no errors, but the actual final results are 0 vs 1 and remain at /search. Both the example observation loop and product MCP render pipeline omit native auxiliary navigation progress. Owner is preparing bounded root-admitted native work within existing deadlines plus a product-level regression; harness-only progress will not be accepted as product completion.

- [ ] **Frame Enter after top navigation** — actual old-frame keyup routing and stale root mirror failures. Fresh v7 six focused plus 20 adjacent controls pass; routing-only and mirror-only OFF reproduce failures, exact restoration passes. Final action integration/strict pending.
- [ ] **FontFaceSet private WeakMap exposure** — authored WeakMap.get intercepts Surf's private font maps. Captured primitives fix both maps; zero-hook/font semantics controls pass. Native get-only OFF reproduces eight hooks, exact restore passes. Final action integration/strict pending.

## Current tally

Accounting: 34 original test cases plus 19 additional tracked cases/findings. This is not a count of unique GitHub issues.

- Merged with component proof: **39**.
- Verified clear without an additional source fix: **1** detector POST control.
- Total done: **40**. Total pending: **13**. Total tracked: **53**.
- Pending includes locally green history/action/MCP packets awaiting final validation and integration; deadline safety still fails.
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
