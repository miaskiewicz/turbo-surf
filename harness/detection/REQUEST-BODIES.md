# Fetch request and response body controls

`request-body-probe.js` records twelve independent controls from fresh normal
Chrome155.0.8059.79 / V8 15.5.35.23. `request-body-details-probe.js` adds sixteen
adversarial controls. The saved JSON records the exact probe, collector hashes, Chrome launch flags, page errors and localhost requests.
The collector launches ordinary Chrome with a fresh profile and no environment
or fingerprint overrides. The runtime test does not launch a browser.

```sh
node harness/detection/authored-request-body-chrome.mjs harness/detection/request-body-probe.js harness/detection/fixtures/request-body-chrome155.json
node harness/detection/authored-request-body-chrome.mjs harness/detection/request-body-details-probe.js harness/detection/fixtures/request-body-details-chrome155.json
node --test harness/detection/request-body-fixtures.test.mjs
cd rust
cargo test -p turbo-surf-render --test request_body
```

The Rust test compares the complete result objects, verifies rejected cleartext
HTTP/1 stream requests never reach the server and checks that a cloned ordinary
text body still uploads its real bytes after `.body` was accessed. Fixtures read
complete bounded HTTP headers and Content-Length bodies; split TCP packets do
not change assertions. The original baseline differs on ten of twelve controls;
its failure log is retained in the development handoff. An additional native
failure demonstrated silent conversion of invalid readable chunk types before
all four values were changed to match Chrome's TypeError behavior.

Request/Response BodyInit conversion recognizes actual native-private interface
records. A duck-typed `arrayBuffer` or `getReader` method and a fake `type` property
do not turn a plain object into a Blob or stream. Genuine Blob parts and body
buffers are copied, URLSearchParams serializes at construction, and genuine stream
chunks must be Uint8Array values. Creating a Request from another Request moves
its body through an official stream transform, locking and disturbing the source;
clone uses an independent tee. No native host or realm root is added.

The synchronous CSP guard and native initiating policy snapshot remain before
asynchronous upload work. Transport uses the captured private Request URL.
The HTTP/1 rejection fixes the measured cleartext protocol case. HTTPS/HTTP2
transport still buffers readable uploads and transferable readable/writable/
transform streams remain unsupported. Those are separate implementation tasks;
passing these controls does not establish complete Fetch, browser or Google SERP
parity. Multipart FormData extraction and broader WebIDL getter-order behavior
also need independent controls and implementation.
