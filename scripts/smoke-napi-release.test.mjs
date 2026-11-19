import assert from "node:assert/strict";
import test from "node:test";
import { EXPECTED, verifyRuntime } from "./smoke-napi-release.mjs";

test("runtime contract requires the actual release version and DOM/V8 result", () => {
  let called = false;
  const observed = verifyRuntime(
    {
      version: () => "0.8.4",
      evaluate: (html, script) => {
        called = true;
        assert.match(html, /id="probe"/);
        assert.match(script, /node.textContent =/);
        assert.match(script, /crypto.getRandomValues/);
        assert.match(script, /new ReadableStream/);
        return JSON.stringify(EXPECTED);
      },
    },
    "0.8.4",
  );
  assert.equal(called, true);
  assert.deepEqual(observed, EXPECTED);
});

test("loading or version alone never establishes native runtime behavior", () => {
  assert.throws(() => verifyRuntime({ version: () => "0.8.4" }, "0.8.4"));
  assert.throws(() => verifyRuntime({ version: () => "0.8.3" }, "0.8.4"), /version/);
  for (const name of Object.keys(EXPECTED)) {
    const value = { ...EXPECTED, [name]: false };
    assert.throws(
      () =>
        verifyRuntime({ version: () => "0.8.4", evaluate: () => JSON.stringify(value) }, "0.8.4"),
      /contract/,
    );
  }
});
