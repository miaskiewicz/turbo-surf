import assert from "node:assert/strict";
import { test } from "node:test";
import { expect } from "./index.mjs";

test("property assertions distinguish array paths, inherited keys and explicit undefined", () => {
  const value = Object.assign(Object.create({ inherited: { ready: true } }), {
    rows: [{ value: undefined }],
    inventory: { count: 2 },
  });
  expect(value).toHaveProperty("rows[0].value", undefined);
  expect(value).toHaveProperty(["inventory", "count"], 2);
  expect(value).toHaveProperty(["rows", 0, "value"], undefined);
  expect(value).toHaveProperty("inherited.ready", true);
  expect(value).not.toHaveProperty("rows[1].value");
  expect(value).not.toHaveProperty("rows[0].value", "different");
  assert.throws(
    () => expect(value).toHaveProperty("rows[1].value", undefined),
    /expected property/,
  );
  assert.throws(() => expect(value).toHaveProperty([]), /must not be empty/);
  assert.throws(() => expect(value).toHaveProperty(2), /string or array/);
  expect({ nested: null }).not.toHaveProperty("nested.value");
});

test("numeric assertions preserve bigint comparison and validate floating precision", () => {
  expect(4n).toBeGreaterThan(3n);
  expect(4n).toBeGreaterThanOrEqual(4n);
  expect(4n).toBeLessThan(5n);
  expect(4n).toBeLessThanOrEqual(4n);
  expect(Infinity).toBeCloseTo(Infinity);
  expect(-Infinity).toBeCloseTo(-Infinity);
  expect(1.125).not.toBeCloseTo(1.25, 4);
  assert.throws(() => expect("4").toBeGreaterThan(3), /Numeric matcher requires/);
  assert.throws(() => expect(4).toBeLessThan("5"), /Numeric matcher requires/);
  assert.throws(() => expect(4n).toBeCloseTo(4), /Numeric matcher requires number arguments/);
  assert.throws(() => expect(4).toBeCloseTo(4, "2"), /Precision must be a number/);
});

test("throw assertions support error instances, constructors and non-Error reasons", async () => {
  class DomainError extends Error {}
  const failure = new DomainError("inventory unavailable");
  const fail = () => {
    throw failure;
  };
  expect(fail).toThrow(DomainError);
  expect(fail).toThrow(new Error("inventory unavailable"));
  expect(fail).toThrow(/inventory/);
  expect(fail).toThrowError("unavailable");
  expect(fail).not.toThrow(TypeError);
  expect(() => {
    throw "plain reason";
  }).toThrow("plain reason");
  assert.throws(
    () => expect(fail).toThrow(42),
    /expects a string, RegExp, Error or error constructor/,
  );
  assert.throws(() => expect("not a function").toThrow(), /requires a function/);
  await expect(Promise.reject(failure)).rejects.toThrowError(DomainError);
  await expect(Promise.reject(failure)).rejects.not.toThrow(TypeError);
  await expect(Promise.resolve({ count: 2 })).resolves.toHaveProperty("count", 2);
  await assert.rejects(expect(Promise.resolve(2)).rejects.toBe(2), /expected promise to reject/);
  await assert.rejects(
    expect(Promise.reject(failure)).resolves.toBe(2),
    (error) => error === failure,
  );
});

test("asymmetric assertions preserve nested shapes, symbols and repeated regex matching", () => {
  const symbol = Symbol("state");
  expect({ [symbol]: 3, extra: true }).toEqual(expect.objectContaining({ [symbol]: 3 }));
  expect({ data: [1, 2] }).toMatchObject({ data: [expect.any(Number), 2] });
  expect(new Date(123)).toMatchObject(new Date(123));
  expect([1, 2]).not.toMatchObject([1]);
  expect(3).not.toMatchObject({ count: 3 });
  expect("queue").toEqual(expect.stringContaining("ue"));
  expect("queue").toEqual(expect.not.stringContaining("xx"));
  const pattern = /queue/g;
  const matcher = expect.stringMatching(pattern);
  expect("queue").toEqual(matcher);
  expect("queue").toEqual(matcher);
  assert.equal(pattern.lastIndex, 0);
  expect([1, 2, 3]).toEqual(expect.arrayContaining([3, 1]));
  expect([1, 2]).toEqual(expect.not.arrayContaining([3]));
  expect(3n).toEqual(expect.any(BigInt));
  expect(Symbol("s")).toEqual(expect.any(Symbol));
  expect(new Set()).toEqual(expect.any(Set));
  expect(null).not.toEqual(expect.anything());
  for (const [factory, invalid] of [
    ["objectContaining", null],
    ["arrayContaining", {}],
    ["stringContaining", 3],
    ["stringMatching", {}],
  ]) {
    assert.throws(() => expect[factory](invalid), TypeError);
  }
  assert.throws(() => expect.any({}), /requires a constructor/);
});

test("callback assertions retry real state changes and reject invalid retry contracts", async () => {
  let attempts = 0;
  await expect(() => {
    attempts++;
    expect(attempts).toBeGreaterThanOrEqual(3);
  }).toPass({ timeout: 1000, intervals: [0] });
  assert.equal(attempts, 3);
  let observed = 0;
  await expect.poll(() => ++observed, { timeout: 1000, intervals: [0] }).toBe(2);
  assert.equal(observed, 2);
  await expect.poll(() => 2).not.toBe(3);
  assert.equal(expect.poll(() => 2).then, undefined);
  assert.throws(() => expect.poll(2), /requires a callback/);
  await assert.rejects(expect(2).toPass(), /requires a callback/);
  for (const options of [
    { timeout: -1 },
    { timeout: Infinity },
    { intervals: [] },
    { intervals: [-1] },
    { intervals: [NaN] },
  ]) {
    await assert.rejects(
      expect(() => {}).toPass(options),
      /timeout must be|intervals must contain/,
    );
  }
});
