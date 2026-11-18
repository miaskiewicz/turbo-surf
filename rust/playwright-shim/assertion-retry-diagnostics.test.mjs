import assert from "node:assert/strict";
import { test } from "node:test";
import { expect, Locator } from "./index.mjs";

function readingLocator(read, context) {
  let observation = 0;
  context.mock.method(Date, "now", () => observation);
  const page = {
    _live: true,
    _ensureLive: async () => {},
    _redrain: async () => {},
  };
  const locator = new Locator(page, () => []);
  locator._prepareQuery = async () => {};
  locator._liveRead = async (...args) => {
    observation++;
    return read(...args);
  };
  return locator;
}

// Advance only on observations: scheduler stalls cannot exhaust the test budget.
const options = { timeout: 3 };

test("visibility retries recover from a transient underlying read error", async (context) => {
  let reads = 0;
  const locator = readingLocator(async () => {
    if (++reads === 1) throw new Error("temporary native read failure");
    return true;
  }, context);
  await expect(locator).toBeVisible(options);
  assert.equal(reads, 2);
});

test("visibility exhaustion preserves the latest genuine read error", async (context) => {
  const errors = [];
  const locator = readingLocator(async () => {
    const error = new Error(`native ownership read failure ${errors.length + 1}`);
    errors.push(error);
    throw error;
  }, context);
  await assert.rejects(expect(locator).toBeVisible(options), (error) => {
    assert.ok(errors.length >= 2);
    assert.equal(error.cause, errors.at(-1));
    assert.match(error.message, /expected element to be visible/);
    assert.ok(error.message.includes(errors.at(-1).message));
    return true;
  });
});

test("a valid false observation clears a stale transient read error", async (context) => {
  let reads = 0;
  const locator = readingLocator(async () => {
    if (++reads === 1) throw new Error("old native failure");
    return false;
  }, context);
  await assert.rejects(expect(locator).toBeVisible(options), (error) => {
    assert.ok(reads >= 2);
    assert.equal(error.message, "expected element to be visible");
    assert.equal(error.cause, undefined);
    return true;
  });
});

test("ordinary invisibility retains the existing assertion diagnostic", async (context) => {
  await assert.rejects(
    expect(readingLocator(async () => false, context)).toBeVisible({ timeout: 0 }),
    {
      message: "expected element to be visible",
    },
  );
});

test("strict locator errors still escape immediately without assertion wrapping", async (context) => {
  const failure = new Error("strict mode violation: locator resolved to 2 elements");
  let reads = 0;
  const locator = readingLocator(async () => {
    reads++;
    throw failure;
  }, context);
  await assert.rejects(expect(locator).toBeVisible(options), (error) => error === failure);
  assert.equal(reads, 1);
});
