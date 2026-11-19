import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const requests = [];
  const held = new Map();
  const sockets = new Set();
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://fixture.test").pathname;
    requests.push({ path, method: request.method, headers: request.headers });
    response.setHeader("Cache-Control", path === "/cached" ? "max-age=600" : "no-store");
    response.setHeader("Content-Type", "text/html");
    response.setHeader("Connection", "close");
    if (path === "/held" || path === "/redirect") {
      held.set(path, response);
      return;
    }
    response.end(
      path === "/page"
        ? '<!doctype html><body>page<iframe src="/frame"></iframe></body>'
        : "<!doctype html><body>local fixture</body>",
    );
  });
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    await run({ origin, requests, held });
  } finally {
    for (const socket of sockets) socket.destroy();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function waitFor(predicate) {
  for (let i = 0; i < 100; i++) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  assert.fail("local fixture request did not start");
}

test("initial offline context exposes false onLine and permits local schemes", async () => {
  const context = new shim.BrowserContext({ offline: true });
  try {
    const page = await context.newPage();
    await page.goto("about:blank");
    assert.equal(await page.evaluate(() => navigator.onLine), false);
    assert.deepEqual(
      await page.evaluate(async () => {
        const data = await (await fetch("data:text/plain,local-data")).text();
        const url = URL.createObjectURL(new Blob(["local-blob"]));
        const blob = await (await fetch(url)).text();
        URL.revokeObjectURL(url);
        return { data, blob };
      }),
      { data: "local-data", blob: "local-blob" },
    );
    await context.setOffline(false);
    assert.equal(await page.evaluate(() => navigator.onLine), true);
  } finally {
    await context.close();
  }
});

test("offline transitions reach owned tabs and frames but leave API and other contexts online", async () =>
  fixture(async ({ origin, requests }) => {
    const context = new shim.BrowserContext();
    const other = new shim.BrowserContext();
    try {
      await context.addCookies([{ name: "owned", value: "cookie", url: origin }]);
      const pages = [await context.newPage(), await context.newPage()];
      for (const page of pages) {
        await page.goto(origin + "/page");
        for (const frame of page.frames())
          await frame.evaluate(() => {
            globalThis.offlineEvents = [];
            for (const type of ["online", "offline"])
              addEventListener(type, (event) =>
                offlineEvents.push({
                  type,
                  trusted: event.isTrusted,
                  bubbles: event.bubbles,
                  cancelable: event.cancelable,
                  online: navigator.onLine,
                }),
              );
          });
      }
      await context.setOffline(true);
      await context.setOffline(true);
      for (const page of pages)
        for (const frame of page.frames()) {
          assert.deepEqual(
            await frame.evaluate(() => ({ online: navigator.onLine, events: offlineEvents })),
            {
              online: false,
              events: [
                {
                  type: "offline",
                  trusted: true,
                  bubbles: false,
                  cancelable: false,
                  online: false,
                },
              ],
            },
          );
        }
      assert.equal((await context.request.get(origin + "/bound-api")).status(), 200);
      assert.match(
        requests.find((request) => request.path === "/bound-api").headers.cookie,
        /owned=cookie/,
      );
      const api = await shim.request.newContext();
      try {
        assert.equal((await api.get(origin + "/standalone-api")).status(), 200);
      } finally {
        await api.dispose();
      }
      const otherPage = await other.newPage();
      await otherPage.goto(origin + "/other-context");
      assert.equal(await otherPage.evaluate(() => navigator.onLine), true);
      await context.setOffline(false);
      await context.setOffline(false);
      for (const page of pages)
        for (const frame of page.frames()) {
          assert.deepEqual(
            await frame.evaluate(() =>
              offlineEvents.map((event) => [event.type, event.online, event.trusted]),
            ),
            [
              ["offline", false, true],
              ["online", true, true],
            ],
          );
        }
    } finally {
      await context.close();
      await other.close();
    }
  }));

test("offline permits fresh cache and route fulfilment but blocks new native sends without replay", async () =>
  fixture(async ({ origin, requests }) => {
    const context = new shim.BrowserContext();
    try {
      const page = await context.newPage();
      await page.goto(origin + "/page");
      await page.evaluate(() => fetch("/cached").then((response) => response.text()));
      await context.setOffline(true);
      assert.equal(
        await page.evaluate(() => fetch("/cached").then((response) => response.status)),
        200,
      );
      assert.equal(requests.filter((request) => request.path === "/cached").length, 1);
      assert.equal(
        await page.evaluate(() =>
          fetch("/fresh").then(
            () => "unexpected",
            () => "failed",
          ),
        ),
        "failed",
      );
      assert.equal(
        await page.evaluate(() =>
          fetch("/post", { method: "POST", body: "owned-companyB" }).then(
            () => "unexpected",
            () => "failed",
          ),
        ),
        "failed",
      );
      await assert.rejects(page.goto(origin + "/offline-navigation"));
      await page.route("**/fulfilled", (route) =>
        route.fulfill({ status: 200, body: "host fulfilled" }),
      );
      assert.equal(
        await page.evaluate(() => fetch("/fulfilled").then((response) => response.text())),
        "host fulfilled",
      );
      await page.route("**/continued", (route) => route.continue());
      assert.equal(
        await page.evaluate(() =>
          fetch("/continued").then(
            () => "unexpected",
            () => "failed",
          ),
        ),
        "failed",
      );
      await context.setOffline(false);
      assert.equal(
        await page.evaluate(() => fetch("/online").then((response) => response.status)),
        200,
      );
      for (const path of ["/fresh", "/post", "/offline-navigation", "/fulfilled", "/continued"])
        assert.equal(requests.filter((request) => request.path === path).length, 0, path);
    } finally {
      await context.close();
    }
  }));

test("already-started HTTP continues offline while a redirect cannot start its next hop", async () =>
  fixture(async ({ origin, held, requests }) => {
    const context = new shim.BrowserContext();
    try {
      const page = await context.newPage();
      await page.goto(origin + "/page");
      await page.evaluate(() => {
        globalThis.heldRequest = fetch("/held").then((value) => value.text());
      });
      await waitFor(async () => {
        await page.evaluate(() => 0);
        return held.has("/held");
      });
      await context.setOffline(true);
      held.get("/held").end("already-started");
      assert.equal(await page.evaluate(() => globalThis.heldRequest), "already-started");
      await context.setOffline(false);
      await page.evaluate(() => {
        globalThis.redirectedRequest = fetch("/redirect").then(
          () => "unexpected",
          () => "failed",
        );
      });
      await waitFor(async () => {
        await page.evaluate(() => 0);
        return held.has("/redirect");
      });
      await context.setOffline(true);
      held.get("/redirect").writeHead(302, { Location: "/redirect-target" });
      held.get("/redirect").end();
      assert.equal(await page.evaluate(() => globalThis.redirectedRequest), "failed");
      assert.equal(requests.filter((request) => request.path === "/redirect-target").length, 0);
    } finally {
      await context.close();
    }
  }));
