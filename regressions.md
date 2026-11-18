# Release regressions

Target: `release/v0.8.4`. Updated 2026-11-17.

Original batch: **34 test cases (11 Rust, 23 shim)**. **All 34 have component proof and their fixes are merged.** This is not a fresh combined-suite result. Existing intentional ignores/skips are unchanged.

All additional bugs found during this work are explicitly in scope and tracked below until fixed and validated. No deferrals. Payroll/backend/frontend tests and service changes remain paused until a new express go-ahead. Required supporting dependency publication was explicitly authorized and completed; no Surf release tag is authorized.

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
