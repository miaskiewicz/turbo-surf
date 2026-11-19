import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { createHash } from "node:crypto";
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
test("fetch redirect responses retain real hop status, identity and final body", async () => {
  const wire = [];
  const server = createServer(async (q, s) => {
    const chunks = [];
    for await (const chunk of q) chunks.push(chunk);
    wire.push([q.url, q.method, q.headers.cookie, Buffer.concat(chunks).toString()]);
    if (q.url.startsWith("/redirect")) {
      s.writeHead(Number(q.url.split("/")[2]), {
        location: "/data",
        "set-cookie": "hop=yes; Path=/",
        "x-hop": "redirect",
      });
      s.end("redirect");
    } else if (q.url === "/data") {
      s.setHeader("content-type", "application/json");
      s.end(JSON.stringify({ ok: true, method: q.method, cookie: q.headers.cookie ?? "" }));
    } else s.end("<body>test</body>");
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  try {
    await page.goto(origin);
    const responses = [];
    page.on("response", (r) => responses.push(r));
    for (const [status, buffered] of [302, 303, 307].flatMap((status) => [
      [status, false],
      [status, true],
    ])) {
      const expectedBody = JSON.stringify({
        ok: true,
        method: status === 307 ? "POST" : "GET",
        cookie: "hop=yes",
      });
      const integrity = buffered
        ? `sha256-${createHash("sha256").update(expectedBody).digest("base64")}`
        : "";
      responses.length = 0;
      await page.evaluate(
        async ({ status, integrity }) => {
          const r = await fetch(`/redirect/${status}`, {
            method: "POST",
            body: "payload",
            integrity,
          });
          window.result = await r.json();
        },
        { status, integrity },
      );
      assert.deepEqual(
        responses.map((r) => [new URL(r.url()).pathname, r.status()]),
        [
          [`/redirect/${status}`, status],
          ["/data", 200],
        ],
      );
      for (const response of responses) assert.equal(await response.request().response(), response);
      assert.equal(responses[0].request().redirectedTo(), responses[1].request());
      assert.equal(responses[1].request().redirectedFrom(), responses[0].request());
      assert.equal(await responses[0].headerValue("x-hop"), "redirect");
      assert.equal(
        responses[0].statusText(),
        status === 302 ? "Found" : status === 303 ? "See Other" : "Temporary Redirect",
      );
      await assert.rejects(responses[0].body(), /Redirect response body is unavailable/);
      const result = await responses[1].json();
      assert.equal(result.ok, true);
      assert.equal(result.method, status === 307 ? "POST" : "GET");
      assert.match(result.cookie, /hop=yes/);
      assert.deepEqual(await page.evaluate(() => window.result), result);
      assert.equal(responses[1].request().method(), result.method);
      assert.equal(responses[1].request().postData(), status === 307 ? "payload" : null);
      assert.equal(wire.at(-1)[1], result.method);
      assert.equal(wire.at(-1)[3], status === 307 ? "payload" : "");
    }
  } finally {
    await page.close();
    await new Promise((r) => server.close(r));
  }
});
