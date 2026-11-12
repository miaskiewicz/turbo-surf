import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import test from "node:test";
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
for (const mode of ["consume", "unread", "event-only", "buffered", "broken", "close"])
  test(`response headers precede separately completed body: ${mode}`, async () => {
    let release;
    const gate = new Promise((r) => (release = r));
    let ended = false;
    const server = createServer(async (q, s) => {
      if (q.url === "/slow") {
        s.writeHead(200, { "content-type": "application/json", "x-real": "headers" });
        s.flushHeaders();
        await gate;
        ended = true;
        if (mode === "broken") s.destroy();
        else s.end('{"ok":true}');
      } else
        s.end(
          `<button id="go" onclick="fetch('/slow',${JSON.stringify(mode === "buffered" ? { integrity: `sha256-${createHash("sha256").update('{"ok":true}').digest("base64")}` } : {}).replaceAll('"', "&quot;")}).then(r=>${mode === "consume" || mode === "buffered" ? "r.json()" : "undefined"}).catch(()=>{})">go</button>`,
        );
    });
    await new Promise((r) => server.listen(0, "127.0.0.1", r));
    const page = shim.newPage();
    let action;
    try {
      await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
      const finished = [],
        failed = [],
        responses = [];
      page.on("requestfinished", (r) => finished.push(r));
      page.on("requestfailed", (r) => failed.push(r));
      page.on("response", (r) => responses.push(r));
      const waiting =
        mode === "event-only"
          ? new Promise((resolve, reject) => {
              const timer = setTimeout(
                () => reject(new Error("Response event did not arrive before the held body")),
                500,
              );
              const listener = (response) => {
                if (response.url().endsWith("/slow")) {
                  clearTimeout(timer);
                  page.off("response", listener);
                  resolve(response);
                }
              };
              page.on("response", listener);
            })
          : page.waitForResponse((r) => r.url().endsWith("/slow"), { timeout: 500 });
      action = page.locator("#go").click();
      const response = await waiting;
      assert.equal(ended, false, "headers must arrive while the server still holds the body");
      assert.equal(response.status(), 200);
      assert.equal(await response.headerValue("x-real"), "headers");
      assert.equal(await response.request().response(), response);
      assert.equal(finished.length, 0);
      let settled = false;
      const body = response.body();
      body.then(
        () => (settled = true),
        () => (settled = true),
      );
      const done = response.finished();
      done.catch(() => {});
      await new Promise((r) => setTimeout(r, 10));
      assert.equal(settled, false);
      if (mode === "close") {
        await page.close();
        await assert.rejects(body, /closed|cancel/i);
        await assert.rejects(done, /closed|cancel/i);
      } else {
        release();
        if (mode === "broken") {
          await assert.rejects(body, /body|stream|network|error|closed/i);
          await assert.rejects(done);
          assert.equal(finished.length, 0);
          assert.equal(failed.length, 1);
        } else {
          assert.deepEqual(await body, Buffer.from('{"ok":true}'));
          assert.deepEqual(await response.json(), { ok: true });
          assert.equal(await done, null);
          assert.equal(finished.length, 1);
          assert.equal(failed.length, 0);
        }
        await Promise.all(Array.from({ length: 5 }, () => page["_drainNetworkEvents"]()));
        assert.equal(responses.length, 1);
      }
    } finally {
      release();
      await action?.catch(() => {});
      await page.close();
      server.closeAllConnections();
      await new Promise((r) => server.close(r));
    }
  });

