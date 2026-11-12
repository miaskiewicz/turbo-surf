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
      requests.length = 0;
      const viewport = await page.screenshot({ externalCss: false });
      assert.deepEqual(requests, ["/image/0.png"]);
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
      assert.equal(
        new Set(requests).size,
        5,
        "full-page selection obtains remaining offscreen images",
      );
      assert.equal(full.readUInt32BE(20), 1200);
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
