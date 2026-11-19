import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("captured requests retain authored headers and POST bytes independently of response metadata", async () => {
  let observed;
  let observedBinary;
  let binaryRequest;
  const server = createServer(async (request, response) => {
    if (request.url === "/api" || request.url === "/binary") {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      if (request.url === "/api")
        observed = { header: request.headers["x-probe"], body: Buffer.concat(chunks).toString() };
      else observedBinary = Buffer.concat(chunks);
      response.setHeader("content-type", "text/plain");
      response.setHeader("x-response-only", "response");
      response.end("ok");
    } else {
      response.end(
        '<body><script>fetch("/api",{method:"POST",headers:{"x-probe":"bench","content-type":"application/json"},body:JSON.stringify({a:1})}).then(r=>r.text());fetch("/binary",{method:"POST",body:new Uint8Array([0,255,128,10])}).then(r=>r.text())</script></body>',
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  const requests = [];
  page.on("request", (request) => {
    if (request.url().endsWith("/api")) requests.push(request);
    if (request.url().endsWith("/binary")) binaryRequest = request;
  });
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
    assert.deepEqual(observed, { header: "bench", body: '{"a":1}' });
    assert.equal(requests.length, 1);
    const request = requests[0];
    assert.equal(request.method(), "POST");
    assert.equal(request.headers()["x-probe"], "bench");
    assert.equal(request.headers()["content-type"], "application/json");
    assert.equal(request.headers()["x-response-only"], undefined);
    assert.equal(request.postData(), '{"a":1}');
    assert.deepEqual(request.postDataBuffer(), Buffer.from('{"a":1}'));
    assert.deepEqual(request.postDataJSON(), { a: 1 });
    assert.deepEqual(observedBinary, Buffer.from([0, 255, 128, 10]));
    assert.deepEqual(binaryRequest.postDataBuffer(), Buffer.from([0, 255, 128, 10]));
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