test("response headers are not published for a policy-denied transport", async () => {
  let requests = 0;
  const server = createServer((q, s) => {
    if (q.url === "/denied") {
      requests++;
      s.end("forbidden");
    } else {
      s.setHeader(
        "content-security-policy",
        "default-src 'self'; script-src 'unsafe-inline'; connect-src 'none'",
      );
      s.end("<body>policy</body>");
    }
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const page = shim.newPage();
  try {
    await page.goto(`http://127.0.0.1:${server.address().port}`, { waitUntil: "networkidle" });
    const responses = [];
    page.on("response", (r) => responses.push(r));
    assert.equal(
      await page.evaluate(() =>
        fetch("/denied").then(
          () => false,
          () => true,
        ),
      ),
      true,
    );
    await page["_drainNetworkEvents"]();
    assert.equal(requests, 0);
    assert.equal(responses.length, 0);
    assert.equal(page["_responseBodies"]?.size ?? 0, 0);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((r) => server.close(r));
  }
});

for (const mode of ["initial", "initial-close", "form", "form-console"])
  test(`headers remain observable during ${mode} hydration`, { timeout: 10000 }, async () => {
    let release;
    const gate = new Promise((r) => (release = r));
    let headersSent;
    const wireHeaders = new Promise((resolve) => (headersSent = resolve));
    let headerTimer;
    let ended = false;
    let destinations = 0;
    const server = createServer(async (q, s) => {
      if (q.url === "/slow") {
        s.writeHead(200, { "content-type": "application/json" });
        s.flushHeaders();
        headersSent();
        await gate;
        ended = true;
        s.end('{"ok":true}');
      } else if (q.url === "/start")
        s.end(
          `<form action="/destination"><button id="submit" onclick="console.log('before-form', {phase:1})">submit</button></form>`,
        );
      else {
        destinations++;
        s.end('<script>fetch("/slow").then(r=>r.json()).catch(()=>{})</script>');
      }
    });
    await new Promise((r) => server.listen(0, "127.0.0.1", r));
    const origin = `http://127.0.0.1:${server.address().port}`;
    const page = shim.newPage();
    let action;
    const messages = [];
    if (mode === "form-console") page.on("console", (message) => messages.push(message));
    try {
      if (mode.startsWith("form")) await page.goto(origin + "/start", { waitUntil: "networkidle" });
      const waiting = new Promise((resolve) => {
        const listener = (response) => {
          if (!response.url().endsWith("/slow")) return;
          page.off("response", listener);
          resolve(response);
        };
        page.on("response", listener);
      });
      action = mode.startsWith("form")
        ? page.locator("#submit").click()
        : page.goto(origin, { waitUntil: "networkidle" });
      // Cold isolate startup and form navigation precede the transport under test.
      // Start the delivery deadline only once the server has flushed its headers;
      // the body remains gated until the response has actually been observed.
      await wireHeaders;
      const response = await Promise.race([
        waiting,
        new Promise((_, reject) => {
          headerTimer = setTimeout(
            () => reject(new Error("Response headers were not delivered while the body was held")),
            500,
          );
        }),
      ]);
      clearTimeout(headerTimer);
      assert.equal(ended, false);
      assert.equal(response.status(), 200);
      assert.equal(response.request().frame(), page.mainFrame());
      const body = response.json();
      body.catch(() => {});
      if (mode === "initial-close") {
        await page.close();
        await assert.rejects(body, /closed|cancel/i);
      } else {
        release();
        assert.deepEqual(await body, { ok: true });
        await action;
        assert.equal(destinations, 1);
        if (mode === "form-console") {
          const message = messages.find((entry) => entry.text().startsWith("before-form"));
          assert.ok(message, "console record must survive the native runtime swap");
          assert.equal(message.location().url, origin + "/start");
          const old = message.args()[1];
          await assert.rejects(old.jsonValue(), /context|destroyed|disposed/i);
          const current = await page.evaluateHandle(() => ({ phase: "after" }));
          await old.dispose();
          assert.deepEqual(await current.jsonValue(), { phase: "after" });
          await current.dispose();
          await Promise.all(message.args().map((argument) => argument.dispose()));
        }
      }
    } finally {
      clearTimeout(headerTimer);
      release();
      await action?.catch(() => {});
      await page.close();
      server.closeAllConnections();
      await new Promise((r) => server.close(r));
    }
  });
