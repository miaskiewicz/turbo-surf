# CSP reporting controls

The committed controls use fresh installed normal Chrome 155.0.8059.79 / V8
15.5.35.23 and localhost resources. Each control pins its probe and collector;
header controls also pin their cases. The native tests compare the original
independent results rather than regenerating expected values from Turbo-surf.

Implemented and checked:

- SecurityPolicyViolationEvent dictionary conversion, native receiver brands,
  readonly descriptors, inheritance, original constructor identity and fields.
- Trusted asynchronous enforced/report-only inline script, stylesheet,
  style-attribute and fetch violations, actual initiating V8 source locations,
  original policy text, connected target and microtask/task ordering.
- Private producer ownership and captured timers/constructors; queue admission
  bounded to4096 pending tasks and64MiB, capacity released before guest handlers.
- Real capacity termination/recovery, reentrant throwing listeners, guest
  constructor/timer overrides and retired iframe cancellation.
- Early CSS reports retained as genuine target/metadata V8 edges in a private
  context-global array, flushed/deleted on bootstrap, with scalar native quota
  bookkeeping. No native Global or raw node handles are added.
- Opt-in parser source locations come from actual original UTF-16 tokenizer
  consumption and native node/attribute installation identity. Fourteen complete
  Chrome controls match exact locations, targets and event order for CRLF,
  non-BMP prefixes, duplicate tags, malformed foster parenting, reconstruction,
  later body attributes, inert template attributes and inert stylesheets.
  Synthetic/reconstructed node lexical spans remain absent; their attribute
  installation context is recorded separately. Initial private bootstrap reports
  use stable original offset order. Dynamic reports keep actual V8 stack locations.
- Parser report delivery relies on the pre-existing classic-script-before-timer
  scheduler fix from Surf commit69bfaa9. Successful inline classic/module scripts
  execute without load events, matching the independent200ms control.

- Enforced external classic/module script and image violations now match a
  dedicated normal Chrome control. External script reports use the actual
  initiating stack and Document target; image reports use empty source metadata
  and arrive after synchronous script/style checks. Native image owner/base
  getters ignore poisoned public ownerDocument. Report samples retain original
  UTF-16, truncate to40 units (including split/lone surrogates), remove only
  measured ASCII whitespace and preserve NBSP/FEFF. The native prefix conversion
  uses bounded128-unit scratch and accounts sample bytes before admission.
- The actual155 SDK now supplies a native V8 code-generation callback and passes
  the full resource/eval oracle. Seven fresh controls cover report-only execution,
  direct lexical scope, Function/AsyncFunction/GeneratorFunction, non-string and
  boxed identity, coercion failure, unsafe-eval, default-src fallback, directive
  intersections and live meta policy. Eval samples truncate the original prefix
  before whitespace trimming; inline samples trim before truncation. The dedicated
  collector explicitly sets allowUnsafeEvalBlockedByCSP:false because [CDP
  Runtime.evaluate defaults to an eval bypass](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/).
  Native code-generation guards poll termination, share the existing bounded
  event queue and recover after actual4097th-attempt termination/reentry. The SDK
  stores a per-context native Function in a private global edge; public symbols
  cannot forge it. Registration rejects foreign-realm, bound and proxy callbacks.
  No native Global/queued node handles or JS eval wrapper are
  added. Default SDK150 lacks the callback and keeps explicit guarded tests;
  production engine alignment remains required before equivalent support is
  claimed for ordinary shipped binaries.

- Authentic document response status and selected navigation referrer replace
  hard-coded report values. Twenty-eight normal Chrome controls cover real error
  responses, same/cross/no-referrer/unsafe-url navigation, redirects and
  blank/srcdoc/data frames. Actual private document ownership ignores guest
  referrer/URL getter overrides. Blank documents copy the creator URL; srcdoc/data
  navigation uses the selected policy, with data status200 and blank/srcdoc status0.
  CSP documentURI removes fragments and serializes non-HTTP schemes; actual
  root/frame Location keeps authored fragments independently of report formatting. Data frames
  inherit the creator policy. Caller response headers/metadata are carried into
  MCP and NAPI hydration; shim init scripts use the trusted native entry point
  without adding parser tokens or being rejected by authored script CSP. Native
  iframe referrerpolicy overrides the parent selection, normalizes enumerated
  keyword case and rejects whitespace-padded values. Initial blank retains the
  creator URL; srcdoc/data apply the measured selected navigation policy.

