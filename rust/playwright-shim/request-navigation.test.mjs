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
