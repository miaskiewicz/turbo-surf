# Google Chrome parity checklist

This is the canonical Chrome parity checklist for PR #26 and follow-up work.
Every unresolved item remains open until its implementation, behavior regressions
and combined validation are recorded. Work left outside PR26 is tracked in
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28); deferred audio
has its dedicated [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27).
Do not close either issue merely because the PR merges.

Reproduce the current configuration with the [Chrome155 parity runbook](./CHROME-PARITY-RUNBOOK.md); it separates required opt-ins and measured evidence from remaining parity gaps.

The current [API-SHAPE-LAST-RUN.json](./API-SHAPE-LAST-RUN.json) and
[API-INVENTORY.json](./API-INVENTORY.json) record the fresh9304df9 audit:
all75 interfaces measured,64 missing original-four own members,142 expanded
missing members and374 difference paths. Actual Chrome is155.0.8059.85 /
V815.5.35.25; native uses15.5.35.23-rusty and the unchanged optimized executable.
The older [API-SHAPE.json](./API-SHAPE.json) is a historical28-interface snapshot,
including the old94 count, not today's inventory. Keep all listed member and
descriptor paths open under issue28 until behavior and ownership are repaired.
A missing **own prototype member** is an observed descriptor/ownership difference;
it does not by itself prove the feature is entirely absent. Existing inherited
methods and aliases must be tested before adding another implementation.
A repair is closed only after behavior tests and a fresh paired audit pass.

## Startup and scheduling follow-up — 2026-10-21

The following scoped fixes are integrated in PR26 and validated on9304df9.
Broader behavior remains open below and in issue28:

- [x] Share immutable font collection bytes across faces while preserving indexed
  font identity, shaping results and the installed font set. Measure actual cold
  initialization and peak memory independently of guest watchdogs.
- [x] Remove repeated ancestor computed-style work within a single calculation,
  preserving live results after CSSOM, tree and document mutations. The clean
  homepage CPU profile reproduces the cost without interface wrappers.
- [x] Run individual timer tasks with their microtask checkpoints and implement
  measured Window nesting behavior without changing budgets. Broader cadence
  and task-source ordering remain open.
- [x] Rebuild and validate the actual native engine, then remeasure homepage
  hydration and the Google flow. Both native baselines complete initial hydration
  with one pump inside the unchanged budget, save consent204 and submit real
  forms, then end429 with zero results. Native SERP acceptance remains0/2.
- [ ] Resolve remaining instrumented startup exhaustion, interval cadence and
  broader deadline/task-source behavior. Both interface-instrumented native
  captures still exhaust their initial hydration budget.
- [ ] Establish effective fresh Chrome consent dismissal and a complete paired
  SERP comparison. Four Chrome flow captures stop with an obscured input after
  no-effect consent dispatch; two blank-fixture probes fail inherited readiness.
  The subsequent standalone probe configuration repair passes two controls on
  each Node20/22, full Node22 detection206+Python2 and a genuine seven-process
  local matrix with all four probes per browser. It does not establish consent
  success or change the recorded Google failures.

The first rebuilt all-feature homepage diagnostic now completes HTTP200 hydration
with one completed pump and no budget exhaustion:18.52s whole run and1.24GB peak
RSS including profiling/snapshots. Its17.83s guest profile still spends about9.77s
in private CSS lookup/computation. After the selector-first follow-up, a new
diagnostic completes HTTP200 hydration in16.35s whole-run with957MB peak RSS;
guest profiling takes14.70s. Full gates at9304df9 pass1,509 native and1,464 stock
Rust tests, strict Clippy, formatting and rebuilt addon consumers. Native detection
passes204; stock passes195+9 documented skips. The fresh14-process Google matrix
has no timeout, eight exit0 and six Chrome exit1. All75 API interfaces are measured;
142 missing members and374 paths remain. These results do not close reliability,
complete timing parity or native search clearance. Known CSSOM implicit-color
synchronization and unknown-pseudo admission remain open; the latter keeps its
strong ignored Chrome regression and explicit expected exit101.

## Prior runner comparison — 2026-10-20

- [x] Verify a fresh signed-out traced Chrome baseline. Repeat1 uses a verified
  newly created Incognito context, saves consent and reaches a completed200
  search with nine markers. Repeat0 fails effective consent dismissal and stops
  before typing; that capture is not a successful comparison or search verdict.
- [x] Repair collector completion and current-document HTTP attribution. Preserve
  guest errors separately, reject malformed completion metadata and leave absent
  legacy evidence unmeasured. Hidden query fields are excluded by the shared
  selector, with the native matcher repaired in public turbo-dom `d01d6a5`.
- [x] Verify equivalent executed consent, typing and submission on localhost.
  Baseline/interface controls execute all six actions with trusted consent inside
  its box, response204/cookie before search, retained textarea identity and focus,
  genuine native form navigation and final results. Missing readiness or
  ineffective input fails before Enter; no silent refocus or text repair occurs.
- [x] Install real-time Window scheduling before hydration and retain the default
  crawler's virtual mode. The fixed deadline starts before Enter. Four paired
  suites now pass16 tests with zero failures/skips, including both genuine
  rootless-document controls. Retry clocks and actual document/server evidence determine
  shell, terminal or incomplete outcomes. A separate timer-free302 control
  requires429. Deterministic tracker tests cover provisional navigation without
  requiring a transient state to be sampled. Forty focused checks pass on both
  Node20 and22. Original native/Chrome timing failures remain recorded in
  [FINDINGS.md](./FINDINGS.md); scheduling/throughput parity remains in issue #28.
  Native completion summaries now validate the producer's actual host-error
  channel and reject malformed/conflicting evidence; old failed Google captures
  remain failed.
- [x] Freeze browser identity throughout the paired suite. The complete owned
  Chrome155.0.8059.85 bundle, current native binary and selected source/compiled
  fixtures are unchanged before and after the successful run. Chrome's actual
  V815.5.35.25 differs from native15.5.35.23-rusty/profile155.0.8059.80;
  this patch gap remains in issue #28.
- [x] Isolate diagnostic hooks from implicit `.env`/solver initialization. The
  example initializes render hooks directly; production sessions retain their
  existing configuration behavior. Actual subprocess regressions pass.
- [x] Complete stock/native engine validation and the collector follow-up.
  At3a77122 stock passes1,451 Rust tests and native1,496, with strict/fmt,
  verified optimized artifacts and loader/consumer guards. The subsequent
  harness-only rootless-document repair passes29 focused checks per Node20/22,
  both real browser controls, all16 paired controls and full detection200/0skips
  on Node22 and191/9 documented browser skips on Node20, plus Python2 each.
  Its genuinely refreshed resource oracle preserves every observed value and
  passes its stock Rust consumer. Original receipts keep their source identities;
  reviewed byte/parsed-JSON bridges cover unchanged native production inputs.
  Exact-head CI must pass after each code change. Final API/Google collection
  and the final report/description commit remain below.
- [x] Separate offline transport regression coverage from an optional external
  TLS echo diagnostic. The actual localhost ClientHello control and one bounded
  explicit live echo run pass with unchanged wire assertions; strict/fmt pass.
  Default gates now record the external test as ignored. The earlier interrupted
  request and ambiguous historical auto-return passes remain documented in
  [FINDINGS.md](./FINDINGS.md); this does not certify complete transport parity.
- [x] Remeasure all75 interfaces with the current optimized artifact and frozen
  Chrome. Counts are64 original-four missing members,142 expanded missing and
  374 difference paths; every path is retained in API-INVENTORY.json and issue28.
- [x] Run the fresh independent-cookie Google matrix. All14 process records are
  measured; ten exit0 and four Chrome captures fail, with no timeout/abort.
  One signed-out Chrome baseline reaches a nine-marker SERP. Both native
  baselines save consent, execute all six actions and use native forms, then
  receive429 with zero markers. Complete source/artifact guards pass.
- [ ] Resolve native Google clearance and initial homepage hydration exhaustion
  under issue28. Native home through step4 still records budget exhaustion
  during post-load hydration with zero completed pumps; final429 completion does
  not erase it.
- [ ] Resolve the observed Chrome consent precondition failure under issue28.
  A dispatched click without save/cookie/overlay effect must not count as
  successful dismissal; its cause remains unproved.