- Parsed and dynamic meta referrer policies have 20 source-pinned Chrome
  controls, including actual header precedence, body processing, legacy aliases,
  name/content mutation and retained last-valid state after invalid input/removal.
  Template, shadow and foreign nodes remain inert; native attributes ignore
  public getter overrides. Ordered parser policy activation preserves script
  boundaries. The true associated-template dependency is an unpublished reviewed
  candidate; imported inactive storage does not invent new lexical source tokens.
  Parser-time style attributes inside template contents still use the parsing
  document's CSP and produce the measured document-targeted report with zero
  source positions. Native traversal follows the actual private template graph;
  template stylesheets remain inactive. Fresh normal Chrome reconfirms all 14
  original parser controls. The combined policy cohorts pass 71 on genuine 155
  and 68 on default 150 with three explicit native capability guards.

- Native request preparation captures the initiating CSP, referrer and embedder
  policy before any upload body await. Immutable policy data uses copy-on-write
  updates, so a later meta mutation affects subsequent requests without changing
  an existing job or duplicating the full policy per pending upload. Jobs retain
  no guest document or realm. Twelve independent Chrome controls use real HTTP
  echo headers and cover parser phases, explicit Request policies and Blob/CSP
  timing. Inert template scripts do not advance active parser phases, and
  template metadata cannot change the active CSP or initial base policy.
  The final combined cohort passes 86 genuine 155 and 82 stock 150 contracts
  with three explicit native capability guards. Both strict renderer checks and
  source pins pass. Genuine 155 executes the foreign template adoption control;
  stock 150 retains its separate explicit guard.
  Poisoned or throwing Request URL prototype getters cannot redirect transport;
  the request uses its captured private URL without invoking guest getters.
  Request upload streaming and broader BodyInit behavior are still
  separate gaps; these controls certify policy metadata, not every upload shape.

Native HTTP reporting uses profile-owned bounded jobs (4096 and64MiB), original
native metadata and response cookies. Jobs hold only primitive bytes/request
context, a native client, bounded queues and the profile cancellation receiver.
Removing a reporting frame cancels its pending DOM delivery while the native
upload survives; collecting the retired realm does not retain a V8 root. Disposing
the final profile owner closes a stalled report transport.

Twenty-five independently authored cases are collected over HTTP and HTTPS in
normal installed Chrome155; the HTTPS localhost certificate is pinned by SPKI
and the recorded flags do not modify reporting timing. Legacy uploads use
application/csp-report, no-cors/report metadata, same-origin credentials, the
selected referrer policy, genuine client hints, and error redirects. Modern
uploads use application/reports+json, cross-origin CORS preflight, no referrer,
Fetch Metadata, client hints, Priority or Accept header; same-origin cookies are
admitted for Reporting-Endpoints. Modern302 redirects becomeGET and307 retain
POST. report-to takes precedence even without a configured group. Meta excludes
legacy report-uri and permits report-to. Deprecated Report-To controls remain
explicitly separate because its observed uploads omit cookies.

Legacy samples preserve Chrome's measured WTF8 encoding of lone UTF16 units;
modern samples reproduce the measured UTF8 replacement bytes. Serialization and
network dispatch do not invoke guest fetch or JSON/toJSON hooks. Raw credential
admitted response observers run before CORS checks and receive no cached headers;
storage/cache integration must supply a native backing-only callback, with a
trusted private-partition override for credentialless jobs.

Remaining measured or explicit gaps:

- Broader module/allowed report-only request stages, failed requests and redirect
  event stages still need separate controls before request-lifecycle parity is
  claimed. The measured report-only fetch emits two DOM events and one legacy
  endpoint upload; duplicate report-uri tokens produce distinct uploads.
- Reporting API batching, delivery timing, retries, weighted endpoint selection
  and persistent deprecated Report-To endpoint lifetime are outside the current
  endpoint controls. The selected configured endpoint is sent best effort.
- Full sandbox form/popup/navigation/download flags and opaque inherited-origin
  identity are separate pending security work.

Audio remains deferred under issue27. No live Google SERP success is established
by these offline CSP controls.
