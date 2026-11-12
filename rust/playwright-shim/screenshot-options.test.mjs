import assert from "node:assert/strict";
import test from "node:test";
import { RECTANGLES, jpegDimensions, pngPixels } from "./screenshot-test-utils.mjs";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}
const { newPage } = shim;

test("screenshot masks resolve scoped and indexed locators in the captured snapshot (#469)", async () => {
  const p = newPage({ viewport: { width: 100, height: 80 } });
  try {
    await p.setContent(
      '<style>html,body{margin:0}.secret{position:absolute;top:10px;width:20px;height:20px;background:red}#outside{left:10px}#inside{left:50px}#bottom{position:absolute;top:110px;width:20px;height:20px;background:blue}</style><div id="outside" class="secret"></div><section id="scope"><div id="inside" class="secret"></div></section><div id="bottom"></div>',
    );
    const mask = p.locator("#scope").locator(".secret").first();
    const plain = pngPixels(await p.screenshot());
    assert.deepEqual(plain.pixel(60, 15), [255, 0, 0, 255]);
    const covered = pngPixels(await p.screenshot({ mask: [mask] }));
    assert.deepEqual(covered.pixel(60, 15), [255, 0, 255, 255]);
    assert.deepEqual(covered.pixel(15, 15), [255, 0, 0, 255]);
    const full = pngPixels(
      await p.screenshot({
        scale: "css",
        fullPage: true,
        mask: [p.locator("#bottom")],
        maskColor: "rgb(0, 255, 0)",
      }),
    );
    assert.deepEqual(full.pixel(5, 115), [0, 255, 0, 255]);
    await assert.rejects(
      () => p.screenshot({ mask: [mask], maskColor: "not-a-color" }),
      /maskColor/,
    );
  } finally {
    await p.close();
  }
});

test("screenshot masks clip descendants and retain source indices after SVG (#469)", async () => {
  const p = newPage({ viewport: { width: 100, height: 80 } });
  try {
    await p.setContent(
      '<style>html,body{margin:0}#clip{position:absolute;left:10px;top:10px;width:10px;height:10px;overflow:hidden}#secret{width:30px;height:30px;background:red}#transformed{position:absolute;left:40px;top:40px;width:10px;height:10px;background:red;transform:translateX(10px)}</style><svg width="1" height="1"><rect width="1" height="1"/></svg><div id="clip"><div id="secret"></div></div><div id="transformed"></div>',
    );
    const masked = pngPixels(await p.screenshot({ mask: [p.locator("#secret")] }));
    assert.deepEqual(masked.pixel(15, 15), [255, 0, 255, 255]);
    assert.deepEqual(masked.pixel(25, 15), [255, 255, 255, 255]);
    const transformed = pngPixels(await p.screenshot({ mask: [p.locator("#transformed")] }));
    assert.deepEqual(transformed.pixel(55, 45), [255, 0, 255, 255]);
    assert.deepEqual(transformed.pixel(45, 45), [255, 255, 255, 255]);
    const svg = (await p.screenshot({ type: "svg", mask: [p.locator("#secret")] })).toString();
    assert.match(svg, /<svg/);
    const other = newPage();
    try {
      await assert.rejects(
        () => p.screenshot({ mask: [other.locator("body")] }),
        /different.page|frame/i,
      );
    } finally {
      await other.close();
    }
  } finally {
    await p.close();
  }
});