- [x] Record tested changes and sanitized reports, refresh the changelog, docs
  and PR description. Current-head CI remains an independently required check:
  use the [PR checks](https://github.com/miaskiewicz/turbo-surf/pull/26/checks).
  No merge, tag or release is included in this publication batch.

Current integrated public producers:

| Component | Revision and measured validation |
|---|---|
| turbo-dom | `d01d6a5`, [PR15](https://github.com/miaskiewicz/turbo-dom/pull/15): all six consumers and lockfile pinned; producer gate and exact-head CI pass. |
| turbo-html2pdf | `8f0aa13`, [PR25](https://github.com/miaskiewicz/turbo-html2pdf/pull/25), stacked on PR24: shared immutable font programs preserve indexed face identity;863 local workspace tests,15 xref controls, strict Clippy, formatting and complexity checks pass. Exact-head CI passes, including100% coverage. Surf integration is awaiting its rebuilt gates. |
| turbo-test | `e3b4817`, [PR27](https://github.com/miaskiewicz/turbo-test/pull/27), stacked on PR26: official committed-HEAD re-vendor; ancestor snapshots and selector-first matching pass304 library,52 compatibility and63 Node tests, with one explicit known-gap ignore retaining a failing Chrome admission expectation. All four platform CI jobs pass. A supplemental historical strict run retains28 unchanged-baseline diagnostics; it is not a passing Surf strict gate. |

The native66-test integration covers static split-inline layout, continuation
metrics, measured UA button styles, all nine border/priority controls, two genuine
same-embedded-font Range scenes, input/timers and the private adopted-sheet Array
setter regression. Source and compiled PNG/font fixtures remain unchanged.
The separate Times Range width is80.2734375 versus Chrome80.2578125; native canvas
weight/face selection also needs work. Both remain under issue #28's existing
system-font/GPOS/platform-metrics scope. The genuine Times golden and failures
are preserved; same-font passing controls do not certify Times parity.

Default inherited color can remain stale after `CSSStyleRule.style.color` changes:
when a descendant has no authored color, native cascade fallback can still read
original DOM stylesheet text while the JS CSSOM contains the edited value. The
old and candidate bindings reproduce the same failure. Explicit `color: inherit`
uses a separately supported path; its cache-lifetime test does not close this
native CSSOM/source synchronization gap. Track the broader fix in issue28.

CSS selector admission also remains broader than Chrome: the existing parser
accepts `.target, :definitely-unknown` in `CSSStyleSheet.insertRule`, whereas the
frozen Chrome155 control throws `SyntaxError`. Both the old and optimized
bindings reproduce this gap. The exact failing expectation is retained as a
separate known-gap regression under issue28. A browser syntax policy must preserve
valid newer/vendor forms and forgiving `:is()`/`:where()` branches; private
selector-first computation does not implement that admission policy. Coordinate
stylesheet parsing, insertion, `selectorText`, query/matches and `CSS.supports`
validation rather than using the current DOM matcher as a syntax validator.

Computed custom-property own values/case, unusual foreign `Reflect.construct`
prototype fallback and broader pointer hit-test/retargeting also remain in
issue #28, together with nested timer minimum delays/task ordering and CSS-wide
or unresolved-variable border values. Focused constructor, adopted-sheet and cascade fixes do not close
those broader behaviors. The dated checkpoints below retain earlier evidence;
they do not supersede the current run status above.

## Active PR26 follow-up — 2026-10-17

Implementation source `a005140` passes the complete stock-V8-150 workspace
(1,411 tests, nine ignores and six zero-test native-only targets) and genuine
native-V8-155 workspace (1,456 tests across 134 suites, six generated-doctest
ignores), with zero failures. Strict all-target Clippy and formatting pass;
all 1,457 selected implementation source hashes remain unchanged. Both Linux
Node20/Node22 CI jobs pass. Rebuilt native-addon checks pass: shim96 with one
intentional skip, E2E20/20, detection123 with two Node20 CDP skips, Python2/2;
separate Node22 real-Chrome CDP controls pass3/3 with zero skips.

Fresh paired API measurement covers75 interfaces with no unmeasured entries:
64 missing own members across the original four interfaces,142 expanded missing
members and374 descriptor difference paths. Complete member/difference arrays
are unchanged from the preceding inventory; this is measured shape/ownership
coverage, not full behavioral conformance. All remaining API and browser parity
work stays in issue #28; advanced audio stays in issue #27.

The fresh `a005140` Google matrix completes all14 processes without process
timeouts. Both Chrome baselines and both native baselines reach `/sorry/index`
with HTTP429 and zero result markers: native0/2 and Chrome0/2 accepted SERPs.
Both instrumented Chrome passes also receive429. All eight consent responses
are204 before search. Native uses genuine form navigation, with no fallback,
imported Chrome cookies or sidecar. All four native homepage passes exhaust the
20-second hydration budget with zero completed pumps; dismiss retains that
failure, and search subsequently reaches the real native429 response. The budget
residual remains in issue #28. No jc/lc requests appear in any of the eight
baseline/instrumented captures; their absence does not establish a rejection cause.
These Node22 captures are separate from the historical Node24/source59f7835 run
(Chrome7/8 result markers, native0/2). No native SERP success is claimed.


### Requested PR26 implementation batch — messaging, layout/Range and DOM

This batch addresses these three named areas in parallel. A source audit or
an isolated pass does not close the combined production regression. Update each
entry with implementation, independent controls and integrated results as it lands.

| Area | Implemented and measured | Remaining delivery checks |
|---|---|---|
| Messaging / GC | Weak-context registry pruning, retained/nested frame retirement, native initialization rollback and queued transfer cleanup have regressions. Source31bf6fe removes both heap-proven first-frame WebCrypto roots while preserving child exception identity, the original3 collections and5000ms budget. It passes76 stock focused controls,20 genuine-native155 lifetime/crypto controls, then the complete native workspace:1453 tests, zero failures,6 generated-doctest ignores across134 suites, strict Clippy and formatting, with1447 selected source hashes unchanged. Both Linux CI jobs pass all8 frame-lifetime tests. | Final a005 stock/native workspace, strict/fmt and rebuilt addon/Node/CDP gates pass. Residual browser behavior remains issue28. |
| Layout / Range | Public layoutdcbfd20 is pinned in both consumers. Source-run admission, separate Range/block/replaced float stages, complete leading/internal whitespace UTF-16 ownership, wrapped endpoints and source-derived caret canonicalization have regressions. Producer767 full tests/56 suites and41 focused controls pass with205 source hashes unchanged, alongside strict checks and independent reviews. The nineteen-scene control preserves the earlier fifteen observations;42 transform captures match and two fixed-child cases retain typed rejections. | Current stock and genuine-native155 inline targets each pass5/5 tests, zero ignored, with all1457 selected source hashes unchanged. All nineteen scenes run through actual DOM/CSS; complete a005 workspaces and rebuilt addon gates now pass as recorded above. Earlier085 stock/native inline5/5 at1457 source hashes and5a component78/78/stock4/4 at1455 hashes are predecessor proof. Earlier75 controls resolved the combined hang, iframe sizing, composed slots, scoped CSS and retirement; exact source/hash records remain in FINDINGS.md and LIVE-GEOMETRY.md. |
| DOM | Canonical3ed91fb is officially re-vendored; public DOM aaeb290 is pinned in all six consumers. Canonical74 focused tests pass on rusty_v8 149.3.0, including23 CSS controls and40 independent Chrome font cases. Production custom-element callbacks, parser/observer lifecycle, adopted stylesheet clearing and frame font ownership have genuine-native155 controls and are included in the source31bf6fe full workspace result above. | Final a005 combined gates and fresh paired API/Google measurements are complete; measured API residuals and unsuccessful Google results remain open in issue28. |

Closing the two uncovered producer lines exposed another bug in predecessor5a:
a span's leading whitespace after an image or inline block is assigned outside
its opening boundary. Independent same-font Chrome places the caption at x10
with width38.40625 and the visible ABC Range at x19.59375 with width28.8125;
the5a native handoff had the wrong source origin. Fifteen frozen Chrome
scenes cover direct text, nested/decorated spans, wrapped spans/bare text and paragraph starts/BRs;
individual Ranges and carets distinguish the first rendered space from the second
collapsed space. The reusable collector and exact end-to-end regression are saved;
all three collector modes were genuinely recaptured with previous geometry unchanged.
Public layout0850383 now fixes native boundary/source ownership and wrapped
span/bare-text prefix placement; its full765-test core/PDF gate and corrected
bare-text/host-box supplement pass, along with strict checks and independent
reviews. That085 revision's stock and genuine-native155
inline targets each pass5/5 tests with all1457 selected source hashes unchanged,
including the complete fifteen-scene DOM/CSS comparison. The public5a/Surf048d
focused passes above precede this repair and the completed a005 gates.

The subsequent ordinary-wrap control exposed missing internal-space source cells in
bare text. Public layoutdcbfd20 now retains real UTF-16 cells, both wrapped Range
endpoints and distinct caret affinities. Only actual connected zero-space suffixes
at the winning glyph edge canonicalize caret offsets; visible leading spaces and
pre/nowrap glyphs remain candidates. Producer767 full tests and41 focused controls
pass, with strict checks and independent reviews. Four ordinary one/two-space scenes
extend the reusable control to nineteen, preserving previous observations exactly.
Both final-source stock/native155 inline targets pass5/5 tests at the same1457
unchanged source hashes, covering all nineteen scenes. Current a005 full gates
pass; the test-only producer follow-up `4999290` passes768 tests and coverage9,806/9,806
with pinned dcb production bytes unchanged.

The native glyph candidate is bounded by the measured fonts. Broader GPOS
adjustment ordering and macOS AAT/trak backend selection still need independent
controls and remain tracked in issue28; matching IBM/Fira or the saved Times/Menlo
cases does not establish full text-shaping conformance.

The independent zero-viewport control also exposes a remaining BackCompat body
strut difference: native34px versus Chrome30px, with matching child rectangles and
Times text Range18.5px. Full quirks geometry needs document-mode input and remains
in issue28. A separate standards-srcdoc control must not erase that evidence.

### Historical fresh retry — 2026-10-17, source59f7835

All14 processes finish without a process timeout, with56 source/helper hashes
unchanged throughout collection. **Chrome baselines reach7 and8 SERP markers;
native baselines accept0/2 SERPs**, ending at/sorry429. Both instrumented Chrome
passes are challenged; their outcomes remain separate from the clean baselines.
Native uses fresh cookies, actual consent and genuine forms with all four native
features, no imported Chrome jar and no form fallback. Unlike the earlier88528ae
matrix below, this retry supplies two passing clean Chrome controls. It still does
not identify the native rejection signal. These historical receipts superseded
the earlier same-day capture; the current a005 results are recorded above.

Both instrumented final Chrome documents contain reCAPTCHA element/iframe/script/
response-field markers, corroborating the user observation. Native homepage opens
exhaust20s in3/4 captures: clean repeat1 and both instrumented passes, all in
post-load-hydration with zero completed pumps. Clean repeat0 reaches500pumps without
exhaustion. Dismiss retains the original open diagnostic; later navigation
budget=false does not erase those failures. Homepage boundedness remains unreliable.
See [challenge indicators](./GOOGLE-CHALLENGES.json).

[OBSERVABILITY-LAST-RUN.json](./OBSERVABILITY-LAST-RUN.json) records the sanitized
Chrome protocol observations, native source/binary hashes, measured homepage
outcomes and preserved earlier failures. Full-gate status is updated separately.

| Item | Implemented / observed | Validation and remaining work |
|---|---|---|
| Chrome response transport metadata | Both collectors retain protocol, security/TLS, timing, connection/cache state, redirects and raw response events. Missing fields and collector scopes are explicit. | Real localhost HTTP/2+TLS, HTTP/1.1, redirect, worker/frame and alternative-collector controls pass. These are CDP/application observations, not packets or complete TLS fingerprints. |
| Request/header correlation | Direct CDP uses exact session/request IDs and validated redirect hops. The alternative collector enriches only unambiguous main-frame navigations; other associations remain unavailable. | Adversarial controls cover concurrent same-URL worker/frame traffic and request-ID collisions. Callback arrival time is no longer an identity anchor. Both rebuilt addons pass116 detection controls; Node22 executes all3 real-Chrome controls with zero skips. |
| Native GPU interface tracing | Returned WebGL/2D contexts and extension objects have configurable own/prototype methods observed, with explicit skipped/unavailable coverage and bounded inspection. | Twelve VM controls pass on Node 20/22; default and genuine155/Metal addons pass paired clean/traced identity and pixel controls. Wrappers remain observable. Symbols, prior captured functions, later replacements and ordinary data reads remain outside coverage. |
| Fresh homepage transport observation | Independent clean Chrome 155.80 homepage captures report HTTP/3/QUIC for the main Google document; native homepage captures report HTTP/2. | This is a scoped measured difference, not proof of a rejection cause. Source-pinned metadata and safe protocol aggregates are recorded in OBSERVABILITY-LAST-RUN.json; the latest complete search matrix remains 0/2 accepted native SERPs. |
| Native hydration diagnosis | Host-owned phase/pump diagnostics and optional Inspector CPU profiles locate the original 20s failure in the first post-load hydration pump, with zero completed pumps. The selector-validation build completes three clean homepage-only runs and one CPU-profiled run within the unchanged 20s budget. Earlier failed/mixed candidates remain preserved separately. | All four homepage-only controls reach phase complete and the500-pump cap; clean runs report pendingWork1. The earlier88528ae matrix has one instrumented open failure; the historical59f7835 retry instead exhausts3/4 homepage opens, including clean repeat1. Its dismiss snapshot retains that same open diagnostic, not a second timeout. The current a005 matrix exhausts all4 native homepage opens with zero completed pumps. Full-trace boundedness remains in#28; no budget increase is made. |
| Computed-style refresh cost | Canonical turbo-test commit `fe2b7a4` snapshots the private stylesheet list once per computation, preserving fresh reads on subsequent computed-property access. Officially re-vendored into this branch. | Canonical29 CSS controls and the iframe-owner regression on both rebuilt addons pass, including child CSSOM mutation and poisoned public styleSheets access. The private entry point bypasses document.styleSheets but its provider still uses public querySelectorAll/sheet hooks; those remain in#28. No persistent style cache is added. |
| Parser resource scan cost | Avoid repeated whole-tree document-order comparisons when the native tree is unchanged; use a native mutation fallback, trusted collection-order snapshot and script-element exclusion. | Default reentrant resource ordering, SVG/uppercase exclusion and poisoned Array.from/iterator controls pass within 29 focused tests across 8 binaries. Native profiling reduces inclusive compareDocumentPosition samples from 2662 to 0; two clean homepage controls complete. The profiled run timed out at that intermediate checkpoint; the later selector-validation build supplies the latest passing measurements. |
| Inherited computed-style recursion | An isolated control reveals exponential stylesheet scans at nested explicit inherit declarations: depths 3/5/7 produce 248/1016/4088 scans on the older addon. Fresh parent declarations are immediately refreshed again through their live property getters. | Canonical turbo-test commit `e5231a7` reads freshly computed parent backing values once per calculation and is officially re-vendored. All 23 canonical CSS controls pass, including linear scan bounds for inheritance/percentage-width/height chains and six independent Chrome mutation states. Independent adversarial review is clean. The intermediate post-inheritance native build succeeded, but both clean repeats and its profiled run timed out before completing a post-load pump. The bounded recursion regression was fixed, while timeout resolution still needed the later selector-validation repair. |
| Native geometry capture cost | The initial host-profile checkpoint locates 8814/12740 inclusive samples beneath a one-pass controller visibility scan, with 6792 under native geometry and 4428 under computed style across the profile. These overlapping counts must not be summed. | Opt-in host timings measure 76 captures at 9.294s, with 9.031s in stylesheet work, versus 84ms composition and 129ms tree capture. The existing layout cache hits 64/76; 12 actual layouts total 496ms. The native-child traversal candidate passes two fixed 100-read controls with the same checksum and 99 cache hits/1 miss: composition falls 6.9ms→1.9–2.1ms, while total capture stays variable at 35.5–40.7ms versus 39.3–39.5ms before. This is a small scoped saving, not reliable timeout resolution. The previous candidate3 Google profile separates 6.17s stylesheet work into 5.05s selector preparation and 0.99s export (98 captures, 83 hits/15 misses, 0.46s actual layout). The reviewed once-per-selector validation fix keeps fresh per-node matching and adds no persistent target cache. Its same 98-capture profile reduces selector preparation 5.05s→1.97s and total capture 6.29s→3.18s; export stays about 0.97s and actual layout about 0.40s. The profile does not demonstrate timer runaway. No timer rewrite, budget increase or unverified persistent cache is justified by this evidence. |
| Duplicate computed-style rule lookup | Canonical turbo-test commit `8d8ebd2` reuses the private rule record already resolved during import flattening; officially re-vendored. | All29 focused canonical CSS checks pass, with fresh Chrome live/computed controls and seven genuine nested-HTTP-import mutation states. Independent review and the combined local gates pass. |
| Repeated native selector validation | The reviewed adapter fix validates each selector only on its first eligible node, then matches every subsequent node against current tree/state. Shadow matching, scope eligibility, empty-scope and first-error behavior are preserved. | Resolved turbo-dom source proves checked validation is source-only before current-state matching. Four focused selector controls cover100 nodes, dynamic checked/custom states, shadow scope and first errors. Candidate4 also passes15 CSS-live and38 lifecycle/geometry/resource/shadow controls, then three clean homepage repeats (13.30s/11.44s/13.61s process wall) and one profiled repeat (12.33s) without budget exhaustion. All reach500 pumps; clean runs report pending work. No persistent match/stylesheet cache is added. |
| Initial script classification | A narrow native HTML-script guard prevents an HTMLScriptElement receiver error from discarding the initial script batch when an SVG/uppercase pseudo-script is present. | Independent old-addon reproduction and new default queue-preservation/resource controls pass. Broader SVG script loading/execution remains open in #28; skipping an unsupported SVG script does not implement that feature. |
| Default geometry fixture startup | The first full default gate passes1011 tests, then both neutral-at-rule controls hit5-second expression budgets before their behavior assertions. An isolated host sample places one thread in cold system-font discovery/parsing and the peer in the same OnceLock wait; an unsampled single control passes. | A separate60-second warmup checks real7×9 geometry, closes its session, then retains the original5-second conformance sessions and every assertion. Affected default/native2 controls and the complete default gate pass. Production cold-font startup latency remains in#28. Failed logs/binary are retained; failed gates are not counted as passes. |

At historical production source88528ae (documentation/evidence59f7835),
the complete default `npm run check` passes: **1384 Rust tests**, zero failures,
9 documented ignores across131 suites, strict all-target Clippy and formatting.
The native155 full workspace passes **1423 tests**, zero failures and6 generated
doctest ignores across131 suites. That native full run precedes only the test-only
font warmup; both affected native controls and current-helper strict/fmt pass with
all implementation hashes unchanged.

Both rebuilt addons pass **95 shim tests** (one intentional framework skip),
**20 E2E**, **116 detection** (two optional Chrome-CDP checks skipped on Node20)
and **2 Python** helper tests. Node22 separately executes all **3 real-Chrome
controls with zero skips**. Both iframe computed-style-owner regressions run and pass.
[VALIDATION-LAST-RUN.json](./VALIDATION-LAST-RUN.json) records source hashes,
commands, results and the exact native full-run/test-only amendment boundary.

The earlier candidates and failures remain preserved. Intermediate stylesheet,
inheritance and native-child traversal repairs alone did not give stable homepage
completion; the latest selector-validation build supplies the3 clean/1 profiled
passing controls. The first default full gate stopped after1011 passes on two
cold-font fixture watchdogs. Host sampling locates font discovery/parsing and the
shared-registry wait; unsampled isolated controls also pass. A subsequent retry
and an additional native lint attempt encountered disk exhaustion. Their partial
results are not counted as passing gates. The bounded fixture warmup and inactive
compiler-cache cleanup precede the successful complete retries. Production
budgets and cold-font performance are unchanged.

The earlier same-day consent/search matrix at production commit `88528ae`
completes all14 processes without a process timeout: **native baseline0/2 accepted
SERPs, Chrome baseline0/2**. All four native passes save real consent204 before
native-form search200 and retry to/sorry429, using native cookies and no fallback
or imported Chrome jar. Both clean native flows stay within the20s hydration
budget; instrumented repeat1 exhausts it during homepage open with zero completed
post-load pumps. The sticky dismiss snapshot is the same original diagnostic.
This leaves intrusive full-trace boundedness open in#28. Chrome is also blocked,
so this run cannot isolate a native-specific rejection cause. The October16
Chrome-success checkpoint remains historical below. See [GOOGLE-LAST-RUN.json](./GOOGLE-LAST-RUN.json).
Exact final-head CI status belongs to the linked PR checks; local records do not
certify a future CI run. Every broader residual stays in#28; audio stays in#27.

Audited cleanup removed consumed-success test executables in exact reviewed
batches:102 stale default (17.82GiB),109 native (18.87GiB),14 native reserve
(2.94GiB),25 default prefix (2.13GiB), one native focused executable, and109 final
default executables (18.06GiB). Only inactive incremental compiler cache was also
cleared. Hashes/file identity, ignore/tracking state and open handles were checked;
source, libraries/dependencies, SDKs, frozen evidence binaries and raw logs/profiles
remain preserved. The final109-file cleanup raised actual free space from11.92GB
to31.30GB before the fresh native release-CLI rebuild.

## Historical repair status — 2026-10-17

Update this table when a fix or its validation changes. A component pass closes
only its named control; the broader area stays open until its listed requirements
and combined gates pass. Historical inventory/live counts are not updated by
reading source alone.

| Repair | Current evidence | State |
|---|---|---|
| Dictionary store, negotiation and incremental decoding | Pushed ec868b7; Core220 and10 HTTPS controls per transport, strict/format pass | Implemented within controls; fresh Google capture complete, publication/Range residuals open |
| Dictionary discovery and resource task source | Pushed a15d098;32 exact independent Chrome cases pass on stock and actual155 | Implemented; publication timing/encoded Range remain open |
| Live form collections, submitters and radio ownership | Pushed a15d098/canonical51b5b97; actual155 collection2/navigation12/radio1 pass | Implemented within controls; broader forms remain open |
| Native control labels and receiver-realm NodeList | Pushed5c0b886/canonical1e93cae; actual155 and stock production behavior/GC2 pass | Integrated; full Rust gates pass, label activation open |
| Real native deadlines vs virtual Window timers | Pushed5c0b886; source-pinned Chrome4 outcomes, actual155 affected14 and MCP73 pass | Integrated; full Rust gates pass |
| Private abort/controller conversion | Actual155 privacy2 and stock privacy2/adapter2 pass; original hostile-hook reproductions retained | Concrete exposed paths fixed; broader hooks/array fast-path count remain in#28 |
| Native XHR identity and receivers | Independent Chrome control1 + existing HTTP-frame4 pass on both engines; strict and both detector captures pass | Bounded fix validated; broader XHR remains#28 |
| Interface attribution | Five focused controls and89 detection tests per addon; all20 saved host-frame rows classify runtime in fresh capture | Immediate script-pump misclassification fixed; attribution remains a first-stack heuristic |
| API inventory | Fresh75-interface Chrome155.80/native155 pair:64 original/142 expanded missing,374 paths, all interfaces measured | Measured; all residual paths tracked in#28 |
| Google acceptance | Latest October17 source59f7835 GPU/wreq matrix: native baseline0/2; Chrome baseline7/8 markers; all14 processes complete | Native consent/forms work; clean Chrome succeeds, instrumented Chrome is challenged. Native rejection signal and reliable acceptance remain#28 |

### Historical full workspace checkpoint — 2026-10-16

At that checkpoint, the default full-workspace command exits0 with1,376 passing tests,
zero failures and nine explicit ignores (three unavailable stock-SDK native
callbacks and six generated doctests). The final actual155 full six-feature
command exits0 with1,415 passing tests, zero failures and six generated doctest
ignores. Both commands cover129 component suites.
Both engines pass the final XHR5 and shadow9 controls, workspace all-target strict
Clippy with `-D warnings`, and formatting. Both rebuilt addons pass95 shim tests
(one intentional test-framework skip),20 E2E tests and89 detection controls with
zero skips, plus2 Python SDK-helper tests. The native addon actually renders the
shared GPU triangle: pixelHash3630084024 and centerRGBA[128,128,153,255].

The October16 75-interface pair and search matrix were complete at that checkpoint.
The current validation report retains those prior counts separately. The current
[Google summary](./GOOGLE-LAST-RUN.json) is the October17 rerun; both Chrome
and native baselines are rejected. Neither completed measurement proves acceptance.

## Earlier PR26 completion checkpoint — 2026-10-16

The implementation and local validation steps below were complete at that checkpoint. Behavioral
residuals remain explicitly open in#27/#28; these checkmarks do not certify
complete Chrome parity. The mutable final-head CI status is recorded in
[PR checks](https://github.com/miaskiewicz/turbo-surf/pull/26/checks) and must pass
before delivery. A commit-local checkbox cannot certify its own future CI run.

- [x] Push tested labels, clock, privacy, driver and conformance fixes:5c0b886/95e4c5c.
- [x] Pass full default/native workspace and both strict/format checkpoint gates.
- [x] Pass final XHR affected controls and all-target strict checks with the complete
  native CLI/addon feature combination.
- [x] Finish the October16 full workspace commands: native1,415 tests with
  six doctest ignores; default1,376 with nine documented ignores, zero failures.
- [x] Rebuild both addons and pass shim/E2E controls with recorded engine/features;
  each passes95 shim/20 E2E and89 detection controls plus2 Python tests.
- [x] Rebuild the optimized native executable with wreq, trust anchors, Metal and CoreText.
- [x] Capture the fresh75-interface pair and record64 original/142 expanded missing
  members and374 paths; every unresolved path remains open in#28.
- [x] Run two fresh native-cookie Google controls with actual consent and genuine
  forms, all four native features, no imported Chrome cookies or form fallback.
  Both native baselines reach/search200 then/sorry429:0/2 accepted SERPs.
- [x] Refresh changelog, findings, validation/API/Google snapshots and PR description;
  retain raw evidence and safely reclaim10.05GiB of consumed test executables.

## Open work and issue ownership

The residuals below have durable follow-up coverage in
[issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28), with audio in
[issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27). These are broader
behavioral residuals beyond the completed local gates and measured controls;
the issue retains the specific unclosed requirements.

| Area | Remaining closure requirements | Tracking |
|---|---|---|
| Messaging / GC | Broader realm/frame retirement, detached storage reclamation, endpoint and reentrant failure coverage | #28 |
| Layout | Inline behavior beyond measured decorated/empty/mixed-font controls, stacking, scrolling/events/RTL, fixed positioning, iframe migration and cold system-font discovery/parsing latency | #28 |
| Range / caret | Remaining composed-tree/lifecycle/geometry cases beyond the integrated interface/receiver/GC controls | #28 |
| DOM | Parsing/observer edges, custom-element reactions/registries, broader form association/CustomStateSet lifecycle, UTF-16 edge cases | #28 |
| Forms | Broader radio/adoption and form-collection behavior beyond saved controls, FileList/multipart, other targets and encoding/submitter/policy cases | #28 |
| User activation | Consuming APIs, cross-frame propagation, broader trusted input, MCP reconstruction continuity and retained/retired receiver or cross-origin-return redirect behavior | #28 |
| Events / promises | Stream/listener microtask interleaving and broader Object-prototype descriptor hooks during abort/event dispatch and exception reporting | #28 |
| CSS / resources | Remaining selectors/at-rules, used downloaded fonts/animations, images/pseudo epochs, inner SVG | #28 |
| Security | Remaining CSP/reporting/sinks, popup/navigation/sandbox, COOP/COEP and permissions policies | #28 |
| Fetch / storage | Dictionary publication scheduling and encoded Range behavior; broader streaming/cancellation/cache/signals/referrer and partition/storage-access behavior | #28 |
| API inventory | Every unresolved missing/extra member and descriptor/inheritance/behavior path in API-INVENTORY.json; fresh paired measurement | #28 |
| Distribution | Stock/native V8 alignment, native patch distribution, layout publication and registry-ready transport/crypto dependencies | #28 |
| Browser lifecycle | Watchdog/async-op admission recovery, timeout-signal delivery/GC, listener-count release and retained detached-window active-time suspension | #28 |
| Validation | Broader lifecycle regressions and additional conformance controls beyond the completed default/native/addon gates | #28 |
| Transport / GPU / Google | Unmeasured wire/rendering differences and reliable native-cookie SERP acceptance; current546 native baselines both end429/zero markers, while one Chrome baseline reaches a SERP and the other fails consent dismissal before typing; homepage boundedness remains open for clean and instrumented runs | #28 |
| Audio | Realtime/feedback/parameter connections/multichannel/custom waves/oversampling/suspend-resume | [#27](https://github.com/miaskiewicz/turbo-surf/issues/27) |

When an item is finished, record the relevant test/capture and commit before
checking its issue task. If a narrower follow-up issue is created, link it from
this table and #28 so the residual backlog stays complete.

### Measured abort/stream queue difference

The independent [Chrome control](./fixtures/abort-bridge-order-chrome155.json)
and [unadapted Deno baseline](./fixtures/abort-bridge-deno-baseline.json) retain an
exact open difference: Chrome runs stream sink-abort/cancel reactions before the
microtask queued by the signal listener; the native Deno backend runs that listener
microtask first. The same difference existed before the private event-array fix
and remains tracked in issue28. Event state, dependent reason identity and timeout
results are checked against Chrome separately; a passing state control does not
close this queue-order gap.

The private-array and reason-cache controls do not certify all descriptor paths.
Poisoning `Object.prototype.get`/`set` around the entire abort dispatch still reaches
another event-dispatch/exception-reporting path. The reason-cache read itself is
protected by a null-prototype descriptor; broader descriptor-hook isolation remains
open in issue28.

### Dictionary transport: implemented chain and remaining differences

[Saved independent evidence](./compression-dictionary-saved-evidence.json) shows
Chrome fetching/storing dictionaries, sending Available-Dictionary on87 requests
and decoding one dcb response, without an Origin-Trial response header. The older
frozen544180d executable received the Link but lacked discovery/store/decode.
That historical absence is repaired inec868b7/a15d098: partitioned origin-scoped
state, validated hashes/headers, eligible negotiation, bounded incremental
Brotli/Zstandard decoding and policy-aware discovery are integrated. Both final
workspace commands pass, including10 real HTTPS controls per transport and32
independent Chrome discovery/lifecycle controls on each engine.

Publication scheduling and encoded HTTP Range behavior remain open in#28.
Chrome's immediate post-GET request lacks the newly stored dictionary, while
+100ms/+500ms requests advertise it; native EOF publication is synchronous.
Stable delayed positives do not close that difference. [RFC9842](https://www.rfc-editor.org/rfc/rfc9842.html)
requires an eligible dictionary before advertising its encodings. Native explicit
header logs do not enumerate every final wire header. The bounded TLS/H2 and GPU
controls match within their measured scopes; none establishes Google's rejection
cause. See [dictionary controls and bounds](./COMPRESSION-DICTIONARIES.md).

## Earlier integration checkpoint (2026-10-16)

The following subsections preserve the sequence of earlier component results and
failures. Their pending/in-progress gate statements and API counts describe those earlier
checkpoints, not unresolved final delivery checks. The completed a005 validation
and fresh API/Google results at the top are authoritative for current status.

Canonical turbo-test51b5b97 is mechanically vendored with public DOM0.5.7
candidatefb60e6f (latest registry0.5.6). Genuine ElementInternals/CustomStateSet,
checked-state foundation, FormData events, disconnected-form admission and native
form reflection are integrated. The pushed544180d affected native renderer cohort passes
385 tests with zero failures/ignores, including312 library and73 integration tests.
Full all-target actual155 workspace strict checks pass. The earlier native MCP
checkpoint passes72/default71 tests; full final workspace/addon/shim/E2E gates
remain separate and are in progress.

Public immutable layout producer0.3.8 candidate9d7f02fb is integrated; latest
published registry release is0.3.7. Actual V815.5.35.23-rusty matches installed
Chrome155.0.8059.80's V8 patch; SDK source provenance remains155.0.8059.79.
Stock production SDK150 remains a distinct incomplete configuration.

The host-media, modern-CSS and elapsed-wait input failures are repaired. In the
latest two-repeat optimized544180d matrix, both native baselines complete real
consent/save204 and carry the genuine HttpOnly BUCKET cookie before search. Both
submit forms, receive/search200 and follow retry to/sorry429: zero native baseline
SERP successes in two fresh runs. Chrome baselines have9 markers each and zero
recorded errors. Native traced repeat0 fails with Missing promise after home
budget exhaustion; repeat1 reaches/sorry. Both native homes still report20s budget
termination. See [latest run](./GOOGLE-LAST-RUN.json).

An offline admission-window probe reproduces the SDK promise bookkeeping failure;
the pushed26a08b5 experimental155 transaction patch passes independent
preimage/recovery/rejection controls. Its managed-SDK scope, interrupted-reservation
retention and production-distribution bounds remain documented in
[ASYNC-ADMISSION.md](./engine-155/ASYNC-ADMISSION.md). The combined
default suite also found premature GC of an awaited timeout signal. Its realm-local
retention candidate passes the exact failing test with unchanged budget/Chrome
assertion; native retirement/forced-GC controls now pass within155 affected tests, and
all-target native workspace strict checks pass. Final integrated full gates remain
separate. A live or externally retained document currently retains pending timeout frontends even without public
listeners until timeout fires. Listener-count early release and retained detached-
window active-time suspension remain open. Reliable native acceptance is unresolved.

The runtime follow-up default workspace gate passes1,312 tests across120 suites,
with three explicit native-CSP capability skips and six generated doctest skips.
Default all-target strict checks and formatting pass. Native155 affected tests
pass155/155 and native all-target strict checks pass. These results precede the
next canonical DOM/dictionary integrations; addon/shim/E2E, final integrated gates
and fresh live validation remain pending.

### Linux font control and current source integrations

CI for26a08b5 passes18 geometry controls and fails the UTF-16 geometry test because
it compares exact macOS Chrome system-font measurements with Linux's available
fallback fonts. Preserve the full exact macOS control; separately assert original
code units, all selected boundaries, private access, retained caret identity and
lifecycle on every platform. This test correction does not certify Linux glyph or
fallback parity. The correction is pushed as6f9ebc7 and survived the subsequent CI run. A matching Linux
Chrome/font capture remains open under#28.

Pusheda15d098 vendors registry provenance, submitter reflection and genuine live
form collections from canonical51b5b97. The actual155 production cohort passes two real iframe/collection-GC tests,
12 form-navigation tests including reflected submitter POST overrides, and
23 geometry controls. The final actual155 combined component cohort passes46 tests, including the
corrected radio GC policy and all32 exact dictionary task/lifecycle controls.
Stock-engine renderer library tests pass326/326; its dictionary and radio
integration tests also pass. Core passes220 tests, including real HTTPS dictionary
controls on both transports and incremental output/cancellation. Independent
Chrome controls and official dictionary-dependent Brotli/Zstd fixtures retain
source provenance.
Do not close these rows or refresh measured inventory counts until the final
integrated executable is rebuilt and captured.


### Labels and native timer clock follow-up

Pushed5c0b886 mechanically vendors canonical1e93cae. Seven existing
labelable interfaces have genuine native `.labels`; controls cover live identity,
hidden transitions, adoption, first-ID/empty-ID rules, foreign namespaces,
custom-control ordering and hostile public methods/constructors. The NodeList
factory follows the receiver's creation realm. Actual155 production tests pass
both behavior/iframe and retained-list/retired-realm GC controls. Full label
activation is not claimed.

The full native run exposed an independent clock difference: a future virtual
Window timer could overtake an earlier real Deno timeout or network body. The
source fix keeps one precise monotonic wake while native work is pending,
without starving a Window deadline that must abort stalled I/O. Fresh actual
Chrome controls pin four outcomes, including timeout20ms/Window30ms after GC
and with a held signal lacking public listeners. Focused native27 and stock9
checks pass before the additional held-timeout fixture integration; the held
case separately passes actual155. Full native/default retries are in progress.
The offline harness passes78 tests with four explicit skips, plus two Python
SDK-helper tests. Quiescent virtual fast-forward and broader browser timer policy
remain explicit limits; no new native realm root or speculative timer capability
is added.

### Peer layout conformance timing

The stock full retry records1189 passing tests across101 completed suites, plus
one failure and three explicit skips: the peer-layout fixture exhausts its
5-second call budget. An isolated run reproduces the failure.
A native process sample locates the stall in one-time system-font discovery and
font parsing, before the scalar geometry result; this is not evidence of a
Promise, timer or document-ownership leak. The peer-layout conformance fixture now has an explicit60-second budget while
retaining every31/73/97 width,
interleaving and replacement assertion. Production limits and font performance
are unchanged. The current-source shadow8/8 and isolated peer1/1 pass. The sampled failure
and fresh focused proof are retained separately.

### Historical combined-run follow-ups

The actual155 full retry exposed a human-interaction completion failure. Its exact
MCP executable passes the isolated test and a later72-test run; default MCP also
passes. The former driver started a sequence Promise but returned undefined,
leaving completion dependent on incidental draining. The source fix now awaits
that actual final Promise while preserving native form commits and their retired
gesture realm. Actual155 MCP73/73 passes, including the new async-form/forged-marker
and rejection regression. Product budgets are unchanged; final full gates remain.

A separate adversarial probe confirms private abort backend values can escape
through guest conversion hooks. The repair needs both private iterable conversion
in the frontend and a source-pinned dense-array adapter in the official WebIDL
sequence converter. Both pieces are integrated and actual155/stock hostile-hook
regressions pass; the wrapper alone was insufficient. Broader descriptor hooks and the separately
measured stream/listener microtask order remain open.

CI for a15d098 independently reproduces the timeout-order failure on Node20;
the source clock fix addresses that exact unchanged assertion. Node22 reaches
the native storage lifecycle fixture and exhausts its5-second debug execution
budget during600 create/mutate/remove operations. Its conformance-only budget is
now60 seconds; the600 operations,1200-node reclamation assertions, identity and
peer-isolation checks are unchanged. Production execution budgets are unchanged.
The corrected storage fixture passes the final actual155 affected cohort;
this is not a performance claim.

The private abort/descriptor repair passes its initial actual155 regression
with the pinned WebIDL adapter. The final source also replaces a mutable
Array.includes decision that could skip stream-controller wrapping. Its independent
Chrome control and native regression pass. Final actual155 affected privacy2,
stream7, clock3 and storage2 tests pass14/14 with zero skips; stock private
regressions2/2 and pinned adapter units2/2 pass. Exact
Chrome array fast-path behavior remains a separate measured difference: Chrome
skips a poisoned inherited Array iterator, while the current wrapper calls it
once. The regression preserves that count difference and asserts no private raw
value escapes. It stays in#28; no guessed Proxy fast path is added.

Full runs and their failures remain recorded separately from component passes.
The stock full run initially failed linking due to disk exhaustion before tests;
a retry later passed MCP and328 renderer unit tests before a controlled cleanup
pause. That retry continues with its exact compiled source. The newly authored
security/driver follow-ups require their own focused validation and updated
combined scope before final gate or live-acceptance claims.

### Cancellation observation in final full validation

The final native no-fail-fast run exposes one Core cancellation regression
assertion: it checks transport closure immediately after worker permits recover.
Those are different completion events because aborting the producer schedules
its teardown. The test now awaits the actual sender-closed notification under
its existing10-second timeout, then retains the closed-state assertion. Worker
recovery is still checked separately. Production cancellation, caps and budgets
are unchanged. The original failing native executable/log and corrected source
are retained. The corrected complete Core unit cohort passes195 tests, and both
final full workspace commands above include the fix and exit0.

## Active scope and deferred audio

Finish messaging, layout, Range/DOM, security/resources, fetch/storage, combined
validation, the current member inventory, PR housekeeping and fresh live Google
validation. Remaining advanced audio is a documented TODO in [issue #27](https://github.com/miaskiewicz/turbo-surf/issues/27),
deferred at the user's request. Existing offline DSP and tests remain included.
The authored adapter now explicitly selects `std::exp2(float)` on every target;
all 176 independent Chrome frequency-response cells match exactly on Linux x86,
macOS ARM and Rosetta x86. This fixes a platform overload discrepancy without
numeric tolerance or changes to licensed kernels; a Linux Chrome browser audio
comparison remains unmeasured.
Reprioritize deferred audio only with captured detector evidence showing a missing
path matters to the target; no Google-audio cause is established by these controls.

## Fresh own-member inventory (2026-10-16)

[API-INVENTORY.json](./API-INVENTORY.json) retains all measured missing/extra,
descriptor, inheritance, callable and key-order paths. The new paired localhost
capture uses Chrome155.0.8059.80 and actual native V815.5.35.23-rusty with all
wreq/trust-anchor/Metal/CoreText features. Executable SHA256 is
`84d22febc3fde7cf13730e98f688814b8da259a781bf1d183c8631abe0e42f50`.

All **75 interfaces are measured**. There are **64** missing own members in the
original four (Document29, Element29, HTMLElement4, Canvas2), **142** across the
expanded scope, and **374** total difference paths. HTMLFormControlsCollection and
RadioNodeList are now present with zero missing/extra/listed-descriptor differences.
A measured own-member match does not certify complete behavior or key-order parity.

Across the same **68 previously measured interfaces**, missing own members fall
from **134 to119**, with **15 resolved and none introduced**. The five added
interfaces contribute23 additional missing members; the overall expanded totals
therefore have a different denominator. The two formerly absent interfaces are
measured separately from that same-scope comparison. The prior70-interface
snapshot (65 original/134 expanded,349 paths) remains in
[commit95e4c5c](https://github.com/miaskiewicz/turbo-surf/blob/95e4c5c/harness/detection/API-INVENTORY.json).
Historical94 and earlier66/147 counts are not current measurements.

## Current measured missing members

Generated from the fresh snapshot. A missing own member can still have inherited
or instance behavior; zero missing members does not prove conformance. All extra,
descriptor, callable and raw key-order differences remain itemized in the JSON.
Every unresolved path is tracked in issue28.

| Interface | Missing own prototype members |
|---|---|
| Blob | `textStream` |
| Document | `all`, `ariaNotify`, `browsingTopics`, `domain`, `execCommand`, `exitPictureInPicture`, `exitPointerLock`, `fragmentDirective`, `fullscreen`, `fullscreenEnabled`, `getAnimations`, `hasPrivateToken`, `hasRedemptionRecord`, `lastModified`, `pictureInPictureElement`, `pictureInPictureEnabled`, `pointerLockElement`, `prerendering`, `queryCommandEnabled`, `queryCommandIndeterm`, `queryCommandState`, `queryCommandSupported`, `queryCommandValue`, `startViewTransition`, `timeline`, `wasDiscarded`, `webkitCancelFullScreen`, `webkitExitFullscreen`, `webkitFullscreenEnabled` |
| Element | `activeViewTransition`, `animate`, `ariaNotify`, `checkVisibility`, `computedStyleMap`, `currentCSSZoom`, `getAnimations`, `getHTML`, `hasPointerCapture`, `onbeforecopy`, `onbeforecut`, `onbeforepaste`, `onfullscreenchange`, `onfullscreenerror`, `onsearch`, `onwebkitfullscreenchange`, `onwebkitfullscreenerror`, `pseudo`, `releasePointerCapture`, `requestPointerLock`, `scroll`, `scrollIntoViewIfNeeded`, `setHTML`, `setHTMLUnsafe`, `setPointerCapture`, `startViewTransition`, `webkitMatchesSelector`, `webkitRequestFullScreen`, `webkitRequestFullscreen` |
| EventTarget | `when` |
| HTMLButtonElement | `command`, `commandForElement`, `interestForElement`, `popoverTargetAction`, `popoverTargetElement` |
| HTMLCanvasElement | `captureStream`, `transferControlToOffscreen` |
| HTMLElement | `attributeStyleMap`, `editContext`, `focusGroup`, `focusGroupStart` |
| HTMLInputElement | `align`, `alt`, `createValueRange`, `dirName`, `files`, `height`, `incremental`, `indeterminate`, `list`, `maxLength`, `minLength`, `popoverTargetAction`, `popoverTargetElement`, `showPicker`, `size`, `stepDown`, `stepUp`, `useMap`, `valueAsDate`, `webkitEntries`, `webkitdirectory`, `width` |
| HTMLMeterElement | `high`, `low`, `max`, `min`, `optimum`, `value` |
| HTMLOutputElement | `defaultValue`, `htmlFor`, `name`, `type`, `value` |
| HTMLProgressElement | `max`, `position`, `value` |
| HTMLSelectElement | `Symbol(Symbol.iterator)`, `add`, `item`, `length`, `namedItem`, `remove`, `showPicker`, `size`, `type` |
| HTMLTextAreaElement | `cols`, `createValueRange`, `dirName`, `maxLength`, `minLength`, `rows`, `textLength`, `type`, `wrap` |
| MessageEvent | `userActivation` |
| Range | `expand` |
| ShadowRoot | `activeElement`, `clonable`, `delegatesFocus`, `fullscreenElement`, `getAnimations`, `getHTML`, `getSelection`, `onslotchange`, `pictureInPictureElement`, `pointerLockElement`, `referenceTarget`, `serializable`, `setHTML`, `setHTMLUnsafe`, `styleSheets` |

No native interface is absent in this75-interface capture. This does not enumerate every browser interface.

## Behavior and architecture gaps beyond member names

| Surface | Concrete remaining work | Evidence / status |
|---|---|---|
| DOMTokenList | Stable identity, private brand, indexed/live tokens, mutation/error contracts; `part` | Canonical cached/live implementation passes offline mutation/brand contracts; fresh native 155 pair has zero differences for the saved probe |
| ARIA element relationships | ID resolution, tree scope, explicit element assignment, attribute clearing, frozen returned sequences | Scoped/private canonical state implemented and tested; fresh native 155 pair has zero differences for the saved probe |
| Layout and hit testing | Inline behavior beyond measured decorated/empty/mixed-font boxes; broader stacking, scrolling/RTL, iframe migration and layout-unit behavior | Native0.3.8 candidate drives real rectangles, client/offset/scroll and point hits; original UTF-16 Range/caret, outer SVG, rotation and composed shadow controls pass. See [current bounds](./LIVE-GEOMETRY.md) |
| Rendered text | Complete layout-dependent wrapping/line-box behavior beyond native cascade text flow | Canonical visibility/whitespace/break/table/details/setter contracts implemented; fresh native 155 pair has zero differences for the saved probe |
| Shadow/custom elements | ElementInternals/form association, broader upgrades/reactions and scoped-registry lifecycle | Native slots/manual assignment, closed roots, scoped registries and construction/adoption controls are integrated; ElementInternals and CustomStateSet now pass native behavior/GC controls |
| CSS parsing/layout | Complete grammar/cascade, Typed OM, animation timelines and transitions | Measured stylesheet policy controls pass; complete CSS behavior is open |
| Live stylesheet installation | Explain Google's jc/lc reports during actual execution; compare live ownership and rule state | Isolated CSSOM controls pass. October16 native passes all emit jc/lc before search; the October17 rerun has no selected jc/lc reports in any of8 captures, using a historically positive-controlled extractor. GOOGLE-DIAGNOSTICS.json corrects the earlier consent request/response boundary. Changing telemetry does not establish a semantic fix or rejection cause |
| Observers | Real Mutation/Resize/Intersection/Performance observer queues and lifecycle | Native MutationObserver semantic records, microtask delivery, options/old values/transients and a private postconnection consumer are integrated. Full mutation lifecycle and Resize/Intersection/Performance queues remain open |
| Browser lifecycle/resources | Complete parsing/resource scheduling, frame navigation, media and native worker lifecycles | Tested subset only; native workers unsupported |
| Frames/security | Complete WindowProxy/DOM policies in production SDK, opaque/sandbox navigation behavior | Stronger native proxy profile is experimental; default SDK coverage partial |
| CSP | strict-dynamic, report-only/reports/violations, navigation/form/worker/media directives | Enforced supported script/style directives tested; remaining directives open |
| Related resource policies | Full policy effects/reporting, passive upgrades, Integrity Policy and Critical-CH | Tested SRI, Trusted Types, mixed-content/CORP and COOP/COEP/Permissions Policy capability/delegation are integrated. Complete resource behavior remains open |
| Fetch lifecycle | Broader cancellation/cache/signals/referrer/preflight-cache behavior and genuine multipart BodyInit | Genuine response/HTTP2 upload streams, initiating policy snapshots, cache effects and stream transfers are integrated; bounded controls do not establish complete Fetch conformance |
| Cookies/privacy/storage | Complete storage-access, third-party browser policy and private-token APIs | HttpOnly cookie protection, actual partitioned/credentialless storage and peer/clear events are integrated; broader eligibility/policy remains open |
| Element interfaces | Exact HTML namespace/local-name mapping, native borrowed control brands and SVG interfaces | Canonical135 cohort passes the authored723-row matrix's HTML/null mappings and foreign brands. All SVG observations remain saved; SVGScriptElement/SVGImageElement projection/resources and broader custom-element lifecycle are open |
| HTML/XML parsing | Broader XML/sanitizer/template/parser lifecycle and UTF-16 edge cases | Published DOM0.5.6 provenance/template ownership and original-unit storage are integrated; public Git-pinned DOM0.5.7 d01d6a5 preserves actual-context UTF-16 fragments and adds measured attribute-modifier fixes, with cross-document/parser/observer/script controls |
| Structured clone | Complete web-interface serialization and transfer lists | Native V8 clone, genuine ArrayBuffer/resizable-buffer transfers, DOMException, private Rust-only CryptoKey snapshots, borrowed realms, User Timing, Window/MessagePort delivery and genuine interface rejection have saved Chrome controls. Current integrated transfer24/clone17 contracts pass. Endpoint weak anchors, reservations and queued native material enforce admission before ownership commit; retired context registries, timer roots and Window queues have bounded retirement, reentrant-transfer and initialization-failure controls. Weak native ownership/cppgc tracing and private DOMRect snapshots are integrated; broader native ownership and detached-node reclamation beyond the bounded integration tests, Blob/File, SAB exposure and wider serialization remain open |
| BroadcastChannel | Shared cross-realm/tab/browser-session delivery and lifecycle | Local asynchronous delivery/brands tested |
| Range | Remaining composed-tree/lifecycle/geometry edge cases | Native live mutation boundaries and actual text/Range/caret geometry are integrated. AbstractRange/NodeRange/StaticRange passes independent native interfaces, iframe and GC controls |
| Canvas/media | Offscreen transfer, captureStream, audio/media synthesis and media lifecycle | Metal pixels match shared probe. Genuine bounded native offline audio now computes independent Chrome ARM64 PCM/hash controls, with private parameter/buffer ownership and trusted lifecycle tests; scheduled sources, automation and five effects have32 exact authored ARM64 PCM controls and are included in the final passing workspace gates; realtime, feedback, parameter-signal connections, custom waves, oversampling, suspend/resume and multichannel beyond two remain unsupported. Other media APIs remain incomplete |
| Crypto | Real Chrome 155 encapsulation/decapsulation/getPublicKey/supports behavior and supported algorithms | Official native Deno WebCrypto integrated; hostile inherited `keyData` setter regression discovered a raw-byte leak during non-extractable import, hardened licensed provider passes inherited-property isolation and the final native155 workspace gates. Distribution remains open. Covered operation controls: real digests, AES/HMAC/Ed25519, ML-KEM-768 and ML-KEM-1024 (512 rejected by the public Chrome profile), keys/permissions/realm metadata pass offline tests; paired155 audit matches285 operation/name and14 parameter controls; final integrated workspace gates pass |
| Hybrid KEM | Genuine MLKEM768-X25519 (X-Wing) key generation/import/export and encapsulation/decapsulation | Real native X-Wing managed keys, raw-seed/raw-public/JWK, permissions, wrapping and encapsulation implemented; known Chrome seed vector and both ciphertext directions pass. Public/native hybrid, child realms and bootstrap controls are included in the final passing genuine155 workspace gate; distribution remains open |
| Browser identity/version | Real V8 155, measured transport 155, coherent UA/client hints/headers/environment | Actual native V815.5.35.23-rusty/profile155.0.8059.80 differs from frozen Chrome155.0.8059.85/V815.5.35.25. Bounded local155 TLS/H2 fields use an upstream154 base recipe; patch, stock150 and distribution alignment remain open |
| Enumeration | Global and Navigator property ownership/enumerability and behavior | Fresh same-origin clean probe-only pairs count1235 Chrome/1240 native Window own properties. Earlier insecure/secure comparisons are historical; current API descriptors/order are saved separately |
| Input execution | Stop at private input completion, bound synchronous runaway, cancel queued edits | Eight readiness/input regressions have passed profile cohorts. Private bootstrap arguments replace temporary public host globals; the full combined run exposed a termination race during frame setup, fixed with a checked lookup; default and genuine155 final combined gates pass |
| Classic script errors | Preserve original exceptions, trusted Window reporting and fetched-script load success; mute no-CORS cross-origin reports | Current default main/iframe and CORS controls pass; fresh Chrome155 authored fixture saved. Genuine155 paired audit pending; Window onerror registry and wider error handling in progress |
| Element event propagation | Include actual Window in native node propagation path with capture/phase/stop/composedPath semantics | Canonical bf2a0ef passes 133 Rust and 63 JS tests, including private generic/Window/Node/Signal collection regressions. Generic EventTarget and AbortSignal listener/handler records are private; direct-target registration order follows fresh Chrome155 controls. Surf integration removes duplicate manual callbacks/DCL and public abort stubs; the earlier integrated native155 gate passed; six current event contracts and final default/native workspace gates pass. Timeout/any, full composed-shadow retargeting and listener exception reporting remain open |
| Live search | Reliable fresh consent+query+retry acceptance without imported Chrome cookies | Latest October17 source59f7835/native executable537c910b: native baseline0/2; Chrome baseline7/8 markers. Native uses fresh cookies, real consent and genuine forms with no fallback; all native passes end/sorry429. Both instrumented Chrome passes are challenged. The earlier same-day88528ae matrix had both Chrome baselines blocked; it is historical, as are October16 Chrome9-marker controls. Native rejection cause remains unproven |
| Publishing | Clean registry consumers need compatible published DOM and transport APIs; verify per-platform artifacts | Latest published DOM0.5.6 and layout0.3.7 are distinct from pinned public DOM0.5.7/layout0.3.8 candidates; registry-ready transport APIs, distributable hardened crypto provider and per-platform Surf artifacts remain unresolved |

## Closure record

Keep historical observations dated in [FINDINGS.md](./FINDINGS.md). Update this
backlog and the sanitized API snapshot after each rebuilt paired measurement.
Regenerate the durable inventory with `node harness/detection/api-inventory.mjs
<capture>/comparison.json harness/detection/API-INVENTORY.json`. It preserves every
observed difference path, including globals, callable descriptors and raw key
order, alongside the missing-member summary and source/executable hashes.
Missing members tied to browser features cannot be closed by adding no-op methods,
fabricating native results, or changing a version string. Consent handling is a
saved default in run.mjs and both Chrome collectors, with a localhost regression;
native checkpoints retain partial state when execution is interrupted.


### Linux CI initializer correction

The `95e4c5c` Linux Node 20 and Node 22 jobs pass the Rust tests, including all
nine shadow layout controls, then fail strict Clippy on the non-macOS HostMedia
initializer.
Construct the pointer/hover overrides with a struct update rather than assignments
after Default. A standalone exact non-macOS capture-body proof reproduces the
Clippy failure before the fix and passes afterward, including both native-pointer
values. This changes no media-policy or production-budget behavior; fresh CI
still validates the actual Linux build.

### Bounded XMLHttpRequest correction

The independent Botto oracle identifies XMLHttpRequest's `[object Object]` brand
on the preserved default stock150 addon. The existing implementation performs
real native Fetch requests; its anonymous class and later generic setup skipped
the standard tag. The fix names the constructor, installs Chrome's actual
prototype/global/constant and six network-method descriptors, and validates
receivers with captured private WeakMap operations. It does not add unsupported
service-worker behavior or fabricate native function source.

An independent Chrome155.80/V815.5.35.23 control pins the probe/collector/server
sources and matches descriptors, forged/proxy/subclass/detached receivers,
hostile WeakMap hooks and actual HTTP state1/2/3/4/body delivery. The default
control plus four existing HTTP/frame tests and full all-target strict/fmt pass.
Actual155 also passes all five affected tests. Both current default and native155
Botto captures report zero strong findings, with only the weak unsupported
service-worker finding. These bounded detector results do not establish Google acceptance.

Broader XHR writable/public state, EventTarget hierarchy, upload/progress,
synchronous/timeout/transport-abort behavior, cross-realm borrowed receivers and
reentrant loadstart/send state admission remain explicit API/Fetch/lifecycle work
in#28. The bounded repair does not certify complete XHR conformance.
