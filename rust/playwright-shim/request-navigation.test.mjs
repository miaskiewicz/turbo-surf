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

test("navigation retains its actual response across a real chunk response burst", async () => {
  const server = createServer((request, response) => {
    response.setHeader("cache-control", "no-store");
    if (request.url.startsWith("/chunk-")) {
      response.setHeader("content-type", "application/javascript");
      response.end("globalThis.chunksLoaded = (globalThis.chunksLoaded || 0) + 1;");
    } else {
      response.setHeader("content-type", "text/html");
      response.end(
        request.url === "/start"
          ? "<body>start</body>"
          : "<body>chunked" +
              Array.from({ length: 140 }, (_, i) => `<script src='/chunk-${i}.js'></script>`).join(
                "",
              ) +
              "</body>",
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  let owned;
  let chunks = 0;
  page.on("response", (response) => {
    if (response.url() === origin + "/chunked") owned = response;
    if (response.url().includes("/chunk-")) chunks++;
  });
  try {
    await page.goto(origin + "/start", { waitUntil: "load" });
    const response = await page.goto(origin + "/chunked", { waitUntil: "load", timeout: 10000 });
    assert.equal(chunks, 140);
    assert.equal(await page.evaluate(() => chunksLoaded), 140);
    assert.equal(response, owned);
    assert.equal(await response.request().response(), response);
    assert.equal(response.status(), 200);
    assert.equal(response.request().isNavigationRequest(), true);
    assert.equal(page._recentResponses.length, 100);
    assert.equal(page._recentResponses.includes(response), false);
    assert.equal(page._navigationResponseOwners.size, 0);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

async function actorChunkFixture(run) {
  let held;
  let signalHeld;
  const heldStarted = new Promise((resolve) => {
    signalHeld = resolve;
  });
  const server = createServer((request, response) => {
    const url = new URL(request.url, "http://fixture.test");
    response.setHeader("cache-control", "no-store");
    if (url.pathname === "/redirect") {
      response.writeHead(302, { location: "/chunked" + url.search });
      response.end();
      return;
    }
    if (url.pathname === "/failure") {
      request.socket.destroy();
      return;
    }
    if (url.pathname === "/held.js") {
      response.setHeader("content-type", "application/javascript");
      response.write("/* held script */");
      held = response;
      signalHeld();
      return;
    }
    if (url.pathname.startsWith("/chunk-")) {
      response.setHeader("content-type", "application/javascript");
      response.end("globalThis.chunksLoaded = (globalThis.chunksLoaded || 0) + 1;");
      return;
    }
    response.setHeader("content-type", "text/html");
    if (url.pathname === "/start")
      response.end("<body>start<script>globalThis.startReady=true</script></body>");
    else {
      response.statusCode = Number(url.searchParams.get("status") || 200);
      response.end(
        "<body>" +
          url.search +
          Array.from(
            { length: 140 },
            (_, i) => `<script src='/chunk-${i}.js${url.search}'></script>`,
          ).join("") +
          (url.pathname === "/held" ? '<script src="/held.js"></script>' : "") +
          "</body>",
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const context = new shim.BrowserContext();
  try {
    await run({ context, origin, heldStarted });
  } finally {
    held?.end("/* released */");
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("redirected chunk navigations retain final wire status and cross-document identity", async () => {
  await actorChunkFixture(async ({ context, origin }) => {
    const page = await context.newPage();
    const emitted = [];
    page.on("response", (response) => emitted.push(response));
    await page.goto(origin + "/start", { waitUntil: "load" });
    const first = await page.goto(origin + "/chunked?owner=first", {
      waitUntil: "load",
      timeout: 10000,
    });
    const second = await page.goto(origin + "/redirect?owner=second&status=201", {
      waitUntil: "load",
      timeout: 10000,
    });
    assert.notEqual(first, second);
    assert.equal(
      second,
      emitted.find((response) => response.url() === origin + "/chunked?owner=second&status=201"),
    );
    assert.equal(second.status(), 201);
    assert.match(await second.text(), /owner=second/);
    assert.equal(
      second.request().redirectedFrom().url(),
      origin + "/redirect?owner=second&status=201",
    );
    assert.equal(second.request().frame(), page.mainFrame());
    assert.equal(page._recentResponses.length, 100);
    assert.equal(page._recentResponses.includes(second), false);
    assert.equal(page._navigationResponseOwners.size, 0);
  });
});

test("parallel Pages retain only their own chunk-navigation responses", async () => {
  await actorChunkFixture(async ({ context, origin }) => {
    const pages = await Promise.all([context.newPage(), context.newPage()]);
    const emitted = pages.map(() => []);
    for (let i = 0; i < pages.length; i++)
      pages[i].on("response", (response) => emitted[i].push(response));
    await Promise.all(pages.map((page) => page.goto(origin + "/start", { waitUntil: "load" })));
    const responses = await Promise.all(
      pages.map((page, i) =>
        page.goto(origin + `/chunked?owner=${i}`, { waitUntil: "load", timeout: 10000 }),
      ),
    );
    for (let i = 0; i < pages.length; i++) {
      assert.equal(responses[i].url(), origin + `/chunked?owner=${i}`);
      assert.equal(
        responses[i],
        emitted[i].find((response) => response.url() === responses[i].url()),
      );
      assert.equal(emitted[1 - i].includes(responses[i]), false);
      assert.equal(pages[i]._recentResponses.length, 100);
      assert.equal(pages[i]._navigationResponseOwners.size, 0);
    }
  });
});

test("closed chunk navigation releases its retained response collector", async () => {
  await actorChunkFixture(async ({ context, origin, heldStarted }) => {
    const page = await context.newPage();
    await page.goto(origin + "/start", { waitUntil: "load" });
    await page.evaluate(() => true);
    const navigation = page.goto(origin + "/held", { waitUntil: "load", timeout: 2000 });
    navigation.catch(() => {});
    await heldStarted;
    assert.equal(page._navigationResponseOwners.size, 1);
    const owner = [...page._navigationResponseOwners][0];
    await page.close();
    await assert.rejects(
      navigation,
      (error) => !error.message.includes("without an owned navigation response"),
    );
    assert.equal(page._navigationResponseOwners.size, 0);
    assert.equal(owner.responses.size, 0);
  });
});

test("failed navigation preserves its genuine error and releases response ownership", async () => {
  await actorChunkFixture(async ({ context, origin }) => {
    const page = await context.newPage();
    await page.goto(origin + "/start", { waitUntil: "load" });
    await page.evaluate(() => true);
    await assert.rejects(
      page.goto(origin + "/failure", { timeout: 1000 }),
      (error) => !error.message.includes("without an owned navigation response"),
    );
    assert.equal(page._navigationResponseOwners.size, 0);
  });
});

test("request events retain redirect navigation and the actual originating frame", async () => {
  const server = createServer((request, response) => {
    if (request.url === "/redirect") {
      response.writeHead(302, { location: "/landing" });
      response.end();
      return;
    }
    if (request.url === "/landing")
      response.end(
        '<body><iframe style="width:210px;height:120px" src="/child"></iframe><iframe style="width:230px;height:130px" src="/child"></iframe><script>fetch("/main-api").then(r=>r.text())</script></body>',
      );
    else if (request.url === "/child")
      response.end(
        "<title>child</title><script>window.initialViewport=[innerWidth,innerHeight]</script>",
      );
    else response.end("ok");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  const requests = [];
  page.on("request", (request) => requests.push(request));
  try {
    const navigationResponse = await page.goto(origin + "/redirect", { waitUntil: "networkidle" });
    const navigations = requests.filter((request) =>
      [origin + "/redirect", origin + "/landing"].includes(request.url()),
    );
    assert.deepEqual(
      navigations.map((request) => request.url()),
      [origin + "/redirect", origin + "/landing"],
    );
    for (const request of navigations) {
      assert.equal(request.isNavigationRequest(), true);
      assert.equal(request.frame(), page.mainFrame());
      assert.equal(request.resourceType(), "document");
      assert.equal(request.method(), "GET");
      assert.equal(request.postData(), null);
    }
    assert.equal(await navigations[1].response(), navigationResponse);
    assert.equal(navigationResponse.request(), navigations[1]);
    const main = requests.find((request) => request.url() === origin + "/main-api");
    assert.equal(main.isNavigationRequest(), false);
    assert.equal(main.frame(), page.mainFrame());
    const frames = page.childFrames();
    assert.equal(frames.length, 2);
    for (let index = 0; index < frames.length; index++) {
      assert.deepEqual(
        await frames[index].evaluate(() => window.initialViewport),
        index === 0 ? [210, 120] : [230, 130],
      );
      await frames[index].evaluate(
        async (url) => {
          await (await fetch(url)).text();
        },
        origin + "/child-api?index=" + index,
      );
      const request = requests.find(
        (candidate) => candidate.url() === origin + "/child-api?index=" + index,
      );
      assert.equal(request.isNavigationRequest(), false);
      assert.equal(request.frame(), frames[index]);
    }
    const childNavigations = requests.filter((request) => request.url() === origin + "/child");
    assert.equal(childNavigations.length, 2);
    assert.ok(
      frames.every((frame) => childNavigations.some((request) => request.frame() === frame)),
    );
    assert.ok(childNavigations.every((request) => request.isNavigationRequest()));
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("navigation redirect request records preserve native method and body rewriting", async () => {
  const wire = [];
  const server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    wire.push({ url: request.url, method: request.method, body: Buffer.concat(chunks).toString() });
    if (request.url.startsWith("/redirect")) {
      const status = Number(request.url.slice("/redirect".length));
      response.writeHead(status, { location: "/final" + status });
      response.end();
    } else response.end("<title>done</title>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  const requests = [];
  page.on("request", (request) => requests.push(request));
  try {
    for (const status of [303, 307])
      await page._navigate(origin + "/redirect" + status, "POST", "key=value");
    assert.deepEqual(
      requests.map((request) => ({
        url: new URL(request.url()).pathname,
        method: request.method(),
        body: request.postData(),
      })),
      [
        { url: "/redirect303", method: "POST", body: "key=value" },
        { url: "/final303", method: "GET", body: null },
        { url: "/redirect307", method: "POST", body: "key=value" },
        { url: "/final307", method: "POST", body: "key=value" },
      ],
    );
    assert.deepEqual(
      wire,
      requests.map((request) => ({
        url: new URL(request.url()).pathname,
        method: request.method(),
        body: request.postData() ?? "",
      })),
    );
    assert.ok(
      requests.every(
        (request) => request.isNavigationRequest() && request.frame() === page.mainFrame(),
      ),
    );
  } finally {
    await page.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
