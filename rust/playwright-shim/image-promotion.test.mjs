import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
import { newPage } from "./index.mjs";
import { pngPixels } from "./screenshot-test-utils.mjs";

const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
const raster = (color) => native.screenshot(`<body style='margin:0;background:${color}'>`, 3, 2);
const html =
  '<body style="margin:0"><img style="display:block" src="/asset.png" width="3" height="2"><form method="post" action="/"><button>Submit</button></form>';

async function fixture(run, headers = {}) {
  const state = { documents: [], images: 0, color: "red" };
  const server = createServer((req, res) => {
    res.setHeader("cache-control", "no-store");
    if (req.url === "/asset.png") {
      state.images++;
      res.setHeader("content-type", "image/png");
      if (state.holdFirst && state.images === 1) {
        state.releaseImage = () => res.end(raster("red"));
        state.imageArrived();
      } else res.end(raster(state.color));
    } else {
      state.documents.push(req.method);
      res.writeHead(200, { "content-type": "text/html", ...headers });
      res.end(html);
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = newPage({ viewport: { width: 30, height: 40 } });
  try {
    await run(page, `http://127.0.0.1:${server.address().port}/`, state);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

test("static image promotion reuses document bytes but same-URL POST paints fresh bytes (#489)", async () => {
  await fixture(async (page, url, state) => {
    await page.goto(url, { waitUntil: "commit" });
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [255, 0, 0, 255],
    );
    assert.equal(state.images, 1);
    await page._ensureLive();
    assert.equal(state.images, 1, "promotion retains this already-fetched document asset");
    assert.equal(await page.locator("img").evaluate((image) => image.naturalWidth), 3);
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [255, 0, 0, 255],
    );
    state.color = "blue";
    await page.locator("button").click();
    await page.waitForLoadState("load", { timeout: 10000 });
    assert.deepEqual(state.documents, ["GET", "POST"]);
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [0, 0, 255, 255],
    );
    assert.equal(state.images, 2, "new actual document fetched its own blue image");
  });
});

test("promotion image handoff cannot bypass the actual document CSP", async () => {
  await fixture(
    async (page, url, state) => {
      await page.goto(url, { waitUntil: "commit" });
      await page.screenshot({ externalCss: false });
      const requests = state.images;
      await page._ensureLive();
      assert.equal(await page.locator("img").evaluate((image) => image.naturalWidth), 0);
      assert.equal(state.images, requests, "CSP blocked before network or seed decode");
    },
    { "content-security-policy": "img-src 'none'" },
  );
});

test("a copied static image capability cannot seed another Page", async () => {
  await fixture(async (owner, url, state) => {
    await owner.goto(url, { waitUntil: "commit" });
    await owner.screenshot({ externalCss: false });
    const token = owner._imageCache.documentToken;
    const foreign = newPage({ viewport: { width: 30, height: 40 } });
    try {
      await foreign.goto(url, { waitUntil: "commit" });
      foreign._imageCache = {
        key: `${foreign._url}\u0000${foreign._html}`,
        images: Object.create(null),
        documentToken: token,
      };
      state.color = "blue";
      await foreign._ensureLive();
      assert.equal(state.images, 2, "foreign owner must perform its actual image request");
      assert.deepEqual(
        pngPixels(await foreign.screenshot({ externalCss: false })).pixel(1, 1),
        [0, 0, 255, 255],
      );
    } finally {
      await foreign.close();
    }
  });
});

test("promoted image bytes still obey the actual live resource budget", async () => {
  await fixture(async (page, url, state) => {
    await page.goto(url, { waitUntil: "commit" });
    await page.screenshot({ externalCss: false });
    assert.equal(state.images, 1);
    page._context._maxResponseBytes = 1;
    await page._ensureLive();
    assert.equal(await page.locator("img").evaluate((image) => image.naturalWidth), 0);
    assert.equal(state.images, 2, "oversized seed fell through the bounded actual loader");
  });
});

test("a retired document's in-flight static image cannot seed its same-URL replacement", async () => {
  await fixture(async (page, url, state) => {
    await page.goto(url, { waitUntil: "commit" });
    state.holdFirst = true;
    const arrived = new Promise((resolve) => {
      state.imageArrived = resolve;
    });
    const retiredShot = page.screenshot({ externalCss: false });
    await arrived;
    await page.goto(url, { waitUntil: "commit" });
    state.color = "blue";
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [0, 0, 255, 255],
    );
    state.releaseImage();
    await retiredShot;
    await page._ensureLive();
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [0, 0, 255, 255],
    );
    assert.equal(state.images, 2, "late retired red bytes never replace the current blue seed");
    assert.deepEqual(state.documents, ["GET", "GET"]);
  });
});

test("clearing the static cache revokes its exact native image capability", async () => {
  await fixture(async (page, url, state) => {
    await page.goto(url, { waitUntil: "commit" });
    await page.screenshot({ externalCss: false });
    const token = page._imageCache.documentToken;
    page._clearImageCache();
    page._imageCache = {
      key: `${page._url}\u0000${page._html}`,
      images: Object.create(null),
      documentToken: token,
    };
    state.color = "blue";
    await page._ensureLive();
    assert.equal(state.images, 2, "discarded token cannot recover retired red bytes");
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [0, 0, 255, 255],
    );
  });
});

test("setContent on the retained native Document cannot reuse the earlier static seed", async () => {
  await fixture(async (page, url, state) => {
    await page.goto(url, { waitUntil: "commit" });
    await page.screenshot({ externalCss: false });
    await page._ensureLive();
    assert.equal(state.images, 1);
    state.color = "blue";
    await page.setContent(html, { waitUntil: "load", timeout: 10000 });
    assert.equal(page.url(), url);
    assert.deepEqual(
      pngPixels(await page.screenshot({ externalCss: false })).pixel(1, 1),
      [0, 0, 255, 255],
    );
    assert.equal(state.images, 2, "content replacement fetches its own current image");
    assert.deepEqual(state.documents, ["GET"]);
  });
});
