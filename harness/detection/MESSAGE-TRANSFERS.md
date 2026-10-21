# Native message ownership

The reusable `message-transfer-probe.js` control was run against normal headed
Chrome 155 without interface tracing. The checked fixture records its exact
browser/V8 version and results. Offline Rust contracts cover the same ownership
rules and add foreign realms, receiver brands, event ordering and hostile
prototype hooks. The initial transfer implementation passed genuine V8 15.5.35.20-rusty gates.
The current isolated GC/key follow-up passes 62 contracts: 24 transfer, 16 clone,
4 hybrid integration, 6 standalone native/module, 4 event and 8 bounded-input
contracts. Strict all-targets Clippy and formatting pass. The source is integrated
locally; combined default and genuine V8 155 production gates remain pending.


The port registry belongs to the V8 isolate. JavaScript wrappers hold a private
native endpoint identifier and ownership generation; there are no public peer,
queue or listener fields. Transfers validate the entire list and serialize the
graph before any ArrayBuffer detachment or endpoint owner change. A failed getter,
duplicate list or self-transfer preserves all source owners. A successful transfer
invalidates the original wrapper and recreates the port in the receiver's realm.
Already-queued messages stay with the endpoint and run asynchronously after the
new receiver enables its queue. Closed ports remain transferable; sends on closed
or detached sender ports do not serialize their payload. Transferring the receiving
endpoint through its peer serializes and detaches ownership, then discards delivery
and disentangles the endpoint. Active senders still serialize even after their peer
is gone. Port overloads, invalid iterator entries and transfer conversion before
origin parsing match the additional Chrome controls.

Window delivery serializes before matching target origins, so even a mismatched
origin detaches valid transferable buffers. MessageEvent.ports uses the same native
port objects as payload references, with a frozen receiver-owned sequence. Native
port events use the canonical private EventTarget dispatcher for listener options,
handler registration order, cleanup and isTrusted. Guest replacement of dispatch,
timers, array iteration or indexed setters does not expose private port state.

Resource limits are explicit: 4,096 transfer entries, 131,072 endpoint records,
65,536 pending messages per endpoint, 64 MiB serialized graph bytes and 128 MiB
queued graph/backing-store/native-key material bytes per isolate. Budget failures report an error;
these host limits are not Chrome's implementation limits. Queue count capacity is reserved before payload getters execute. Serialized graph,
backing-store and native-key material admission completes before transfer commit,
so ordinary admission failures preserve source ownership. Nested reservations
count against the same limits and roll back on failure. Getter-driven closure or
transfer of an endpoint is rechecked before enqueueing. Completed native
transfer detachments cannot be rolled back if execution is terminated midway
through the final V8 detach sequence.

Endpoint wrappers and native anchors are weakly rooted. Private V8 edges retain
entangled peers only while the receiving peer has message/messageerror listeners,
and queued transfer anchors follow their owning receiver. Private native listener
observers update these edges after add/remove/once/abort. A pending delivery task
pins its receiver; removing the queued edge restores in-flight transfer pins until
receiver reconstruction. No page-visible registry or collector is installed.

The normal Chrome 155.0.8059.79 / V8 15.5.35.23 control
(`message-port-lifetime-chrome.mjs`) uses host CDP GC across distinct jobs. Its
seven retained/released rows match the offline engine: unreachable open,
listening, closed and queued-transfer cycles collect; reachable receivers retain
queued transfer graphs and entangled listening peers. Native sweeps reclaim dead
endpoint records and queued byte accounting; runtime destruction clears remaining
native records before V8 disposal. Closed live wrappers remain transferable, and
stale detached genuine wrappers stay inert after record reclamation.

Queued CryptoKeys use Rust-only material, typed metadata and permission snapshots.
The key lifetime control proves collection despite source expandos, custom
prototypes, forged metadata getters and an X-Wing private-key expando. A retained
receiver still gets usable native key material after original-wrapper collection.
Native material admission rejects before buffer detachment. The exact provider
source/archive/license and reproducible patch are documented in DENO_CRYPTO.md.

The current integrated follow-up passes 24 transfer and six frame lifetime
contracts. Removed Window targets still serialize and detach valid transfers before
discarding delivery. Retirement prunes native context registry roots, pending target
Window messages and timer payloads, while parent-addressed messages remain deliverable
and retained old Documents keep their native DOM methods. Failed DOM conversion
retires a frame only when native parent/document/connection state actually changed;
unchanged preflight and WebIDL failures preserve its live browsing context.

Full production/default/genuine155 gates and actual realm/node reclamation remain
open. Canonical native wrapper/cache/context and callback roots still need traced
ownership; these controls do not establish complete browser garbage collection.

Payload getters can change port lifetime before clone commit. Recheck the original
sender generation, closed state and reciprocal entanglement before delivery, while
still committing valid outer transfers. Chrome155.0.8059.79 discards sends when the
getter closes either endpoint and delivers to a transferred receiving endpoint.
Reentrant structuredClone of the sending endpoint hangs that Chrome renderer; the
versioned timeout fixture records its last authored console checkpoint. The engine
requires a bounded discarded send separately from successful Chrome parity.
