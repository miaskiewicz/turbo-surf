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
