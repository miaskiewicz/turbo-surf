import assert from "node:assert/strict";
import { test } from "@playwright/test";

test("steps expose info and skip only the current step", async () => {
  await test.step("conditional", async (step) => {
    assert.equal(typeof step, "object");
    step.skip(true, "skip this step");
    throw Error("skipped step continued");
  });
  await test.step.skip("disabled", async () => {
    throw Error("disabled step ran");
  });
  assert.equal(
    await test.step("returns value", async (step) => {
      step.skip(false);
      return 42;
    }),
    42,
  );
  await assert.rejects(
    test.step("forwards errors", async () => {
      throw Error("step failure");
    }),
    /step failure/,
  );
  assert.equal(
    await test.step("outer", async () => {
      await test.step("inner", async (step) => step.skip());
      return "outer continued";
    }),
    "outer continued",
  );
});
