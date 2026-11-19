import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { isAbsolute } from "node:path";
import { pngPixels } from "../hotpath/png-pixels.mjs";

const config = JSON.parse(await readFile(process.argv[2], "utf8"));
assert.ok(["napi", "png"].includes(config.kind));
assert.equal(typeof config.avif, "boolean");
assert.equal(typeof config.jxl, "boolean");
assert.ok(isAbsolute(config.addon ?? config.png), "select the exact absolute artifact path");
const fixture = new URL("../../rust/crates/turbo-surf-mcp/tests/fixtures/", import.meta.url);
const page = await readFile(new URL("screenshot_image_formats.html", fixture), "utf8");
const control = JSON.parse(
  await readFile(new URL("screenshot_image_formats_chrome156.json", fixture), "utf8"),
);
assert.equal(createHash("sha256").update(page).digest("hex"), control.pageSha256);
assert.deepEqual(control.errors, []);
assert.match(control.browser, /^Chrome\/156\./);

function validate(png, label) {
  const pixels = pngPixels(png);
  assert.equal(pixels.width, control.viewport[0]);
  assert.equal(pixels.height, control.viewport[1]);
  for (const sample of control.samples) {
    const channels = pixels.pixel(sample.x, sample.y);
    const actual = channels.length === 3 ? [...channels, 255] : channels;
    assert.equal(actual.length, 4);
    const tolerance = sample.id.startsWith("avif") || sample.id.endsWith("-alpha") ? 2 : 0;
    const close = actual.every((value, index) => Math.abs(value - sample.rgba[index]) <= tolerance);
    const decoded = sample.id.startsWith("avif")
      ? config.avif
      : sample.id.startsWith("jxl")
        ? config.jxl
        : true;
    assert.equal(close, decoded, `${label} ${sample.id}: ${actual} versus Chrome ${sample.rgba}`);
    if (!decoded)
      assert.deepEqual(actual, [220, 220, 220, 255], `${label} ${sample.id} neutral placeholder`);
  }
}

async function accepts(native) {
  const png = native.screenshot("<body style='margin:0;background:red'>", 3, 2);
  const seen = [];
  const server = createServer((request, response) => {
    seen.push(request.headers.accept);
    response.writeHead(200, { "content-type": "image/png", "cache-control": "no-store" });
    response.end(png);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const origin = `http://127.0.0.1:${server.address().port}`;
    const bytes = await native.fetchBytes(`${origin}/image.png`, undefined, origin);
    assert.ok(bytes.equals(png));
    assert.equal(seen.length, 1);
    assert.equal(seen[0].includes("image/avif"), config.avif);
    assert.equal(seen[0].includes("image/jxl"), config.jxl);
    return seen[0];
  } finally {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

let accept;
if (config.kind === "napi") {
  const native = createRequire(import.meta.url)(config.addon);
  const assets = Object.fromEntries(
    Array.from(page.matchAll(/ src="([^"]+)"/g), ([, source]) => {
      assert.match(source, /^data:[^,]+;base64,/);
      return [source, Buffer.from(source.slice(source.indexOf(",") + 1), "base64")];
    }),
  );
  assert.equal(Object.keys(assets).length, 8);
  for (const [label, images] of [
    ["empty map", {}],
    ["asset map", assets],
  ])
    validate(native.screenshotWithAssets(page, "", images, ...control.viewport, false), label);
  accept = await accepts(native);
} else validate(await readFile(config.png), "Python PNG artifact");
console.log(
  JSON.stringify({
    kind: config.kind,
    avif: config.avif,
    jxl: config.jxl,
    accept,
    input: config.addon ?? config.png,
    sha256: createHash("sha256")
      .update(await readFile(config.addon ?? config.png))
      .digest("hex"),
    samples: control.samples.length,
  }),
);
