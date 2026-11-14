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
  page._didNativeNavigate({ url: page._url });
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
