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

test("context page waits observe actual page ownership and honor once/off", async () => {
  const context = new shim.BrowserContext();
  const seen = [];
  const removed = () => seen.push("removed");
  try {
    context.on("page", removed).off("page", removed);
    context.once("page", function (page) {
      assert.equal(this, context);
      assert.ok(context.pages().includes(page));
      seen.push(page);
    });
    const next = context.waitForEvent("page", { timeout: 2000 });
    const first = await context.newPage();
    assert.equal(await next, first);
    await context.newPage();
    assert.deepEqual(seen, [first]);
  } finally {
    await context.close();
  }
});

test("context response waits preserve real page identity across existing and future pages", async () => {
  const server = createServer((request, response) => {
    if (request.url.startsWith("/signal/")) response.end("real response");
    else response.end(`<script>fetch('/signal${request.url}')</script>`);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const context = new shim.BrowserContext();
  try {
    const first = await context.newPage();
    const observed = [];
    const listener = (response) => observed.push(response);
    context.on("response", listener);
    const response = context.waitForEvent("response", {
      predicate: async (value) => value.url().endsWith("/signal/second"),
      timeout: 5000,
    });
    await first.goto(`${origin}/first`, { waitUntil: "networkidle" });
    const second = await context.newPage();
    await second.goto(`${origin}/second`, { waitUntil: "networkidle" });
    const actual = await response;
    assert.equal(actual.status(), 200);
    assert.equal(actual.frame().page(), second);
    assert.equal(actual.request().isNavigationRequest(), false);
    assert.equal(
      observed
        .find((value) => value.url().endsWith("/signal/first"))
        .frame()
        .page(),
      first,
    );
    context.off("response", listener);
    assert.equal(context._listeners.get("response")?.length ?? 0, 0);
    assert.equal(first._listeners.get("response")?.length ?? 0, 0);
    assert.equal(second._listeners.get("response")?.length ?? 0, 0);
  } finally {
    await context.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("context waits reject timeouts and predicates without retaining listeners", async () => {
  const context = new shim.BrowserContext();
  try {
    await assert.rejects(context.waitForEvent("page", { timeout: 25 }), { name: "TimeoutError" });
    assert.equal(context._listeners.get("page")?.length ?? 0, 0);
    const rejected = context.waitForEvent("page", {
      predicate: () => {
        throw new Error("fixture predicate rejected");
      },
      timeout: 2000,
    });
    rejected.catch(() => {});
    await context.newPage();
    await assert.rejects(rejected, /fixture predicate rejected/);
    assert.equal(context._listeners.get("page")?.length ?? 0, 0);
  } finally {
    await context.close();
  }
});

test("context close resolves its real event and rejects unlimited unrelated waits", async () => {
  const context = new shim.BrowserContext();
  const page = await context.newPage();
  const closed = context.waitForEvent("close", { timeout: 0 });
  const response = context.waitForEvent("response", { timeout: 0 });
  response.catch(() => {});
  await context.close();
  assert.equal(await closed, context);
  assert.equal(page.isClosed(), true);
  await assert.rejects(response, /closed/i);
  await assert.rejects(context.waitForEvent("page"), /closed/i);
});
