import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { createRequire } from "node:module";
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("console events expose type, source and retained live argument handles", async () => {
  const server = createServer((request, response) => {
    response.setHeader(
      "content-type",
      request.url === "/fixture.js" ? "application/javascript" : "text/html",
    );
    response.end(
      request.url === "/fixture.js"
        ? "globalThis.logged={code:17};console.warn('diagnostic-marker',logged);logged.code=18;"
        : "<script src='/fixture.js'></script><button id=done>done</button>",
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage(),
    messages = [];
  page.on("console", (message) => messages.push(message));
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}/`, { waitUntil: "networkidle" });
    assert.equal(messages.length, 1);
    const message = messages[0];
    assert.equal(message.type(), "warning");
    assert.match(message.text(), /diagnostic-marker/);
    assert.match(message.text(), /code: 17/);
    assert.match(message.location().url, /fixture\.js$/);
    assert.equal(message.location().lineNumber, 0);
    assert.ok(message.location().columnNumber >= 0);
    const args = message.args();
    assert.equal(await args[0].jsonValue(), "diagnostic-marker");
    assert.deepEqual(await args[1].jsonValue(), { code: 18 });
    await page.evaluate(() => {
      logged.code = 19;
    });
    assert.equal(await args[1].evaluate((value) => value.code), 19);
    assert.equal(message.page(), page);
    await args[0].dispose();
    await assert.rejects(args[0].jsonValue(), /disposed/);
    await page.close();
    await assert.rejects(args[1].jsonValue(), /closed|context/);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("child console arguments own the child realm and preserve message ordering", async () => {
  const page = shim.newPage(),
    messages = [];
  page.on("console", (message) => messages.push(message));
  try {
    await page.setContent(
      "<iframe srcdoc=\"<script>console.info('child', {realm: 'child'});</script>\"></iframe>",
    );
    await page.evaluate(() => console.error("parent"));
    assert.deepEqual(
      messages.map((message) => message.type()),
      ["info", "error"],
    );
    assert.equal(await messages[0].args()[1].evaluate(() => document === window.document), true);
    assert.equal(await messages[0].args()[1].evaluate((value) => value.realm), "child");
    await page.evaluate(() => document.querySelector("iframe").remove());
    await assert.rejects(messages[0].args()[1].jsonValue(), /detached|context|disposed/);
  } finally {
    await page.close();
  }
});

test("draining console messages without listeners releases argument roots", async () => {
  const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  const page = shim.newPage();
  page.on("request", () => {});
  try {
    await page.setContent("<body>ready</body>");
    let pulls = 0;
    const refresh = page._refreshFromSession.bind(page);
    page._refreshFromSession = (...args) => {
      pulls++;
      return refresh(...args);
    };
    await page.evaluate(() => console.log({ unobserved: true }));
    assert.equal(pulls, 0, "console delivery must not require a full DOM synchronization");
    assert.equal(
      await native.liveEvalEngineExpression(page._session, "__tsNs.remoteHandleCount()"),
      "0",
    );
  } finally {
    await page.close();
  }
});
