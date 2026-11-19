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

test("concurrent URL observers resolve a host-callback navigation without waiting for unrelated response bodies", async () => {
  let releaseBody;
  let bodyReleased = false;
  let receivedSlow;
  const slowReceived = new Promise((resolve) => {
    receivedSlow = resolve;
  });
  const server = createServer((request, response) => {
    if (request.url === "/held") {
      response.writeHead(200, { "content-type": "text/plain" });
      response.flushHeaders();
      releaseBody = () => {
        bodyReleased = true;
        response.end("sibling-complete");
      };
      receivedSlow();
    } else response.end("<body>login</body>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  let safety;
  let picker;
  let entity;
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.exposeFunction("grantLogin", () => true);
    await page._ensureLive();
    await page._evalEngine(
      `grantLogin().then(()=>{history.pushState({},'', '#entity');fetch('/held').then(r=>r.text()).then(value=>globalThis.siblingResult=value);});globalThis.__RESULT='';`,
    );
    picker = page.waitForURL("**#picker", { timeout: 2000 }).then(() => "picker");
    entity = page
      .waitForURL((url) => url.hash === "#entity", { timeout: 2000 })
      .then(() => "entity");
    picker.catch(() => {});
    entity.catch(() => {});
    // This is a deadlock escape, not the assertion's completion condition.
    safety = setTimeout(() => releaseBody?.(), 5000);
    const result = await Promise.race([picker, entity]);
    assert.equal(result, "entity");
    await slowReceived;
    assert.equal(bodyReleased, false, "URL observation must not wait for a sibling response body");
    await assert.rejects(picker, { name: "TimeoutError" });
    assert.equal(
      bodyReleased,
      false,
      "a losing observer must not complete or abort sibling requests",
    );
    releaseBody();
    await page._redrain();
    assert.equal(await page.evaluate(() => globalThis.siblingResult), "sibling-complete");
  } finally {
    clearTimeout(safety);
    releaseBody?.();
    await Promise.allSettled([picker, entity]);
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});
