import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import {
  decodeEvaluationValue,
  encodeEvaluationError,
  encodeEvaluationValue,
  safeStringifyEnvelope,
  safeStringifyTuple,
} from "./evaluation-codec.mjs";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  if (!String(error).includes("no prebuilt binary found")) throw error;
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function withFixture(run) {
  const server = createServer((_, response) => {
    response.setHeader("Content-Type", "text/html");
    response.end("<!doctype html><title>JSON guard</title>");
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  try {
    await run(origin);
  } finally {
    await new Promise((resolve) => server.close(resolve));
  }
}

test("driver evaluation transport survives Object.prototype.toJSON = () => ({}) (#531)", async () => {
  const { chromium } = shim;
  const browser = await chromium.launch();
  try {
    await withFixture(async (origin) => {
      const context = await browser.newContext();
      try {
        const page = await context.newPage();
        await page.goto(origin, { waitUntil: "networkidle" });
        await page.evaluate(() => {
          Object.prototype.toJSON = () => ({ hijacked: true });
        });
        const seen = await page.evaluate(() => ({ a: 1, b: "two", c: [3, 4, null] }));
        assert.deepEqual(seen, { a: 1, b: "two", c: [3, 4, null] });
        const arr = await page.evaluate(() => [10, 20, 30]);
        assert.deepEqual(arr, [10, 20, 30]);
      } finally {
        await context.close();
      }
    });
  } finally {
    await browser.close();
  }
});

test("evaluateHandle round-trips the descriptor under a toJSON hook (#531)", async () => {
  const { chromium } = shim;
  const browser = await chromium.launch();
  try {
    await withFixture(async (origin) => {
      const context = await browser.newContext();
      try {
        const page = await context.newPage();
        await page.goto(origin, { waitUntil: "networkidle" });
        await page.evaluate(() => {
          Object.prototype.toJSON = () => ({ hijacked: true });
        });
        const handle = await page.evaluateHandle(() => ({ seen: 42 }));
        const seen = await handle.evaluate((value) => value.seen);
        assert.equal(
          seen,
          42,
          "retain=true descriptor must still round-trip when toJSON is hooked",
        );
      } finally {
        await context.close();
      }
    });
  } finally {
    await browser.close();
  }
});

test("driver evaluation transport survives Array.prototype.toJSON = () => ({}) (#531)", async () => {
  const { chromium } = shim;
  const browser = await chromium.launch();
  try {
    await withFixture(async (origin) => {
      const context = await browser.newContext();
      try {
        const page = await context.newPage();
        await page.goto(origin, { waitUntil: "networkidle" });
        await page.evaluate(() => {
          Array.prototype.toJSON = () => ({ hijacked: true });
        });
        const seen = await page.evaluate(() => [1, 2, 3]);
        assert.deepEqual(seen, [1, 2, 3]);
        const nested = await page.evaluate(() => ({ list: [1, 2], flag: true }));
        assert.deepEqual(nested, { list: [1, 2], flag: true });
      } finally {
        await context.close();
      }
    });
  } finally {
    await browser.close();
  }
});

test("evaluation tuples preserve their wire types and exact UTF-16", () => {
  const text = '"\\\b\t\n\f\r\u0001😀\ud800x\udc00';
  const input = {
    payload: [text, null, true, false, 42, -0, Infinity, -Infinity, NaN, 123n, undefined],
    date: new Date("2026-01-01T00:00:00Z"),
    expression: /x/gi,
  };
  input.self = input;
  const tagged = encodeEvaluationValue(input);
  const wire = safeStringifyEnvelope("value", tagged);
  assert.equal(wire, JSON.stringify({ value: tagged }));
  const result = decodeEvaluationValue(JSON.parse(wire).value);
  assert.equal(result.self, result);
  assert.deepEqual(result.payload, input.payload);
  assert.deepEqual(result.date, input.date);
  assert.deepEqual(result.expression, input.expression);
  assert.equal(safeStringifyTuple(Infinity), "null");
  assert.throws(() => safeStringifyTuple({}), TypeError);
});

test("evaluation error envelopes retain escaped messages and optional stacks", () => {
  const error = new RangeError('line\n"two"');
  const tagged = encodeEvaluationError(error);
  assert.deepEqual(JSON.parse(safeStringifyEnvelope("error", tagged)), { error: tagged });
  assert.deepEqual(JSON.parse(safeStringifyEnvelope("error", encodeEvaluationError("oops"))), {
    error: { name: "Error", message: "oops" },
  });
  assert.throws(() => safeStringifyEnvelope("result", tagged), TypeError);
});
