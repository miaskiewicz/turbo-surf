import assert from "node:assert/strict";
import test from "node:test";
import { scheduleBindingDrain, throwBindingDrainError } from "./binding-drain.mjs";

const checkpoint = () => new Promise(setImmediate);

function gate() {
  let resolve, reject;
  const promise = new Promise((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

test("consecutive input events share pending host work and later input can service new calls", async () => {
  const held = gate();
  let calls = 0;
  const page = {
    _session: 1,
    _drainBindings: () => {
      calls++;
      return held.promise;
    },
  };
  assert.equal(scheduleBindingDrain(page), undefined);
  scheduleBindingDrain(page);
  assert.equal(calls, 1);
  held.resolve();
  await checkpoint();
  scheduleBindingDrain(page);
  assert.equal(calls, 2);
});

for (const error of [new RangeError("real-drain-error"), undefined]) {
  test(`a background collection rejection remains observable (${String(error)})`, async () => {
    const page = { _session: 1, _drainBindings: () => Promise.reject(error) };
    scheduleBindingDrain(page);
    await checkpoint();
    let thrown = false;
    try {
      throwBindingDrainError(page);
    } catch (actual) {
      thrown = true;
      assert.equal(actual, error);
    }
    assert.equal(thrown, true);
    assert.doesNotThrow(() => throwBindingDrainError(page));
  });
}

test("old document failures do not leak into a replacement session", async () => {
  const held = gate();
  const page = { _session: 1, _drainBindings: () => held.promise };
  scheduleBindingDrain(page);
  page._session = 2;
  held.reject(new Error("old document"));
  await checkpoint();
  assert.doesNotThrow(() => throwBindingDrainError(page));
  page._drainBindings = () => Promise.resolve();
  assert.doesNotThrow(() => scheduleBindingDrain(page));
});
