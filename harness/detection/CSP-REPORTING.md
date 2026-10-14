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
- Parser report delivery relies on the pre-existing classic-script-before-timer
  scheduler fix from Surf commit69bfaa9. Successful inline classic/module scripts
  execute without load events, matching the independent200ms control.

Remaining measured or explicit gaps:

- The full parser control records real document source line3/columns8 and61.
  Native delivery/URI/targets/directives/trust match; canonical parser positions
  are not exposed yet. The parser test intentionally compares delivery fields
  while preserving the full source-pinned control for the additive backend seam.
- HTTP report-uri / Reporting API endpoints and additional external, module,
  image, eval/Function and redirect violation reporting are unfinished.
- HTTP document status/referrer metadata currently covers200/empty-referrer;
  broader navigation metadata must come from an authentic host context.
- Report samples need exact UTF-16 truncation/conversion, including lone units.
- The measured report-only fetch case emits two reports; other request stages,
  cancellation/failures/redirect cases still need separate controls before broad
  request-lifecycle parity is claimed.
- Full sandbox form/popup/navigation/download flags and opaque inherited-origin
  identity are separate pending security work.

Audio remains deferred under issue27. No live Google SERP success is established
by these offline CSP controls.
