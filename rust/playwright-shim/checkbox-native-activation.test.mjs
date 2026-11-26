import assert from "node:assert/strict";
import test from "node:test";
import { BrowserContext } from "./index.mjs";

test("synthetic checkbox activation toggles once and respects cancellation", async () => {
  const context = new BrowserContext({ animations: "disabled" });
  try {
    const page = await context.newPage();
    await page.setContent("<input id=box type=checkbox>");
    await page.evaluate(() => {
      const box = document.querySelector("#box");
      globalThis.events = [];
      globalThis.cancel = false;
      for (const type of ["click", "input", "change"])
        box.addEventListener(type, (event) => {
          events.push([type, box.checked]);
          if (cancel && type === "click") event.preventDefault();
        });
    });
    const box = page.locator("#box");
    await box.dispatchEvent("click");
    assert.deepEqual(await page.evaluate(() => events), [
      ["click", true],
      ["input", true],
      ["change", true],
    ]);
    assert.equal(await box.isChecked(), true);
    await page.evaluate(() => {
      events.length = 0;
      cancel = true;
    });
    await box.dispatchEvent("click");
    assert.deepEqual(await page.evaluate(() => events), [["click", false]]);
    assert.equal(await box.isChecked(), true);
  } finally {
    await context.close();
  }
});
