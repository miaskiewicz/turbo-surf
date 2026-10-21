import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { test } from "node:test";

const require = createRequire(import.meta.url);
let native;
try {
  native = require("../../rust/crates/turbo-surf-napi/index.js");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
}
const tracer = readFileSync(new URL("./interface-trace.js", import.meta.url), "utf8");
const probe = `(() => {
  const canvas = document.createElement('canvas');
  const gl = canvas.getContext('webgl');
  const method = gl.getParameter;
  const version = gl.getParameter(gl.VERSION);
  const extensions = gl.getSupportedExtensions();
  const extension = gl.getExtension('WEBGL_debug_renderer_info');
  const renderer = extension ? gl.getParameter(extension.UNMASKED_RENDERER_WEBGL) : null;
  const pixels = new Uint8Array(4);
  gl.clearColor(1, 0, 0, 1);gl.clear(gl.COLOR_BUFFER_BIT);
  gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
  return JSON.stringify({
    values: {version, extensions, renderer, pixels: Array.from(pixels), width: canvas.width,
      sameContext: canvas.getContext('webgl') === gl,
      sameMethod: gl.getParameter === method,
      sameCanvas: gl.canvas === canvas},
    methodNative: Function.prototype.toString.call(method).includes('[native code]'),
    trace: globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot() || null
  });
})()`;

test(
  "real native context methods are observed while clean GPU controls remain unchanged",
  { skip: !native && "native addon is not built" },
  () => {
    const html = "<!doctype html><body></body>";
    const baseline = JSON.parse(native.evaluate(html, probe));
    const traced = JSON.parse(native.evaluate(html, `${tracer}\n${probe}`));
    assert.equal(baseline.trace, null);
    assert.equal(baseline.methodNative, true);
    assert.equal(traced.methodNative, false, "wrapping is deliberately observable");
    assert.deepEqual(traced.values, baseline.values);
    for (const key of ["sameContext", "sameMethod", "sameCanvas"])
      assert.equal(traced.values[key], true, key);
    const calls = traced.trace.accesses;
    for (const [name, count] of [
      ["HTMLCanvasElement.getContext", 2],
      ["WebGLRenderingContext.getSupportedExtensions", 1],
      ["WebGLRenderingContext.getExtension", 1],
      ["WebGLRenderingContext.clearColor", 1],
      ["WebGLRenderingContext.clear", 1],
      ["WebGLRenderingContext.readPixels", 1],
      ["WebGLRenderingContext.getParameter", baseline.values.renderer === null ? 1 : 2],
    ]) {
      assert.equal(calls.find((row) => row.name === name)?.count, count, name);
    }
    assert.ok(
      traced.trace.coverage.some(
        (row) => row.name === "WebGLRenderingContext.getParameter" && row.status === "installed",
      ),
    );
    assert.equal(traced.trace.droppedCoverage, 0);
  },
);
