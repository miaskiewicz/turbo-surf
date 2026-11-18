import assert from "node:assert/strict";
import { test } from "node:test";
import { expect } from "./index.mjs";

test("iterable and text assertions distinguish identity, structural equality and string patterns", () => {
  const item = { id: 7 };
  expect(new Set([item])).toContain(item);
  expect(new Set([item])).not.toContain({ id: 7 });
  expect(new Set([item])).toContainEqual({ id: 7 });
  expect(new Set([item])).not.toContainEqual({ id: 8 });
  expect(new Uint8Array([1, 2])).toContain(2);
  expect("invoice-42").toMatch("^invoice-\\d+$");
  expect("invoice-42").not.toMatch("^receipt-");
  assert.throws(() => expect([1, 2]).toContain(3), /expected.*contain/);
  assert.throws(() => expect([{ id: 7 }]).toContainEqual({ id: 8 }), /expected to contain/);
});

test("unbounded callback assertions await real asynchronous state and retry rejected observations", async () => {
  let attempts = 0;
  await expect(async () => {
    await Promise.resolve();
    attempts++;
    expect(attempts).toBe(2);
  }).toPass({ timeout: 0, intervals: [0] });
  assert.equal(attempts, 2);
  let reads = 0;
  await expect
    .poll(
      async () => {
        await Promise.resolve();
        return { state: ++reads === 1 ? "pending" : "ready" };
      },
      { timeout: 0, intervals: [0] },
    )
    .toHaveProperty("state", "ready");
  assert.equal(reads, 2);
});

test("soft assertions outside a shim test propagate failures and preserve nested negation and promise rejection", async () => {
  expect.soft({ id: 1 }).not.toEqual({ id: 2 });
  assert.throws(() => expect.soft(1).toBe(2), /expected 1 to be 2/);
  const failure = new Error("actual asynchronous rejection");
  await assert.rejects(
    expect.soft(Promise.reject(failure)).resolves.toBe(1),
    (error) => error === failure,
  );
  await expect.soft(Promise.resolve(2)).resolves.not.toBe(3);
});
