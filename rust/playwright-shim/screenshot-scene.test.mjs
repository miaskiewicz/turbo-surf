import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { newPage } from "./index.mjs";
import { paintScreenshotScene } from "./screenshot-scene.mjs";
import { pngPixels } from "../../harness/hotpath/png-pixels.mjs";
import { screenshotRowDocument as rows } from "../../harness/hotpath/screenshot-perf-fixture.mjs";
const native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");

test("unchanged viewport captures reuse one prepared scene with identical pixels", async () => {
  const page = newPage({ viewport: { width: 300, height: 200 } });
  try {
    await page.setContent(rows(5000));
    let first;
    for (let capture = 0; capture < 4; capture++) {
      const png = await page.screenshot({ images: false, externalCss: false });
      first ??= png;
      assert.ok(png.equals(first));
    }
    assert.equal(page._screenshotSceneStats?.preparations, 1);
    assert.equal(page._screenshotScene.scene.paintCount, 4);
    assert.ok(
      (await page.screenshot({ images: false, externalCss: false, type: "svg" }))
        .toString()
        .startsWith("<svg"),
    );
    assert.equal(page._screenshotSceneStats.preparations, 1);
    assert.equal(page._screenshotScene.scene.paintCount, 5);
    const pixels = pngPixels(first);
    assert.deepEqual(pixels.pixel(5, 5).slice(0, 3), [255, 0, 0]);
    assert.deepEqual(pixels.pixel(5, 15).slice(0, 3), [0, 0, 255]);
    assert.equal(await page.locator(".row").count(), 5000);
  } finally {
    await page.close();
  }
});

test("scene reuse invalidates HTML, stylesheet, viewport and output-layout options and closes old state", async () => {
  const page = newPage({ viewport: { width: 300, height: 200 } });
  const options = { images: false, externalCss: false };
  try {
    await page.setContent(rows(30));
    const first = await page.screenshot(options);
    const old = page._screenshotScene.scene;
    page._html = page._html.replace("background:red", "background:green");
    const changed = await page.screenshot(options);
    assert.equal(old.closed, true);
    assert.ok(!changed.equals(first));
    await page.setViewportSize({ width: 320, height: 200 });
    assert.equal((await page.screenshot(options)).readUInt32BE(16), 320);
    assert.ok((await page.screenshot({ ...options, fullPage: true })).readUInt32BE(20) > 200);
    page._fetchLinkedCss = async () => ".row:nth-child(odd){background:blue}";
    const styled = await page.screenshot({ images: false });
    assert.deepEqual(pngPixels(styled).pixel(5, 5).slice(0, 3), [0, 0, 255]);
    assert.equal(page._screenshotSceneStats.preparations, 5);
    const finalScene = page._screenshotScene.scene;
    await page.close();
    assert.equal(finalScene.closed, true);
    assert.throws(() => finalScene.png(), /closed/);
  } finally {
    await page.close();
  }
});

test("owned scenes retain copied inputs and invalidate changed image bytes even within the same Buffer", async () => {
  const red = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  const blue = native.screenshot("<body style='margin:0;background:blue'>", 3, 2);
  assert.equal(red.length, blue.length);
  const page = newPage({ viewport: { width: 30, height: 20 } });
  try {
    await page.setContent("<body style='margin:0'><img src='pixel.png' width='30' height='20'>");
    page._fetchImages = async () => ({ "pixel.png": red });
    const first = await page.screenshot({ externalCss: false });
    const original = page._screenshotScene.scene;
    assert.ok(original.png().equals(first));
    blue.copy(red);
    assert.ok(original.png().equals(first), "native prepared inputs are immutable owned snapshots");
    const changed = await page.screenshot({ externalCss: false });
    assert.deepEqual(pngPixels(changed).pixel(5, 5).slice(0, 3), [0, 0, 255]);
    assert.equal(original.closed, true);
    assert.equal(page._screenshotSceneStats.preparations, 2);
  } finally {
    await page.close();
  }
});

test("scene keys include independent intrinsic metadata and preserve owned missing-body layout", async () => {
  const page = newPage();
  const html =
    "<body style='margin:0'><img src='missing.png' style='display:block'><div style='height:5px;width:10px;background:red'></div>";
  const inputs = {
    html,
    css: "",
    images: {},
    width: 40,
    height: 40,
    optionsJson: "{}",
    asSvg: false,
  };
  try {
    const first = paintScreenshotScene(page, native, {
      ...inputs,
      metadataJson: '{"missing.png":{"width":10,"height":10}}',
    });
    const old = page._screenshotScene.scene;
    const changed = paintScreenshotScene(page, native, {
      ...inputs,
      metadataJson: '{"missing.png":{"width":10,"height":20}}',
    });
    assert.deepEqual(pngPixels(first).pixel(5, 12).slice(0, 3), [255, 0, 0]);
    assert.notDeepEqual(pngPixels(changed).pixel(5, 12).slice(0, 3), [255, 0, 0]);
    assert.equal(old.closed, true);
    assert.equal(page._screenshotSceneStats.preparations, 2);
  } finally {
    await page.close();
  }
});

test("prepared scenes retain mask overlays, fractional scale and transparent backgrounds", () => {
  const html =
    "<body style='margin:0'><div id='masked' style='position:absolute;left:20px;top:10px;width:10px;height:10px;background:red'></div>";
  const all = JSON.parse(native.query(html, "*", "css"));
  const index = all.findIndex((node) => native.attrOf(html, node.node, "id") === "masked");
  const scene = new native.ScreenshotScene(
    html,
    "",
    {},
    100,
    80,
    JSON.stringify({
      deviceScale: 1.2,
      omitBackground: true,
      maskIndices: [index],
      maskColor: "#00ff00",
    }),
    "{}",
  );
  try {
    const pixels = pngPixels(scene.png());
    assert.equal(pixels.width, 120);
    assert.equal(pixels.height, 96);
    assert.deepEqual(pixels.pixel(30, 18), [0, 255, 0, 255]);
    assert.equal(pixels.pixel(0, 0)[3], 0);
    const svg = scene.svg();
    assert.match(svg, /width="120"/);
    assert.match(svg, /height="96"/);
  } finally {
    scene.close();
  }
});
