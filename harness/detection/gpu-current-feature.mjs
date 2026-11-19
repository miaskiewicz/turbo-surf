import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { resolve } from "node:path";

assert.ok(process.env.TURBO_SURF_NAPI_PATH, "select the exact GPU-feature addon explicitly");
const addon = resolve(process.env.TURBO_SURF_NAPI_PATH);
const native = createRequire(import.meta.url)(addon);
const probe = (await readFile(new URL("./gpu-bridge-validation-probe.js", import.meta.url), "utf8"))
  .trim()
  .replace(/;$/, "");
const result = JSON.parse(native.evaluate("<!doctype html><body>", `JSON.stringify(${probe})`));
for (const [key, color] of [
  ["red", [255, 0, 0, 255]],
  ["blue", [0, 0, 255, 255]],
]) {
  const draw = result[key];
  assert.equal(draw.vendor, "Google Inc. (Apple)");
  assert.match(draw.renderer, /ANGLE Metal Renderer:/);
  assert.equal(draw.pixels.length, 8 * 8 * 4);
  assert.ok(
    draw.pixels.every((value, index) => value === color[index % 4]),
    `${key} shader pixels`,
  );
}
assert.notDeepEqual(result.red.pixels, result.blue.pixels);
console.log(
  JSON.stringify({
    addon,
    sha256: createHash("sha256")
      .update(await readFile(addon))
      .digest("hex"),
    vendor: result.red.vendor,
    renderer: result.red.renderer,
    checks: ["red-full-frame-RGBA", "blue-full-frame-RGBA", "distinct-fragment-shaders"],
  }),
);
