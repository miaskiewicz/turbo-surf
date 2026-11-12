import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let shim, measureEvaluationSync;
try {
  shim = await import("./index.mjs");
  ({ measureEvaluationSync } = await import("../../harness/hotpath/evaluate-sync-perf.mjs"));
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("read-only typed collectors perform one evaluation and zero synchronization pulls (#297)", async () => {
  const page = shim.newPage();
  try {
    await page.setContent("<title>read</title><h1>heading</h1>");
    await page._ensureLive();
    const result = await measureEvaluationSync(
      page,
      () => ({ title: document.title, heading: document.querySelector("h1").textContent }),
      5,
    );
    assert.deepEqual(result.value, { title: "read", heading: "heading" });
    assert.deepEqual(result.counts, {
      refresh: 0,
      bindings: 0,
      serialize: 0,
      cookies: 0,
      engine: 0,
      expression: 5,
    });
  } finally {
    await page.close();
  }
});

test("aliased DOM mutations, errors, cookies and child documents stay synchronized (#297)", async () => {
  const page = shim.newPage();
  try {
    await page.setContent('<h1>before</h1><iframe srcdoc="<p>child</p>"></iframe>');
    await page._ensureLive();
    await page.evaluate(() => {
      const alias = document.querySelector("h1");
      Reflect.set(alias, "textContent", "after");
    });
    assert.match(await page.content(), /after/);
    await assert.rejects(
      page.evaluate(() => {
        document.querySelector("h1").textContent = "error mutation";
        throw new Error("expected collector error");
      }),
      /expected collector error/,
    );
    assert.match(await page.content(), /error mutation/);
    await page.evaluate(() => {
      const schedule = setTimeout;
      schedule(() => {
        document.querySelector("h1").textContent = "timer mutation";
      }, 0);
    });
    assert.match(await page.content(), /timer mutation/);
    await page.evaluate(() =>
      requestAnimationFrame(() => {
        document.querySelector("h1").textContent = "frame mutation";
      }),
    );
    assert.match(await page.content(), /frame mutation/);
    await page.evaluate(() => {
      frames[0].document.querySelector("p").textContent = "changed child";
    });
    assert.match(page.frames()[1]._html, /changed child/);
  } finally {
    await page.close();
  }
});

test("cookie-only, network and same-URL history changes trigger synchronization (#297)", async () => {
  const server = createServer((req, res) => {
    res.setHeader("content-type", "text/html");
    res.end(req.url === "/data" ? "payload" : "<h1>same document</h1>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = shim.newPage();
  try {
    const url = `http://127.0.0.1:${server.address().port}/`;
    await page.goto(url, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      Reflect.set(document, "cookie", "only=cookie;path=/");
    });
    assert.match(page._cookies, /only/);
    const responses = [];
    page.on("response", (response) => responses.push(response.url()));
    assert.equal(await page.evaluate(async () => await (await fetch("/data")).text()), "payload");
    assert.ok(responses.includes(`${url}data`));
    const navigations = [];
    page.on("framenavigated", (frame) => navigations.push(frame.url()));
    await page.evaluate(() => history.pushState({ same: true }, "", location.href));
    assert.deepEqual(navigations, [url]);
    assert.equal(await page.goBack(), null);
    assert.equal(await page.evaluate(() => history.state), null);
    await page.exposeFunction("hostValue", (value) => value + 1);
    assert.equal(await page.evaluate(async () => await hostValue(41)), 42);
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