test("screenshot masks and native capture share one snapshot across resource preparation (#469)", async () => {
  const p = newPage({ viewport: { width: 100, height: 80 } });
  try {
    await p.setContent(
      '<style>html,body{margin:0}#secret{position:absolute;left:10px;top:10px;width:20px;height:20px;background:red}</style><div id="secret"></div>',
    );
    let release;
    const barrier = new Promise((resolve) => {
      release = resolve;
    });
    p._fetchLinkedCss = async () => {
      await barrier;
      return "";
    };
    const pending = p.screenshot({ mask: [p.locator("#secret")] });
    await p.setContent("<style>html,body{margin:0;background:blue}</style><p>replacement</p>");
    release();
    const shot = pngPixels(await pending);
    assert.deepEqual(shot.pixel(15, 15), [255, 0, 255, 255]);
    assert.deepEqual(shot.pixel(90, 70), [255, 255, 255, 255]);
  } finally {
    await p.close();
  }
});
test("full-page capture includes positioned overflow and viewport minimum (#468)", async () => {
  const p = newPage({ viewport: { width: 100, height: 80 } });
  try {
    await p.setContent(RECTANGLES);
    const viewport = pngPixels(await p.screenshot());
    assert.deepEqual([viewport.width, viewport.height], [100, 80]);
    const full = pngPixels(await p.screenshot({ fullPage: true }));
    assert.deepEqual([full.width, full.height], [100, 240]);
    assert.deepEqual(full.pixel(15, 15), [255, 0, 0, 255]);
    assert.deepEqual(full.pixel(5, 235), [0, 0, 255, 255]);
    assert.deepEqual(jpegDimensions(await p.screenshot({ fullPage: true, type: "jpeg" })), {
      width: 100,
      height: 240,
    });
    await p.setContent("<style>html,body{margin:0}</style>");
    const empty = pngPixels(await p.screenshot({ fullPage: true }));
    assert.deepEqual([empty.width, empty.height], [100, 80]);
  } finally {
    await p.close();
  }
});
test("omitBackground preserves transparent canvas and authored background pixels (#464)", async () => {
  const p = newPage({ viewport: { width: 100, height: 80 } });
  try {
    await p.setContent(RECTANGLES);
    const opaque = pngPixels(await p.screenshot());
    assert.deepEqual(opaque.pixel(90, 70), [255, 255, 255, 255]);
    const transparent = pngPixels(await p.screenshot({ omitBackground: true }));
    assert.deepEqual(
      await p.screenshot({ type: "jpeg", omitBackground: true }),
      await p.screenshot({ type: "jpeg" }),
    );
    const svg = (await p.screenshot({ type: "svg", omitBackground: true })).toString();
    assert.match(svg, /fill-opacity="0.000"/);
    assert.deepEqual(transparent.pixel(90, 70), [0, 0, 0, 0]);
    assert.deepEqual(transparent.pixel(15, 15), [255, 0, 0, 255]);
    await p.setContent(
      "<style>html,body{margin:0}body{background:rgb(0,128,0);height:80px}</style>",
    );
    const authored = pngPixels(await p.screenshot({ omitBackground: true }));
    assert.deepEqual(authored.pixel(90, 70), [0, 128, 0, 255]);
  } finally {
    await p.close();
  }
});

