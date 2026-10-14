# Native CryptoKey cloning

The reusable `crypto-clone-probe.js` runs against an authored localhost page in
normal Chrome 155 and the offline native renderer. The public control is saved
in `fixtures/crypto-clone-chrome155.json` (`Chrome/155.0.8059.40`, V8
`15.5.35.20`). It records behavior and public metadata; it contains no user keys
or captured Google scripts.

The observed Window receiver selects the clone's realm: a parent
`structuredClone` borrowed onto a child Window creates child objects and keys.
A null receiver selects the function's own Window. An ordinary object receiver
throws `TypeError`. Genuine keys ignore page properties and registry-symbol
hooks during cloning. Genuine Crypto/SubtleCrypto, Events, DOM nodes, functions,
and proxies reject with `DataCloneError`; the same holds for genuine EventTarget,
AbortController/AbortSignal (including subclasses and foreign realms), XPath objects,
DOMTokenList, NodeList, URL, and URLSearchParams. A plain object with a forged interface
prototype clones as a plain object. A throwing ordinary getter preserves the
original thrown exception.

`crypto_clone.rs` delegates the object graph to V8, preserving cycles, repeated
references, Map/Set key identity, Date, and typed buffers. The serialized bytes
contain indices for branded CryptoKeys; their native handles are retained
privately in the isolate. Deserialization copies Rust key material into a new
native key and applies the receiving realm's private public-interface adapter.
Native type, algorithm, extractability, and usage permissions survive cloning.
Hybrid X-Wing keys use the same native handle boundary. No JavaScript material
snapshots or public clone/factory symbols are installed.

DOMException cloning snapshots its private name/message and reconstructs the
receiving realm's genuine exception, preserving aliases and the legacy code.
Own page getters and replaced global constructors cannot change that snapshot.
User Timing detail uses the same private graph bridge for keys and exceptions.

The official provider's inherited-setter construction bug is fixed by the
licensed reproducible patch described in
[`rust/vendor/DENO_CRYPTO.md`](../../rust/vendor/DENO_CRYPTO.md). The adversarial
tests cover inherited material setters, array-index setters, forged key
properties, and a poisoned `Deno.core.hostObject` symbol. Native crypto tests
exercise non-extractable AES encryption, Ed25519 signatures, and ML-KEM/X-Wing
roundtrips after cloning.

Queued Window messages retain the snapshot until delivery and then deserialize
in the destination context. MessagePort cloning uses its peer's context.
Focused tests cover same-origin child delivery, aliases/cycles, source/origin,
target-origin suppression, foreign-port key realms, and unchanged sender keys.
These tests do not establish complete MessagePort/EventTarget behavior or every
navigation and target-origin policy.

`structuredClone` supports genuine ArrayBuffer transfers: the receiving realm
gets the backing store, while the source buffer and all views detach. Duplicate,
invalid, detached and non-detachable entries fail before transfer. Serialization
errors preserve attached sources; lists are revalidated after ordinary getters
run. The control covers unused transfers, view aliases, resizable buffers and
WebIDL dictionary/iterator conversion. MessagePort and Window transfer lists
remain explicitly unsupported. Internal shared-buffer support does not establish
Chrome's cross-origin isolation or SharedArrayBuffer exposure policy. Publishing
the renderer for clean Cargo consumers requires a distributable hardened crypto
provider; the workspace patch alone is insufficient.


## Queued key lifetime follow-up

Queued CryptoKeys now use Rust-owned native material, typed algorithm metadata and
permissions. They retain no sender wrapper, expando, prototype or JavaScript
metadata graph. Four independent localhost Chrome 155.0.8059.79 / V8 15.5.35.23
controls cover source expandos, custom prototypes, forged metadata getters and
X-Wing private keys. The native follow-up passes those controls, and a retained
receiver can still encrypt with a cloned non-extractable AES key after the source
wrapper is collected; export/decrypt permissions remain denied. RSA-OAEP, ECDSA
and HMAC operation/metadata controls also pass. Queued native material counts
against admission before transferable ownership commits.

The isolated follow-up passes 62 contracts and strict Clippy/formatting. Combined
default and genuine V8 155 production gates are still pending. These tests do not
establish complete retired-frame or browser garbage-collection parity.
