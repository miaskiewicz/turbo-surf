# Compression dictionary parity

The saved Google Chrome trace actually fetches compression dictionaries, sends
Available-Dictionary and decodes a dcb response. The sanitized observations and
source trace hash are in
[compression-dictionary-saved-evidence.json](./compression-dictionary-saved-evidence.json).
This is a measured missing resource feature in the old native executable. It is
not an established explanation for Google's rejection.

## Native implementation

The Core dictionary store shares the native session's HTTP-cache lifetime, with
explicit partition and dictionary-origin matching. It accepts complete admitted
responses with valid Use-As-Dictionary structured fields, supported raw dictionaries
and fresh HTTP cache metadata. URLPattern matching excludes regular-expression
groups. Destination specificity, pattern length and fetch order determine the
selected dictionary. Native requests advertise dictionary encodings and the actual
SHA-256 hash only after selection. See [RFC 9842](https://www.rfc-editor.org/rfc/rfc9842.html).

The actual Brotli and Zstandard decoders validate the coding envelope and selected
dictionary hash. A shared codec engine serves buffered callers and incremental
Fetch bodies. Two bounded channels connect real async transport input, blocking
codec workers and decoded output. Headers are exposed before the coded body
completes; partial output does not wait for the final encoded chunk. Both channels
require explicit successful termination, so a producer or worker panic cannot turn
into a successful EOF. Final cache publication waits for actual transport EOF. Independent fixtures were produced by the official C encoders;
negative controls confirm that their compressed content requires the dictionary.
The checked producer versions, source hashes and payload hashes are in
[fixture provenance](./fixtures/compression-dictionary/provenance.json).
Decoded HTTP-cache representations retain public Content-Encoding metadata and a
private decoded marker, so cache hits and 304 reuse do not decode the body twice.
HEAD responses preserve representation headers but have no body to decode or
register. Their Content-Length metadata does not count against a body byte cap.
Registration occurs only after complete successful consumption; cancellation,
overflow and stale clear-generation work cannot publish a partial dictionary.

The renderer discovers document Link headers and actual HTML link elements,
including open and closed shadow trees. Native enumeration ignores foreign SVG
links and does not call page-owned DOM methods. Background requests use the
existing native Fetch path, CSP connect-src, CORS, session partition and realm
retirement. They omit credentials and referrer. They keep the host pump active
without delaying DOMContentLoaded or the document load event. A bounded private
resource task queue dispatches load/error with per-frame fairness and microtask
checkpoints; due Window timers run first, while future timers are not advanced
ahead of resource events. Retiring a realm clears its queued callbacks. In-flight URL
coalescing keeps independent header and DOM owners; removing or replacing the last
DOM consumer cancels the actual request and suppresses stale resource events.

## Reusable controls

Run from the repository root:

```sh
node harness/detection/authored-compression-dictionary-chrome.mjs
node --test harness/detection/compression-dictionary.test.mjs
cargo test --manifest-path rust/Cargo.toml -p turbo-surf-core compression_dictionary
cargo test --manifest-path rust/Cargo.toml -p turbo-surf-render --test compression_dictionary
```

The Chrome collector uses a fresh profile and localhost-only HTTPS servers. Only
the checked fixture certificate's SPKI is trusted. It records actual request
headers, completion/cancellation and document/link lifecycle events; its saved
control pins the executed collector and server. The production renderer test
executes those independent cases through real PageSession instances. Chrome retains
author-supplied Available-Dictionary in Headers/Request but rejects its Fetch before
I/O, even for a known hash; Dictionary-ID alone remains allowed. Native admission
preserves that distinction without modifying the public header objects. The offline
bootstrap tests exercise actual discovery, shared ownership, A-to-B-to-A changes,
retirement and hostile promise-constructor hooks.

For the managed V8 155 SDK, use the normal
[engine helper](./engine-155/README.md) with the same Cargo test selection. Preserve
actual engine/source provenance; changing the public profile is not a V8 upgrade.
All32 exact Chrome cases pass in actual155 and stock-engine renderer tests.
Integrated full native/default gates and live Google acceptance are recorded separately
in [FINDINGS.md](./FINDINGS.md), not inferred from these focused controls.

## Final live Google observation — 2026-10-16

[Sanitized live evidence](./compression-dictionary-live-evidence.json) records
real dictionary acquisition and negotiation in the final two-repeat capture.
Native baseline requests advertise Available-Dictionary and dcb/dcz on16 and12
requests; the instrumented passes do so on16 and17. Each native pass receives
one Use-As-Dictionary response. Chrome's baseline counts are87 and76, with two
Use-As-Dictionary responses each; the instrumented passes advertise on28 requests
each and receive one dictionary response. These are different page flows, not
identical request workloads.

Native counts use explicit application headers after dictionary negotiation and
before transport send. They include the dictionary-selected Accept-Encoding but
exclude other headers inserted by the transport. Chrome counts use CDP ExtraInfo.
No captured response header lists dcb/dcz in these eight passes. Native ordinary
transport decoding may strip Content-Encoding, so absence is not proof of an
uncompressed wire response. This live capture exercises acquisition/negotiation;
codec decoding remains demonstrated by the independent HTTPS controls, rather
than inferred from this run. Native Google SERP acceptance is still0/2.

## Bounds and open work

- Store limits: 8 MiB per dictionary, 32 MiB total and 64 entries. The codec path
  allows at most 32 MiB encoded and 64 MiB decoded data, further restricted by the
  caller's response limit. Two worker permits also bound encoded-body buffering;
  a canceled caller does not release a permit while its worker is still running.
  Input and output channels each hold at most two 16 KiB chunks. Dropping the body
  aborts the producer, closes channels and cancels codec reads/output.
- Brotli windows above 16 MiB reject before decoder allocation. The current
  Zstandard window cap is 16 MiB, sufficient for the supported dictionary size.
- Document discovery admits at most 16 header hints and 64 distinct dictionary
  URLs. Native composed traversal has explicit 100,000-node and 256-link limits.
  These are engine bounds, not a claim of identical Chrome resource budgets.
- Chrome's current control does not advertise a dictionary on the immediate
  request after ordinary GET body completion, then advertises it after 100 ms and
  500 ms. Native publication is synchronous at successful EOF. The stable HEAD
  rejection and delayed GET positives are checked; publication scheduling remains
  a measured difference. No artificial delay is used to conceal it.
- Incremental dcb/dcz bodies now have actual HTTPS header, partial-delivery,
  cancellation and cache-publication controls on both transports. Broader Fetch
  queue ordering and encoded HTTP Range behavior remain separate parity work.
- Coding stacks, stale dictionary eligibility, broader redirect/opaque-policy and
  dictionary-policy behavior need additional parity controls. Native
  dictionary state is session-owned; disk persistence is not implemented here.
- A passing localhost pair does not certify the full transport/GPU surface or
  successful native Google SERP. Residuals stay tracked in
  [PARITY-GAPS.md](./PARITY-GAPS.md) and [issue #28](https://github.com/miaskiewicz/turbo-surf/issues/28).