test("native screenshot device scale preserves CSS layout and scales clip pixels (#470)", async () => {
  for (const deviceScaleFactor of [2, 1.25, 1.5, 1.2]) {
    const p = newPage({ viewport: { width: 100, height: 80 }, deviceScaleFactor });
    try {
      await p.setContent(RECTANGLES);
      const css = pngPixels(await p.screenshot({ scale: "css" }));
      assert.deepEqual([css.width, css.height], [100, 80]);
      assert.deepEqual(css.pixel(15, 15), [255, 0, 0, 255]);
      const device = pngPixels(await p.screenshot({ scale: "device" }));
      assert.deepEqual(
        [device.width, device.height],
        [Math.round(100 * deviceScaleFactor), Math.round(80 * deviceScaleFactor)],
        String(deviceScaleFactor),
      );
      assert.deepEqual(
        device.pixel(Math.round(15 * deviceScaleFactor), Math.round(15 * deviceScaleFactor)),
        [255, 0, 0, 255],
      );
      assert.deepEqual(device.pixel(3, 3), [255, 255, 255, 255]);
      const combined = pngPixels(
        await p.screenshot({ mask: [p.locator("#secret")], omitBackground: true }),
      );
      assert.deepEqual(
        combined.pixel(Math.round(15 * deviceScaleFactor), Math.round(15 * deviceScaleFactor)),
        [255, 0, 255, 255],
      );
      assert.deepEqual(combined.pixel(3, 3), [0, 0, 0, 0]);
      const full = pngPixels(await p.screenshot({ fullPage: true }));
      assert.deepEqual(
        [full.width, full.height],
        [Math.round(100 * deviceScaleFactor), Math.round(240 * deviceScaleFactor)],
      );
      assert.deepEqual(
        full.pixel(Math.round(5 * deviceScaleFactor), Math.round(235 * deviceScaleFactor)),
        [0, 0, 255, 255],
      );
      const clip = pngPixels(await p.screenshot({ clip: { x: 10, y: 10, width: 10, height: 10 } }));
      assert.deepEqual(
        [clip.width, clip.height],
        [Math.round(10 * deviceScaleFactor), Math.round(10 * deviceScaleFactor)],
      );
      assert.deepEqual(clip.pixel(2, 2), [255, 0, 0, 255]);
      const jpeg = jpegDimensions(await p.screenshot({ type: "jpeg", scale: "device" }));
      assert.deepEqual(jpeg, {
        width: Math.round(100 * deviceScaleFactor),
        height: Math.round(80 * deviceScaleFactor),
      });
    } finally {
      await p.close();
    }
  }
});

test("screenshot scale validates context/options and retains SVG CSS coordinates (#470)", async () => {
  for (const deviceScaleFactor of [0, -1, Infinity, NaN])
    assert.throws(() => newPage({ deviceScaleFactor }), /deviceScaleFactor/);
  const p = newPage({ viewport: { width: 100, height: 80 }, deviceScaleFactor: 1.25 });
  try {
    await p.setContent(RECTANGLES);
    const svg = (await p.screenshot({ type: "svg" })).toString("utf8");
    assert.match(svg, /width="125"/);
    assert.match(svg, /height="100"/);
    assert.match(svg, /viewBox="0 0 100 80"/);
    await assert.rejects(() => p.screenshot({ scale: "invalid" }), /scale/);
    await assert.rejects(
      () => p.screenshot({ type: "svg", clip: { x: 0, y: 0, width: 10, height: 10 } }),
      /clip.*unavailable/,
    );
  } finally {
    await p.close();
  }
});

test("fractional DPR viewport dimensions and clipped CSS sizes match Playwright controls (#470)", async () => {
  for (const [width, height, deviceScaleFactor, expected] of [
    [103, 83, 1.2, [124, 100]],
    [101, 81, 1.25, [126, 101]],
    [101, 81, 1.5, [152, 122]],
    [101, 81, 1.6, [162, 130]],
    [101, 81, 0.5, [51, 41]],
  ]) {
    const p = newPage({ viewport: { width, height }, deviceScaleFactor });
    try {
      await p.setContent(RECTANGLES);
      const shot = pngPixels(await p.screenshot());
      assert.deepEqual([shot.width, shot.height], expected);
      const controlClip = pngPixels(
        await p.screenshot({ clip: { x: 0.25, y: 0.5, width: 10.5, height: 20.25 } }),
      );
      assert.deepEqual(
        [controlClip.width, controlClip.height],
        [Math.round(10 * deviceScaleFactor), Math.round(20 * deviceScaleFactor)],
      );
      const clipped = pngPixels(
        await p.screenshot({ clip: { x: 10.25, y: 10.5, width: 10.5, height: 10.25 } }),
      );
      assert.deepEqual(
        [clipped.width, clipped.height],
        [Math.round(10 * deviceScaleFactor), Math.round(10 * deviceScaleFactor)],
      );
      assert.deepEqual(clipped.pixel(2, 2), [255, 0, 0, 255]);
    } finally {
      await p.close();
    }
  }
});
