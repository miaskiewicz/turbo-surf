import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { withScreenshotImageSite } from "../../harness/hotpath/screenshot-perf-fixture.mjs";
import { newPage } from "./index.mjs";
import { pngPixels } from "../../harness/hotpath/png-pixels.mjs";
const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");

test("viewport screenshot selects contributing image bytes while preserving independent intrinsic sizes", async () => {
  await withScreenshotImageSite(native, async ({ url, requests }) => {
    const page = newPage({ viewport: { width: 300, height: 200 } });
    try {
      await page.goto(url, { waitUntil: "networkidle" });
      assert.equal(
        await page.evaluate(() =>
          Array.from(document.images).every(
            (image) => image.naturalWidth === 3 && image.naturalHeight === 2,
          ),
        ),
        true,
      );
      const refs = Array.from({ length: 6 }, (_, index) => `/image/${index}.png`);
      assert.deepEqual([...requests].sort(), refs, "the native document loaded each image once");
      requests.length = 0;
      const viewport = await page.screenshot({ externalCss: false });
      assert.deepEqual(requests, [], "capture reuses the native document's resident bytes");
      assert.deepEqual(
        page._capturePlan.refs,
        [refs[0]],
        "only viewport-contributing pixels are selected",
      );
      assert.deepEqual(
        Object.keys(page._imageCache.images),
        [refs[0]],
        "offscreen bodies stay outside capture preparation",
      );
      assert.equal(viewport.readUInt32BE(16), 300);
      assert.equal(viewport.readUInt32BE(20), 200);
      assert.deepEqual(pngPixels(viewport).pixel(150, 100).slice(0, 3), [255, 0, 0]);
      requests.length = 0;
      await page.evaluate(() => {
        Object.defineProperty(HTMLImageElement.prototype, "naturalWidth", {
          configurable: true,
          get: () => 999,
        });
      });
      const repeated = await page.screenshot({ externalCss: false });
      assert.ok(repeated.equals(viewport), "guest getters cannot spoof native intrinsic metadata");
      assert.equal(requests.length, 0);
      const full = await page.screenshot({ externalCss: false, fullPage: true });
      assert.deepEqual(requests, [], "full-page capture also reuses current-document responses");
      assert.deepEqual(page._capturePlan.refs, refs);
      assert.deepEqual(Object.keys(page._imageCache.images).sort(), refs);
      assert.equal(full.readUInt32BE(20), 1200);
      const pixels = pngPixels(full);
      for (let index = 0; index < 6; index++)
        assert.deepEqual(pixels.pixel(150, 100 + index * 200).slice(0, 3), [255, 0, 0]);
    } finally {
      await page.close();
    }
  });
});

test("capture resource planning preserves pixels, unknown intrinsic dependencies and extending effects", () => {
  const refs = ["0.png", "1.png", "2.png"];
  const metadata = JSON.stringify(
    Object.fromEntries(refs.map((ref) => [ref, { width: 3, height: 2 }])),
  );
  const html =
    "<body style='margin:0'>" +
    refs.map((ref) => `<img src='${ref}' style='display:block;width:300px'>`).join("");
  const plan = (source, sizes = metadata, options = "{}", css = "") =>
    JSON.parse(native.screenshotCaptureResources(source, css, 300, 200, sizes, options));
  assert.deepEqual(plan(html).pixelUrls, ["0.png"]);
  assert.deepEqual(
    plan(html, "{}").pixelUrls,
    refs,
    "unknown intrinsic dependencies remain selected",
  );
  assert.deepEqual(plan(html, metadata, '{"fullPage":true}').pixelUrls, refs);
  assert.ok(plan(html).fragmentsExamined > 0);
  const red = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  const all = Object.fromEntries(refs.map((ref) => [ref, red]));
  const expected = native.screenshotCapture(html, "", all, 300, 200, "{}", metadata);
  const selected = native.screenshotCapture(html, "", { "0.png": red }, 300, 200, "{}", metadata);
  assert.ok(
    expected.equals(selected),
    "metadata keeps layout identical when offscreen pixel bodies are absent",
  );
  assert.deepEqual(pngPixels(selected).pixel(150, 100).slice(0, 3), [255, 0, 0]);
  for (const style of [
    "position:absolute;top:0",
    "transform:translateY(-400px)",
    "box-shadow:0 -400px 20px black",
    "filter:blur(200px)",
  ]) {
    const source = `<body style='margin:0'><div style='height:400px'></div><div style='${style}'><img src='0.png' width='300' height='200'></div>`;
    assert.ok(plan(source).pixelUrls.includes("0.png"), style);
  }
});

