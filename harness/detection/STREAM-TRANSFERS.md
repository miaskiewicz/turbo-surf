# Native stream transfers

ReadableStream, WritableStream and TransformStream use the official Deno raw
stream algorithms behind private realm-local frontends. Transfer transport uses
Surf's genuine native MessagePort endpoints. Deno resource IDs are never treated
as Surf endpoint IDs.

Preparation validates stream brands, locks and duplicate explicit entries, then
allocates private channels before graph serialization. After getter execution and
queue admission, transfer ownership is checked again before ArrayBuffer detach,
stream locking and endpoint generation changes. Native buffers commit first,
then streams, then public and hidden ports, as measured in Chrome. A source pull
can synchronously invoke page code during the stream phase. Revalidate port
generations afterward so recursive port transfers fail safely rather than
committing a stale endpoint. A failed graph or queue admission
leaves buffers attached and streams unlocked. Chrome deliberately partially commits
overlapping TransformStream/component transfers; the saved controls check the
resulting locks and detached buffer instead of imposing stronger atomicity.

Queued snapshots retain numeric stream kinds and native endpoint generations.
Original stream wrappers and prepared plans have strong handles only during the
synchronous serialization transaction, and those handles are cleared before a
snapshot escapes. Hidden endpoints do not appear in MessageEvent.ports. Port
anchors and private interface ephemerons provide reachability; there are no
unconditional host roots for source realms.

The private channel protocol carries pull, chunk, close and error messages.
Chunks and cancellation reasons pass through the same genuine structured clone
implementation as ordinary port messages. Cloning errors propagate to both sides.
Readable receivers use zero high water mark, writable receivers acknowledge pull
backpressure, and received byte streams are ordinary readable streams, matching
Chrome. Captured raw methods and private frontend/port adapters avoid consulting
page replacements during transfer.

Detached source readable streams stop producing transport chunks. Source writable
callbacks can remain usable through a transferred endpoint. Reads from a retained
receiver in a detached realm reject with the measured TypeError. Completed and
abandoned transfers release endpoint graphs and allow source-frame collection.

## Reusable controls

Run `node harness/detection/authored-upload-chrome.mjs PROBE OUTPUT` with a fresh
normal installed Chrome profile. The localhost fixture trusts only its checked
certificate SPKI. Each JSON control records browser/V8 versions and SHA-256 hashes
of its exact probe, collector and fixture. Probes are self-contained expressions;
serial cancellation and prototype replacements are intentional test inputs.

- `stream-transfer-probe.js`: 20 baseline observations, including Window/port
  delivery, ownership, aliases, brands, backpressure, EOF and cloned reasons.
- `stream-adversarial-probe.js`: nine overlap, BYOB downgrade, uncloneable
  reason/chunk and retransferring observations.
- `stream-retirement-probe.js`: three actual frame-retirement outcomes.
- `stream-cancel-context-probe.js`: distinct ReadableStream.cancel,
  ReadableStreamDefaultReader.cancel and WritableStreamDefaultWriter.abort errors.
- `stream-reentry-probe.js`: synchronous source pull locking a later stream
  preserves Chrome's partial commit effects.
- `stream-order-probe.js`: five buffer/stream/port phase and recursive-transfer
  controls, including rejection of a public port transferred during source pull.
- `stream-poison-probe.js`: captured MessageChannel, port postMessage and pipeTo
  despite page replacements; the original source unlocks after EOF in Chrome.

`cargo test -p turbo-surf-render --test stream_transfers` consumes those exact
controls. Additional tests exercise queue-admission rollback, 64 failed graph
plans followed by endpoint reclamation, and actual native weak-reference
collection of completed source frame/stream/receiver/reader objects. Run existing
`crypto_clone_contracts` and `message_transfer_contracts` alongside this binary.

The controls cover these measured cases, not every browser stream lifecycle or
transferable host type. Production engine-version alignment, unrelated storage
policies and native Google SERP success are separate validation work.
