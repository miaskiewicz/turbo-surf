import assert from "node:assert/strict";
import test from "node:test";
import { Page } from "./index.mjs";

test("native same-URL document replacement retires screenshot assets and scene", () => {
  let closed = 0;
  const page = Object.assign(Object.create(Page.prototype), {
    _closed: false,
    _session: "owned-page",
    _targetId: "target",
    _url: "http://fixture.test/page",
    _driverNavigation: true,
    _actorHub: { canReadTarget: () => false },
    _imageCache: { key: "same-url-and-html", images: { "/image.png": Buffer.from("old") } },
    _capturePlan: { refs: ["/image.png"] },
    _screenshotScene: { scene: { close: () => closed++ } },
    _emit() {},
  });
  page._retireNativeDocumentAssets();
  assert.deepEqual(
    {
      assetsRetired: page._imageCache === undefined,
      planRetired: page._capturePlan === undefined,
      sceneRetired: page._screenshotScene === undefined,
      closed,
    },
    { assetsRetired: true, planRetired: true, sceneRetired: true, closed: 1 },
  );
});

test("same-URL native document navigation paints fresh background bytes", async () => {
  const { createRequire } = await import("node:module");
  const { createServer } = await import("node:http");
  const { pngPixels } = await import("../../harness/hotpath/png-pixels.mjs");
  const { newPage } = await import("./index.mjs");
  const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  let bytes = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  let requests = 0;
  const server = createServer((request, response) => {
    response.setHeader("cache-control", "no-store");
    if (request.url === "/image.png") {
      requests++;
      response.setHeader("content-type", "image/png");
      response.end(bytes);
    } else {
      response.setHeader("content-type", "text/html");
      response.end(
        "<body style='margin:0'><div style='width:3px;height:2px;background-image:url(/image.png)'></div>",
      );
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}/`;
  const page = newPage({ viewport: { width: 3, height: 2 } });
  try {
    await page.goto(url, { waitUntil: "load" });
    assert.equal(await page.evaluate(() => document.body.tagName), "BODY");
    const first = await page.screenshot({ externalCss: false });
    assert.deepEqual(pngPixels(first).pixel(1, 1).slice(0, 3), [255, 0, 0]);
    assert.equal(requests, 1);
    bytes = native.screenshot("<body style='margin:0;background:blue'>", 3, 2);
    await page.goto(url, { waitUntil: "load" });
    const second = await page.screenshot({ externalCss: false });
    assert.deepEqual(pngPixels(second).pixel(1, 1).slice(0, 3), [0, 0, 255]);
    assert.equal(requests, 2);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("native screenshot reuses resident no-store pixels without exposing bytes to guest promises", async () => {
  const { createRequire } = await import("node:module");
  const { createServer } = await import("node:http");
  const { pngPixels } = await import("../../harness/hotpath/png-pixels.mjs");
  const { newPage } = await import("./index.mjs");
  const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  const red = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  const blue = native.screenshot("<body style='margin:0;background:blue'>", 3, 2);
  let requests = 0;
  const imageServer = createServer((_request, response) => {
    requests++;
    response.setHeader("cache-control", "no-store");
    response.setHeader("content-type", "image/png");
    response.end(requests === 1 ? red : blue);
  });
  await new Promise((resolve) => imageServer.listen(0, "127.0.0.1", resolve));
  const imageUrl = `http://127.0.0.1:${imageServer.address().port}/image.png`;
  const server = createServer((_request, response) => {
    response.setHeader("content-type", "text/html");
    response.end(
      `<body style='margin:0'><img src='${imageUrl}?fallback' srcset='${imageUrl} 1x' width='3' height='2'></body>`,
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}/`;
  const page = newPage({ viewport: { width: 3, height: 2 } });
  let peer;
  try {
    await page.addInitScript(() => {
      const then = Promise.prototype.then;
      globalThis.imageResultFields = [];
      globalThis.imageBytesSerialized = false;
      const stringify = JSON.stringify;
      const exposesBytes = (value, depth = 0) => {
        if (typeof value === "string") return value.startsWith("iVBORw0K");
        if (!value || typeof value !== "object" || depth >= 3) return false;
        return Object.values(value).some((item) => exposesBytes(item, depth + 1));
      };
      JSON.stringify = function (value, ...options) {
        if (exposesBytes(value)) globalThis.imageBytesSerialized = true;
        return stringify(value, ...options);
      };
      const observeResult = function (fulfilled, rejected) {
        return then.call(
          this,
          (value) => {
            if (
              typeof value?.url === "string" &&
              value.url.endsWith("/image.png") &&
              value.width === 3
            )
              globalThis.imageResultFields.push(Object.keys(value).sort());
            return typeof fulfilled === "function" ? fulfilled(value) : value;
          },
          rejected,
        );
      };
      Reflect.set(Promise.prototype, "then", observeResult);
    });
    await page.goto(url, { waitUntil: "networkidle" });
    assert.equal(requests, 1);
    assert.equal(await page.evaluate(() => document.images[0].complete), true);
    assert.equal(await page.evaluate(() => document.images[0].currentSrc), imageUrl);
    const first = await page.screenshot({ externalCss: false });
    assert.deepEqual(pngPixels(first).pixel(1, 1).slice(0, 3), [255, 0, 0]);
    assert.equal(requests, 1, "a capture uses the image already loaded by this document");
    const observed = await page.evaluate(() => globalThis.imageResultFields);
    for (const fields of observed)
      assert.deepEqual(fields, ["height", "ok", "ratio", "status", "url", "width"]);
    assert.equal(await page.evaluate(() => typeof globalThis.liveImageAssets), "undefined");
    assert.ok((await page.screenshot({ externalCss: false })).equals(first));
    assert.equal(requests, 1);
    assert.equal(await page.evaluate(() => globalThis.imageBytesSerialized), false);
    peer = await page.context().newPage();
    await peer.goto(url, { waitUntil: "networkidle" });
    const second = await peer.screenshot({ externalCss: false });
    assert.deepEqual(pngPixels(second).pixel(1, 1).slice(0, 3), [0, 0, 255]);
    assert.equal(requests, 2, "no-store bytes do not cross document or page lifetimes");
  } finally {
    await peer?.close();
    await page.close();
    server.closeAllConnections();
    imageServer.closeAllConnections();
    await Promise.all([
      new Promise((resolve) => server.close(resolve)),
      new Promise((resolve) => imageServer.close(resolve)),
    ]);
  }
});

test("late native image completion cannot replace the new document's resident bytes", async () => {
  const { createRequire } = await import("node:module");
  const { createServer } = await import("node:http");
  const { pngPixels } = await import("../../harness/hotpath/png-pixels.mjs");
  const { newPage } = await import("./index.mjs");
  const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  const red = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  const blue = native.screenshot("<body style='margin:0;background:blue'>", 3, 2);
  let requests = 0,
    release,
    seen;
  const started = new Promise((resolve) => {
    seen = resolve;
  });
  const html = "<body style='margin:0'><img src='/image.png' width='3' height='2'></body>";
  const server = createServer((request, response) => {
    response.setHeader("cache-control", "no-store");
    if (request.url === "/image.png") {
      requests++;
      response.setHeader("content-type", "image/png");
      if (requests === 1) {
        release = () => {
          if (!response.writableEnded && !response.destroyed) response.end(red);
        };
        seen();
      } else response.end(blue);
    } else {
      response.setHeader("content-type", "text/html");
      response.end("<body>initial</body>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const url = `http://127.0.0.1:${server.address().port}/`;
  const page = newPage({ viewport: { width: 3, height: 2 } });
  try {
    await page.goto(url, { waitUntil: "load" });
    await page.setContent(html, { waitUntil: "domcontentloaded" });
    await started;
    await page.setContent(html);
    assert.equal(requests, 2);
    release();
    await page.waitForLoadState("networkidle");
    const result = await page.screenshot({ externalCss: false });
    assert.deepEqual(pngPixels(result).pixel(1, 1).slice(0, 3), [0, 0, 255]);
    assert.equal(requests, 2);
    await page.screenshot({ externalCss: false });
    assert.equal(requests, 2);
  } finally {
    release?.();
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
});

test("resident generation reads stay ordered across concurrent capture plans", async () => {
  const { createRequire } = await import("node:module");
  const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  const original = native.liveImageAssets;
  const queries = [];
  let release;
  native.liveImageAssets = (_session, query) => {
    queries.push(JSON.parse(query));
    if (queries.length === 1)
      return new Promise((resolve) => {
        release = () => resolve(JSON.stringify({ generation: "1:1", images: { a: "cmVk" } }));
      });
    return Promise.resolve(JSON.stringify({ generation: "1:2", images: { b: "Ymx1ZQ==" } }));
  };
  try {
    const page = Object.assign(Object.create(Page.prototype), { _session: "owned-page" });
    const cache = { images: Object.create(null) };
    const first = page._seedResidentImages(cache, ["a"]);
    const second = page._seedResidentImages(cache, ["b"]);
    await Promise.resolve();
    assert.equal(queries.length, 1);
    release();
    await Promise.all([first, second]);
    assert.equal(queries.length, 2);
    assert.equal(queries[1].generation, "1:1");
    assert.equal(cache.generation, "1:2");
    assert.deepEqual(Object.keys(cache.images), ["b"]);
    assert.deepEqual(await cache.images.b, ["b", Buffer.from("blue")]);
  } finally {
    if (original === undefined) delete native.liveImageAssets;
    else native.liveImageAssets = original;
  }
});