test("resident image keys preserve URL strings without invoking object prototype setters", async () => {
  await withScreenshotImageSite(native, async ({ url, requests, png }) => {
    const page = newPage();
    try {
      await page.goto(url, { waitUntil: "commit" });
      await page.setContent("<img src='__proto__' width='3' height='2'>");
      assert.deepEqual(requests, ["/__proto__"], "setContent loads the native image");
      requests.length = 0;
      await page.screenshot({ externalCss: false });
      assert.equal(Object.hasOwn(page._imageCache.images, "__proto__"), true);
      assert.equal(Object.getPrototypeOf(page._imageCache.images), null);
      const [reference, bytes] = await page._imageCache.images.__proto__;
      assert.equal(reference, "__proto__");
      assert.ok(bytes.equals(png), "the own-property key retains the actual native response bytes");
      await page.screenshot({ externalCss: false });
      assert.deepEqual(
        requests,
        [],
        "resident no-store image bytes do not trigger another HTTP request",
      );
    } finally {
      await page.close();
    }
  });
});

test("viewport capture fetches only the selected image beyond bounded native residency", async () => {
  await withScreenshotImageSite(native, async ({ url, requests }) => {
    const page = newPage({ viewport: { width: 300, height: 200 } });
    try {
      await page.goto(url, { waitUntil: "commit" });
      await page.setContent(
        "<!doctype html><style>body{margin:0}img{display:block;width:300px;position:absolute;left:0;top:400px}#viewport-target{top:0}</style>",
      );
      requests.length = 0;
      // The documented resident-body bound is 60. The first 60 sequential
      // loads fill it; the final image still loads its independent intrinsic size.
      const valid = await page.evaluate(async () => {
        for (let index = 0; index < 61; index++) {
          const image = document.createElement("img");
          const loaded = new Promise((resolve, reject) => {
            image.onload = resolve;
            image.onerror = reject;
          });
          document.body.appendChild(image);
          image.src = `/image/${index}.png`;
          await loaded;
        }
        document.images[60].id = "viewport-target";
        return Array.from(document.images).every(
          (image) => image.naturalWidth === 3 && image.naturalHeight === 2,
        );
      });
      assert.equal(valid, true);
      assert.equal(
        requests.length,
        61,
        "fixture completed the actual sequential native image requests",
      );
      const resident = JSON.parse(
        await native.liveImageAssets(
          page._session,
          JSON.stringify({
            refs: ["/image/0.png", "/image/60.png"],
          }),
        ),
      );
      assert.equal(
        Object.hasOwn(resident.images, "/image/0.png"),
        true,
        "the first admitted response is resident",
      );
      assert.equal(
        Object.hasOwn(resident.images, "/image/60.png"),
        false,
        "the excess completed response has no resident body",
      );
      requests.length = 0;
      const first = await page.screenshot({ externalCss: false });
      assert.deepEqual(requests, ["/image/60.png"], "only the missing viewport image is fetched");
      assert.deepEqual(page._capturePlan.refs, ["/image/60.png"]);
      assert.deepEqual(Object.keys(page._imageCache.images), ["/image/60.png"]);
      assert.deepEqual(pngPixels(first).pixel(150, 100).slice(0, 3), [255, 0, 0]);
      requests.length = 0;
      assert.ok((await page.screenshot({ externalCss: false })).equals(first));
      assert.deepEqual(requests, [], "the fetched capture asset is retained for repeated captures");
    } finally {
      await page.close();
    }
  });
});
